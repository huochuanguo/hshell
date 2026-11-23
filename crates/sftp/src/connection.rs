use futures_util::StreamExt;
use openssh_sftp_client::{Sftp, SftpOptions};
use ssh_core::SubsystemAuthentication;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Child};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: PathBuf,
    pub directory: bool,
    pub symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<u32>,
    pub owner_name: Option<String>,
}

// SFTP v3 的结构化属性只提供 UID，OpenSSH 在目录条目的长格式中附带用户名。
// 仅在识别出类 Unix 列表格式时读取所有者，跳过纯数字或无法识别的值。
fn listed_owner_name(longname: &[u8]) -> Option<&str> {
    let mut fields = longname
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty());
    let [kind, mode @ ..] = fields.next()? else {
        return None;
    };
    let (permissions, suffix) = mode.split_at_checked(9)?;
    let allowed = [
        &b"r-"[..],
        b"w-",
        b"xsS-",
        b"r-",
        b"w-",
        b"xsS-",
        b"r-",
        b"w-",
        b"xtT-",
    ];
    if !b"-dlbcpswD".contains(kind)
        || !matches!(suffix, [] | [b'+' | b'.' | b'@'])
        || !permissions
            .iter()
            .zip(allowed)
            .all(|(bit, allowed)| allowed.contains(bit))
        || !fields.next()?.iter().all(u8::is_ascii_digit)
    {
        return None;
    }

    let owner = std::str::from_utf8(fields.next()?).ok()?;
    fields.next()?; // 所属组
    let size = fields.next()?;
    fields.next()?;
    let digits = owner.trim_start_matches(['+', '-']);
    (!digits.as_bytes().iter().all(u8::is_ascii_digit)
        && !owner.chars().any(char::is_control)
        && owner != "?"
        && size.iter().all(u8::is_ascii_digit))
    .then_some(owner)
}

/// One independently cancellable OpenSSH subsystem; never shares a terminal PTY.
pub struct Connection {
    pub(crate) sftp: Sftp,
    child: Child,
    _authentication: SubsystemAuthentication,
    diagnostic: Arc<Mutex<String>>,
    _group: ProcessGroup,
    #[cfg(windows)]
    _job: ssh_core::WindowsJob,
}

struct ProcessGroup(u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(not(unix))]
        let _ = self.0;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

impl Connection {
    pub async fn open(mut command: Command, auth: SubsystemAuthentication) -> Result<Self, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            #[cfg(target_os = "linux")]
            {
                let parent = std::process::id() as libc::pid_t;
                // Also close the transport after a forced application crash.
                unsafe {
                    command.pre_exec(move || {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        if libc::getppid() != parent {
                            libc::_exit(1);
                        }
                        Ok(())
                    });
                }
            }
        }
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|e| format!("启动 SFTP 失败：{e}"))?;
        let group = ProcessGroup(child.id().ok_or("SFTP 进程已退出")?);
        #[cfg(windows)]
        let job = match ssh_core::WindowsJob::attach(group.0) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill().await;
                return Err(error);
            }
        };
        let diagnostic = Arc::new(Mutex::new(String::new()));
        let errors = diagnostic.clone();
        let mut stderr = child.stderr.take().ok_or("SFTP 错误通道不可用")?;
        tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(length) = stderr.read(&mut buffer).await {
                if length == 0 {
                    break;
                }
                let mut text = errors.lock().unwrap();
                text.push_str(&String::from_utf8_lossy(&buffer[..length]));
                if text.len() > 16384 {
                    let mut start = text.len() - 8192;
                    while !text.is_char_boundary(start) {
                        start += 1;
                    }
                    text.drain(..start);
                }
            }
        });
        let stdin = child.stdin.take().ok_or("SFTP 输入通道不可用")?;
        let stdout = child.stdout.take().ok_or("SFTP 输出通道不可用")?;
        let options = SftpOptions::default()
            .flush_interval(Duration::ZERO)
            .tokio_compat_file_write_limit(NonZeroUsize::new(1024 * 1024).unwrap());
        let sftp =
            match tokio::time::timeout(Duration::from_secs(150), Sftp::new(stdin, stdout, options))
                .await
            {
                Ok(Ok(sftp)) => sftp,
                result => {
                    let _ = child.kill().await;
                    return Err(format!(
                        "SFTP 连接失败：{} {}",
                        match result {
                            Ok(Err(e)) => e.to_string(),
                            _ => "连接或认证超时".into(),
                        },
                        diagnostic.lock().unwrap().trim()
                    ));
                }
            };
        Ok(Self {
            sftp,
            child,
            _authentication: auth,
            diagnostic,
            _group: group,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn diagnostic(&self) -> String {
        self.diagnostic.lock().unwrap().clone()
    }

    /// Streams small batches; callers provide backpressure instead of collecting
    /// an unbounded directory or posting one GUI event per entry.
    pub async fn list(
        &self,
        path: &Path,
        mut batch: impl FnMut(Vec<Entry>) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        let mut fs = self.sftp.fs();
        let path = fs.canonicalize(path).await.map_err(|e| e.to_string())?;
        let mut entries = Box::pin(
            fs.open_dir(&path)
                .await
                .map_err(|e| e.to_string())?
                .read_dir(),
        );
        let mut page = Vec::with_capacity(128);
        let mut count = 0usize;
        while let Some(entry) = entries.next().await {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.filename();
            if name.as_os_str() == std::ffi::OsStr::new(".")
                || name.as_os_str() == std::ffi::OsStr::new("..")
            {
                continue;
            }
            // 文件名按服务器的 Unix 规则校验，避免 Windows 将反斜杠和冒号误判。
            #[cfg(unix)]
            let valid = name.components().count() == 1 && !name.is_absolute();
            #[cfg(not(unix))]
            let valid = name.to_str().is_some_and(crate::remote_name);
            if !valid {
                return Err("服务器返回无效文件名".into());
            }
            let metadata = entry.metadata();
            let kind = entry.file_type();
            count += 1;
            if count > 50_000 {
                return Err("目录超过 50,000 项，请直接输入更具体的子目录".into());
            }
            page.push(Entry {
                name: name.into(),
                directory: kind.is_some_and(|t| t.is_dir()),
                symlink: kind.is_some_and(|t| t.is_symlink()),
                size: metadata.len(),
                modified: metadata.modified().map(|time| time.into_raw()),
                owner_name: listed_owner_name(entry.longname()).map(str::to_owned),
            });
            if page.len() == 128 {
                batch(std::mem::replace(&mut page, Vec::with_capacity(128)))?;
            }
        }
        if !page.is_empty() {
            batch(page)?;
        }
        Ok(path)
    }

    pub async fn mkdir(&self, path: &Path) -> Result<(), String> {
        self.sftp
            .fs()
            .create_dir(path)
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        // Explicitly refuse an existing destination, rather than silently use
        // posix-rename's overwrite semantics for an ordinary rename action.
        if self.sftp.fs().symlink_metadata(to).await.is_ok() {
            return Err("目标名称已存在".into());
        }
        self.sftp
            .fs()
            .rename(from, to)
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn remove(&self, path: &Path, directory: bool) -> Result<(), String> {
        let mut fs = self.sftp.fs();
        if directory {
            fs.remove_dir(path).await
        } else {
            fs.remove_file(path).await
        }
        .map_err(|e| e.to_string())
    }

    pub async fn close(self) {
        // A broken network must not hold application shutdown hostage.
        let Self {
            sftp,
            mut child,
            _group,
            #[cfg(windows)]
            _job,
            ..
        } = self;
        let _ = tokio::time::timeout(Duration::from_secs(1), sftp.close()).await;
        if tokio::time::timeout(Duration::from_secs(1), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::listed_owner_name;

    #[test]
    fn reads_names_from_unix_sftp_listings() {
        assert_eq!(
            listed_owner_name(
                b"-rw-r--r--    1 root     root          23 Jan  1  1970 file name.txt"
            ),
            Some("root")
        );
        assert_eq!(
            listed_owner_name("drwxr-sr-x+  2 ops<研发>& 1000 4096 Aug 14  1:05 目录".as_bytes()),
            Some("ops<研发>&")
        );
        assert_eq!(
            listed_owner_name(b"lrwxrwxrwx   1 www-data  www-data      10 Jan  1  1970 file-\xff"),
            Some("www-data")
        );
    }

    #[test]
    fn does_not_guess_names_from_ids_or_unrecognized_listings() {
        for longname in [
            "",
            "report.txt",
            "report.txt 1 filename field 23 another",
            "-rw-r--r-- 1",
            "-rw-r--r-- 1 4294967294 root 23 Jan 1 1970 file.txt",
            "-rw-r--r-- 1 -1 root 23 Jan 1 1970 file.txt",
            "-rw-r--r-- 1 ? root 23 Jan 1 1970 file.txt",
            "-rw-r--r-- 1 first last staff 23 Jan 1 1970 file.txt",
        ] {
            assert_eq!(listed_owner_name(longname.as_bytes()), None, "{longname}");
        }
        assert_eq!(
            listed_owner_name(b"-rw-r--r-- 1 invalid-\xff root 23 Jan 1 1970 file.txt"),
            None
        );
    }
}
