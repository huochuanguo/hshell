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
}

/// One independently cancellable OpenSSH subsystem; never shares a terminal PTY.
pub struct Connection {
    pub(crate) sftp: Sftp,
    child: Child,
    _authentication: SubsystemAuthentication,
    diagnostic: Arc<Mutex<String>>,
    _group: ProcessGroup,
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
            if name == Path::new(".") || name == Path::new("..") {
                continue;
            }
            // A server-provided directory entry must be a single path component.
            if name.components().count() != 1 || name.is_absolute() {
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
