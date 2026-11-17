use crate::{askpass::AskpassBroker, Profile};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
};
use zeroize::Zeroizing;

const MAX_SESSIONS: usize = 12;
const OUTPUT_CHUNK: usize = 16 * 1024;
const OUTPUT_WINDOW: usize = 8;

pub enum SessionEvent {
    Data(Vec<u8>),
    Auth {
        request_id: String,
        prompt: String,
        confirm: bool,
    },
    Exit {
        code: Option<u32>,
        error: Option<String>,
    },
}

#[derive(Default)]
struct Flow {
    pending: usize,
    closed: bool,
}

struct Session {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    flow: Mutex<Flow>,
    ready: Condvar,
    askpass: Option<AskpassBroker>,
}

impl Session {
    fn stop(&self) {
        self.flow.lock().unwrap().closed = true;
        self.ready.notify_all();
        let _ = self.killer.lock().unwrap().kill();
    }
}

#[derive(Clone, Default)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
    shutting_down: Arc<AtomicBool>,
}

fn size(cols: u16, rows: u16) -> Result<PtySize, String> {
    if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        return Err("终端尺寸超出范围".into());
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}

pub fn ssh_command(profile: &Profile) -> Result<CommandBuilder, String> {
    profile.validate()?;
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "ssh.exe" } else { "ssh" });
    cmd.args([
        "-tt",
        "-e",
        "none",
        "-o",
        "StrictHostKeyChecking=ask",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ConnectTimeout=15",
        // A tab owns its process: don't leave multiplexing masters alive on exit.
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "ControlPersist=no",
        "-o",
        "PermitLocalCommand=no",
    ]);
    if !profile.username.is_empty() {
        cmd.args(["-l", &profile.username]);
    }
    if let Some(port) = profile.port {
        cmd.args(["-p", &port.to_string()]);
    }
    if !profile.identity_file.is_empty() {
        cmd.args(["-i", &profile.identity_file]);
    }
    cmd.args(["--", &profile.host]);
    cmd.env("TERM", "xterm-256color");
    // Askpass would hide prompts in another app or fail in headless sessions.
    cmd.env("SSH_ASKPASS_REQUIRE", "never");
    Ok(cmd)
}

impl SessionManager {
    pub fn start(
        &self,
        id: String,
        profile: &Profile,
        cols: u16,
        rows: u16,
        sink: impl Fn(SessionEvent) -> bool + Send + Sync + 'static,
    ) -> Result<(), String> {
        self.start_command(id, ssh_command(profile)?, cols, rows, None, sink)
    }

    #[cfg(all(feature = "test-support", unix))]
    pub fn start_probe(
        &self,
        id: String,
        cols: u16,
        rows: u16,
        sink: impl Fn(SessionEvent) -> bool + Send + Sync + 'static,
    ) -> Result<(), String> {
        if let Ok(config) = std::env::var("HSHELL_PERF_SSH_CONFIG") {
            let profile = Profile {
                id: "perf-probe".into(),
                name: "SSH loopback fixture".into(),
                host: "fixture-perf".into(),
                username: String::new(),
                port: None,
                identity_file: String::new(),
                group: String::new(),
                favorite: false,
                password_saved: false,
            };
            let mut command = ssh_command(&profile)?;
            command
                .get_argv_mut()
                .splice(1..1, ["-F".into(), config.into()]);
            return self.start_command(id, command, cols, rows, None, sink);
        }
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "stty -echo; printf 'probe-ready\\n'; exec cat"]);
        self.start_command(id, command, cols, rows, None, sink)
    }

    pub fn start_with_password(
        &self,
        id: String,
        profile: &Profile,
        cols: u16,
        rows: u16,
        password: Zeroizing<String>,
        sink: impl Fn(SessionEvent) -> bool + Send + Sync + 'static,
    ) -> Result<(), String> {
        self.start_command(id, ssh_command(profile)?, cols, rows, Some(password), sink)
    }

    pub fn auth_respond(
        &self,
        id: &str,
        request_id: &str,
        answer: Option<String>,
    ) -> Result<(), String> {
        self.get(id)?
            .askpass
            .as_ref()
            .ok_or("此会话没有认证请求")?
            .respond(request_id, answer)
    }

    fn start_command(
        &self,
        id: String,
        mut command: CommandBuilder,
        cols: u16,
        rows: u16,
        password: Option<Zeroizing<String>>,
        sink: impl Fn(SessionEvent) -> bool + Send + Sync + 'static,
    ) -> Result<(), String> {
        let sink = Arc::new(sink);
        let auth_sink = sink.clone();
        let askpass = password
            .map(|password| {
                AskpassBroker::attach(
                    &mut command,
                    password,
                    move |request_id, prompt, confirm| {
                        auth_sink(SessionEvent::Auth {
                            request_id,
                            prompt,
                            confirm,
                        })
                    },
                )
            })
            .transpose()?;
        uuid::Uuid::parse_str(&id).map_err(|_| "会话标识无效")?;
        let dimensions = size(cols, rows)?;
        let mut sessions = self.sessions.lock().unwrap();
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("应用正在退出，无法创建新会话".into());
        }
        if sessions.len() >= MAX_SESSIONS {
            return Err(format!("最多同时打开 {MAX_SESSIONS} 个 SSH 会话"));
        }
        if sessions.contains_key(&id) {
            return Err("会话标识已存在".into());
        }
        let pair = native_pty_system()
            .openpty(dimensions)
            .map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("启动 OpenSSH 失败，请检查是否安装 SSH 客户端：{e}"))?;
        drop(pair.slave);
        let session = Arc::new(Session {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(child.clone_killer()),
            flow: Mutex::new(Flow::default()),
            ready: Condvar::new(),
            askpass,
        });
        sessions.insert(id.clone(), session.clone());
        let manager = self.clone();
        thread::spawn(move || {
            let mut buf = vec![0; OUTPUT_CHUNK];
            let mut read_error = None;
            loop {
                {
                    let mut flow = session.flow.lock().unwrap();
                    while flow.pending >= OUTPUT_WINDOW && !flow.closed {
                        flow = session.ready.wait(flow).unwrap();
                    }
                    if flow.closed {
                        break;
                    }
                }
                let length = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Linux PTYs report EIO once the slave closes.
                    Err(e) if cfg!(target_os = "linux") && e.raw_os_error() == Some(5) => break,
                    Err(e) => {
                        read_error = Some(e.to_string());
                        break;
                    }
                };
                session.flow.lock().unwrap().pending += 1;
                if !sink(SessionEvent::Data(buf[..length].to_vec())) {
                    session.stop();
                    break;
                }
            }
            if read_error.is_some() {
                session.stop();
            }
            let result = child.wait();
            let (code, error) = match result {
                Ok(status) => (Some(status.exit_code()), read_error),
                Err(e) => (None, Some(e.to_string())),
            };
            manager.sessions.lock().unwrap().remove(&id);
            sink(SessionEvent::Exit { code, error });
        });
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Arc<Session>, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| "SSH 会话已结束".into())
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        if data.len() > 64 * 1024 {
            return Err("单次输入超过 64 KiB".into());
        }
        let session = self.get(id)?;
        let mut writer = session.writer.lock().unwrap();
        writer
            .write_all(data)
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let dimensions = size(cols, rows)?;
        self.get(id)?
            .master
            .lock()
            .unwrap()
            .resize(dimensions)
            .map_err(|e| e.to_string())
    }

    pub fn acknowledge(&self, id: &str) {
        if let Ok(session) = self.get(id) {
            let mut flow = session.flow.lock().unwrap();
            flow.pending = flow.pending.saturating_sub(1);
            session.ready.notify_one();
        }
    }

    pub fn close(&self, id: &str) {
        if let Ok(session) = self.get(id) {
            session.stop();
        }
    }

    pub fn close_all(&self) {
        self.shutting_down.store(true, Ordering::Release);
        for session in self.sessions.lock().unwrap().values() {
            session.stop();
        }
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn command_preserves_config_and_enforces_host_verification() {
        let profile: Profile = serde_json::from_str(r#"{"id":"x","name":"dev","host":"dev","username":"","port":null,"identityFile":"","group":"","favorite":false}"#).unwrap();
        let cmd = ssh_command(&profile).unwrap();
        let args: Vec<_> = cmd.get_argv().iter().map(|s| s.to_string_lossy()).collect();
        assert!(args.iter().any(|s| s == "StrictHostKeyChecking=ask"));
        assert!(!args.iter().any(|s| s == "-p" || s == "-l" || s == "-F"));
        assert_eq!(&args[args.len() - 2..], ["--", "dev"]);
    }

    #[test]
    #[cfg(unix)]
    fn pty_roundtrip_resize_exit_and_cleanup() {
        let manager = SessionManager::default();
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args([
            "-c",
            "read answer; stty size; printf 'received:%s' \"$answer\"",
        ]);
        let ack = manager.clone();
        let ack_id = id.clone();
        manager
            .start_command(id.clone(), command, 80, 24, None, move |event| {
                let data = matches!(&event, SessionEvent::Data(_));
                let ok = tx.send(event).is_ok();
                if data {
                    ack.acknowledge(&ack_id);
                }
                ok
            })
            .unwrap();
        manager.resize(&id, 100, 35).unwrap();
        manager.write(&id, b"hello\n").unwrap();
        let mut output = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                SessionEvent::Auth { .. } => panic!("unexpected auth"),
                SessionEvent::Data(data) => output.extend(data),
                SessionEvent::Exit { code, error } => {
                    assert_eq!(code, Some(0));
                    assert!(error.is_none());
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("35 100"), "{text}");
        assert!(text.contains("received:hello"), "{text}");
        assert_eq!(manager.count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn unacknowledged_output_is_bounded_and_close_unblocks_reader() {
        let manager = SessionManager::default();
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "exec yes load-test"]);
        manager
            .start_command(id.clone(), command, 80, 24, None, move |event| {
                tx.send(event).is_ok()
            })
            .unwrap();
        let mut bytes = 0;
        for _ in 0..OUTPUT_WINDOW {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                SessionEvent::Data(data) => bytes += data.len(),
                _ => panic!("unexpected exit"),
            }
        }
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert!(bytes <= OUTPUT_CHUNK * OUTPUT_WINDOW);
        manager.close(&id);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            SessionEvent::Exit { .. }
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        while manager.count() != 0 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(manager.count(), 0);
    }

    #[test]
    #[ignore = "Run with python3 scripts/ssh_smoke.py (isolated loopback SSH server)"]
    #[cfg(unix)]
    fn openssh_fixture() {
        use std::{env, ffi::OsString, fs};
        let port: u16 = env::var("HSHELL_TEST_PORT").unwrap().parse().unwrap();
        let fingerprint = env::var("HSHELL_TEST_FINGERPRINT").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let known_hosts = dir.path().join("known_hosts");
        let manager = SessionManager::default();
        for method in [
            "saved-password",
            "saved-wrong-fallback",
            "saved-host-declined",
            "password",
            "publickey",
            "changed-host",
            "saved-changed-host",
        ] {
            if method == "changed-host" || method == "saved-changed-host" {
                fs::write(
                    &known_hosts,
                    format!(
                        "[127.0.0.1]:{port} ssh-rsa {}\n",
                        env::var("HSHELL_TEST_WRONG_KEY").unwrap()
                    ),
                )
                .unwrap();
            }
            if method.starts_with("saved-") && method != "saved-changed-host" {
                let _ = fs::remove_file(&known_hosts);
            }
            let profile = Profile {
                id: "fixture".into(),
                name: "fixture".into(),
                host: "127.0.0.1".into(),
                username: "fixture".into(),
                port: Some(port),
                identity_file: env::var("HSHELL_TEST_KEY").unwrap(),
                group: String::new(),
                favorite: false,
                password_saved: false,
            };
            let mut command = ssh_command(&profile).unwrap();
            let overrides = [
                "-F".to_string(),
                "/dev/null".to_string(),
                "-o".to_string(),
                format!("UserKnownHostsFile={}", known_hosts.display()),
                "-o".to_string(),
                "GlobalKnownHostsFile=/dev/null".to_string(),
                "-o".to_string(),
                format!(
                    "PreferredAuthentications={}",
                    if method.contains("password") || method.starts_with("saved-") {
                        "password"
                    } else {
                        "publickey"
                    }
                ),
                "-o".to_string(),
                "IdentitiesOnly=yes".to_string(),
                "-o".to_string(),
                "IdentityAgent=none".to_string(),
            ];
            command
                .get_argv_mut()
                .splice(1..1, overrides.map(OsString::from));
            let id = uuid::Uuid::new_v4().to_string();
            let (tx, rx) = mpsc::channel();
            let ack = manager.clone();
            let ack_id = id.clone();
            manager
                .start_command(
                    id.clone(),
                    command,
                    80,
                    24,
                    if method.starts_with("saved-") {
                        Some(Zeroizing::new(
                            if method == "saved-wrong-fallback" {
                                "wrong-password"
                            } else {
                                "fixture-only"
                            }
                            .into(),
                        ))
                    } else {
                        None
                    },
                    move |event| {
                        let data = matches!(&event, SessionEvent::Data(_));
                        let ok = tx.send(event).is_ok();
                        if data {
                            ack.acknowledge(&ack_id);
                        }
                        ok
                    },
                )
                .unwrap();
            let mut output = String::new();
            let mut trusted = false;
            let mut authenticated = false;
            let mut sent = false;
            loop {
                match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                    SessionEvent::Auth {
                        request_id,
                        prompt,
                        confirm,
                    } => {
                        assert!(method.starts_with("saved-"));
                        if confirm {
                            assert!(prompt.contains(&fingerprint), "{prompt}");
                            manager
                                .auth_respond(
                                    &id,
                                    &request_id,
                                    if method == "saved-host-declined" {
                                        None
                                    } else {
                                        Some("yes".into())
                                    },
                                )
                                .unwrap();
                        } else {
                            assert_eq!(method, "saved-wrong-fallback", "{prompt}");
                            manager
                                .auth_respond(&id, &request_id, Some("fixture-only".into()))
                                .unwrap();
                        }
                    }
                    SessionEvent::Data(data) => {
                        output.push_str(&String::from_utf8_lossy(&data));
                        if !trusted && output.contains("(yes/no/[fingerprint])?") {
                            assert_eq!(method, "password");
                            assert!(output.contains(&fingerprint));
                            manager.write(&id, b"yes\n").unwrap();
                            trusted = true;
                        }
                        if !authenticated && output.contains("password:") {
                            assert_eq!(method, "password");
                            manager.write(&id, b"fixture-only\n").unwrap();
                            authenticated = true;
                        }
                        if !sent && output.contains("fixture-ready") {
                            manager.resize(&id, 100, 35).unwrap();
                            manager.write(&id, "hello-中文\n".as_bytes()).unwrap();
                            sent = true;
                        }
                    }
                    SessionEvent::Exit { code, error } => {
                        assert!(error.is_none(), "{error:?}");
                        if method == "changed-host" || method == "saved-changed-host" {
                            assert_eq!(code, Some(255), "{output}");
                            assert!(
                                output.contains("REMOTE HOST IDENTIFICATION HAS CHANGED"),
                                "{output}"
                            );
                            assert!(!sent);
                        } else if method == "saved-host-declined" {
                            assert_eq!(code, Some(255), "{output}");
                            assert!(!sent);
                        } else {
                            assert_eq!(code, Some(0), "{output}");
                            assert!(output.contains("fixture-received:hello-中文"), "{output}");
                            assert!(
                                !output.contains("fixture-only"),
                                "password must not be echoed"
                            );
                        }
                        break;
                    }
                }
            }
            assert_eq!(manager.count(), 0);
            println!("OpenSSH {method}: passed");
        }
    }
}
