//! A native terminal owns PTY I/O directly. OpenSSH validation and askpass stay
//! shared with the WebView client, without copying output through an IPC bridge.
use crate::{askpass::AskpassBroker, ssh_command, Profile, SessionEvent};
use portable_pty::{native_pty_system, ChildKiller, MasterPty, PtySize};
use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use zeroize::Zeroizing;

pub struct DirectSession {
    master: Box<dyn MasterPty + Send>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    askpass: Option<AskpassBroker>,
    running: Arc<AtomicBool>,
}

impl DirectSession {
    pub fn start(
        profile: &Profile,
        password: Option<Zeroizing<String>>,
        cols: u16,
        rows: u16,
        sink: impl Fn(SessionEvent) -> bool + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
            return Err("终端尺寸超出范围".into());
        }
        let mut command = ssh_command(profile)?;
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
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("启动 OpenSSH 失败：{error}"))?;
        drop(pair.slave);
        let killer = Mutex::new(child.clone_killer());
        let running = Arc::new(AtomicBool::new(true));
        let child_running = running.clone();
        std::thread::spawn(move || {
            let (code, error) = match child.wait() {
                Ok(status) => (Some(status.exit_code()), None),
                Err(error) => (None, Some(error.to_string())),
            };
            child_running.store(false, Ordering::Release);
            sink(SessionEvent::Exit { code, error });
        });
        Ok(Self {
            master: pair.master,
            killer,
            askpass,
            running,
        })
    }

    pub fn terminal_fd(&self) -> Result<OwnedFd, String> {
        let fd = self.master.as_raw_fd().ok_or("PTY 文件描述符不可用")?;
        // The master remains owned by this session. VTE receives its own fd.
        unsafe { BorrowedFd::borrow_raw(fd) }
            .try_clone_to_owned()
            .map_err(|error| error.to_string())
    }

    pub fn respond(&self, request_id: &str, answer: Option<String>) -> Result<(), String> {
        self.askpass
            .as_ref()
            .ok_or("认证请求已结束")?
            .respond(request_id, answer)
    }

    pub fn close(&self) {
        if self.running.load(Ordering::Acquire) {
            let _ = self.killer.lock().unwrap().kill();
        }
    }
}

impl Drop for DirectSession {
    fn drop(&mut self) {
        self.close();
    }
}
