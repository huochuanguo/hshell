//! OpenSSH invokes this executable as its local askpass helper. Passwords travel
//! over a short-lived loopback socket, never argv, environment, disk or the PTY.
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

type Reply = Option<Zeroizing<String>>;
type Pending = Arc<Mutex<HashMap<String, mpsc::SyncSender<Reply>>>>;
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    prompt: String,
    confirm: bool,
}

pub struct AskpassBroker {
    closed: Arc<AtomicBool>,
    pending: Pending,
    address: std::net::SocketAddr,
}
impl Drop for AskpassBroker {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.pending.lock().unwrap().clear();
        // Wake the blocking accept without a permanent polling loop.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
    }
}
impl AskpassBroker {
    pub fn attach(
        command: &mut CommandBuilder,
        password: Zeroizing<String>,
        prompt: impl Fn(String, String, bool) -> bool + Send + 'static,
    ) -> Result<Self, String> {
        Self::attach_optional(command, Some(password), prompt)
    }
    pub(crate) fn attach_optional(
        command: &mut CommandBuilder,
        password: Option<Zeroizing<String>>,
        prompt: impl Fn(String, String, bool) -> bool + Send + 'static,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        crate::platform::require_askpass_support()?;
        let expected_prompt = password_prompt_for(command)?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "无法启动本地认证通道")?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let token = uuid::Uuid::new_v4().to_string();
        command.env(
            "HSHELL_ASKPASS_ADDRESS",
            listener.local_addr().unwrap().to_string(),
        );
        command.env("HSHELL_ASKPASS_TOKEN", &token);
        command.env(
            "SSH_ASKPASS",
            std::env::current_exe().map_err(|e| e.to_string())?,
        );
        #[cfg(test)]
        if let Ok(helper) = std::env::var("HSHELL_TEST_ASKPASS") {
            command.env("SSH_ASKPASS", helper);
        }
        command.env("SSH_ASKPASS_REQUIRE", "force");
        // Make password prompt classification deterministic, and exclude remote
        // keyboard-interactive challenges from automatic password submission.
        command.env("LC_ALL", "C.UTF-8");
        let options = [
            "-o",
            "KbdInteractiveAuthentication=no",
            "-o",
            "PreferredAuthentications=publickey,password",
            "-o",
            "BatchMode=no",
        ];
        command
            .get_argv_mut()
            .splice(1..1, options.map(std::ffi::OsString::from));
        let closed = Arc::new(AtomicBool::new(false));
        let pending: Pending = Arc::default();
        let broker = Self {
            closed: closed.clone(),
            pending: pending.clone(),
            address,
        };
        thread::spawn(move || {
            let mut password = password;
            while !closed.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(_) => break,
                };
                if closed.load(Ordering::Acquire) {
                    break;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut bytes = Vec::new();
                if BufReader::new((&stream).take(16384))
                    .read_until(b'\n', &mut bytes)
                    .is_err()
                {
                    continue;
                }
                let mut request: Request = match serde_json::from_slice(&bytes) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                // Some OpenSSH releases omit SSH_ASKPASS_PROMPT for host verification.
                request.confirm |= request.prompt.ends_with("(yes/no/[fingerprint])? ");
                if request.token != token || request.prompt.len() > 8192 {
                    continue;
                }
                let reply = if !request.confirm
                    && expected_prompt.as_ref() == Some(&request.prompt)
                    && password.is_some()
                {
                    password.take()
                } else {
                    let id = uuid::Uuid::new_v4().to_string();
                    let (tx, rx) = mpsc::sync_channel(1);
                    pending.lock().unwrap().insert(id.clone(), tx);
                    let mut answer = None;
                    if prompt(id.clone(), request.prompt, request.confirm) {
                        let deadline = Instant::now() + Duration::from_secs(120);
                        while !closed.load(Ordering::Acquire) && Instant::now() < deadline {
                            match rx.recv_timeout(Duration::from_millis(50)) {
                                Ok(reply) => {
                                    answer = reply;
                                    break;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                                Err(_) => {}
                            }
                        }
                    }
                    pending.lock().unwrap().remove(&id);
                    answer
                };
                if let Some(reply) = reply {
                    let _ = stream.write_all(reply.as_bytes());
                    let _ = stream.write_all(b"\n");
                }
            }
        });
        Ok(broker)
    }
    pub fn respond(&self, id: &str, answer: Option<String>) -> Result<(), String> {
        let answer = answer.map(Zeroizing::new);
        if answer
            .as_ref()
            .is_some_and(|s| s.len() > 8192 || s.contains(['\0', '\r', '\n']))
        {
            return Err("认证输入无效".into());
        }
        self.pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or("认证请求已过期")?
            .send(answer)
            .map_err(|_| "认证请求已取消".into())
    }
}

// Resolve aliases and default usernames without opening a network connection.
// Proxy helpers inherit askpass: do not give the destination password to a jump
// host, even if both endpoints happen to use the same username/hostname.
fn password_prompt_for(command: &CommandBuilder) -> Result<Option<String>, String> {
    let args = command.get_argv();
    let mut resolve = std::process::Command::new(&args[0]);
    resolve
        .arg("-G")
        .args(&args[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        resolve.creation_flags(0x08000000);
    }
    let mut child = resolve.spawn().map_err(|_| "无法解析 SSH 配置")?;
    let stdout = child.stdout.take().ok_or("SSH 配置解析失败")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    // Windows 匿名管道容量较小，必须边读取边等待，避免 ssh -G 写满管道后死锁。
    thread::spawn(move || {
        let mut data = String::new();
        let result = stdout
            .take(1024 * 1024 + 1)
            .read_to_string(&mut data)
            .map(|_| data);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("SSH 配置解析失败".into());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("SSH 配置解析超时".into());
            }
        }
    }
    let data = receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "SSH 配置输出超时")?
        .map_err(|_| "SSH 配置解析失败")?;
    if data.len() > 1024 * 1024 {
        return Err("SSH 配置输出超过 1 MiB".into());
    }
    let value = |name: &str| data.lines().find_map(|line| line.strip_prefix(name));
    if ["proxyjump ", "proxycommand "]
        .iter()
        .any(|key| value(key).is_some_and(|v| v != "none"))
    {
        return Ok(None);
    }
    match (value("user "), value("hostname ")) {
        (Some(user), Some(host)) => Ok(Some(format!("{user}@{host}'s password: "))),
        _ => Err("无法确定 SSH 登录目标".into()),
    }
}

/// Called before Tauri/GTK initialization, including in the installed binary.
/// The environment contains only a random connection capability, not a password.
pub fn run_askpass_helper() -> Option<i32> {
    let address = std::env::var("HSHELL_ASKPASS_ADDRESS").ok()?;
    let token = std::env::var("HSHELL_ASKPASS_TOKEN").ok()?;
    let result = (|| -> std::io::Result<()> {
        let address: std::net::SocketAddr = address.parse().map_err(std::io::Error::other)?;
        if !address.ip().is_loopback() {
            return Err(std::io::Error::other("invalid address"));
        }
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(130)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let request = Request {
            token,
            prompt: std::env::args().nth(1).unwrap_or_default(),
            confirm: std::env::var("SSH_ASKPASS_PROMPT").is_ok_and(|v| v == "confirm"),
        };
        serde_json::to_writer(&mut stream, &request)?;
        stream.write_all(b"\n")?;
        let mut reply = Zeroizing::new(Vec::new());
        (&stream).take(8194).read_to_end(&mut reply)?;
        if reply.is_empty() || reply.len() > 8193 {
            return Err(std::io::Error::other("cancelled"));
        }
        std::io::stdout().write_all(&reply)?;
        Ok(())
    })();
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolved_alias_and_jump_hosts_have_separate_password_scope() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let mut command = CommandBuilder::new(crate::ssh_program().unwrap());
        command.args(["-F", config.to_str().unwrap(), "--", "fixture-alias"]);
        std::fs::write(
            &config,
            "Host fixture-alias\n HostName 127.0.0.1\n User fixture\n Port 2222\n",
        )
        .unwrap();
        assert_eq!(
            password_prompt_for(&command).unwrap().as_deref(),
            Some("fixture@127.0.0.1's password: ")
        );
        std::fs::write(&config, "Host fixture-alias\n HostName 127.0.0.1\n User fixture\n ProxyJump fixture@127.0.0.1:2223\n").unwrap();
        assert!(password_prompt_for(&command).unwrap().is_none());
    }
    #[test]
    fn wrong_local_capability_cannot_read_saved_password() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        std::fs::write(&config, "").unwrap();
        let mut command = CommandBuilder::new(crate::ssh_program().unwrap());
        command.args([
            "-F",
            config.to_str().unwrap(),
            "-l",
            "fixture",
            "--",
            "127.0.0.1",
        ]);
        let _broker = AskpassBroker::attach(
            &mut command,
            Zeroizing::new("fixture-secret".into()),
            |_, _, _| panic!("invalid capability must not prompt"),
        )
        .unwrap();
        let address = command
            .get_env("HSHELL_ASKPASS_ADDRESS")
            .unwrap()
            .to_str()
            .unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let request = Request {
            token: "wrong-token".into(),
            prompt: "fixture@127.0.0.1's password: ".into(),
            confirm: false,
        };
        serde_json::to_writer(&mut stream, &request).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut data = Vec::new();
        stream.read_to_end(&mut data).unwrap();
        assert!(data.is_empty());
    }
}
