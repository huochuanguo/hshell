//! Pipe transport for SFTP. No PTY, shell command, or terminal-output parsing.
use crate::{askpass::AskpassBroker, ssh_command, Profile, SessionEvent};
use std::{process::Command, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct SubsystemAuthentication(Arc<AskpassBroker>);

impl SubsystemAuthentication {
    pub fn respond(&self, id: &str, answer: Option<String>) -> Result<(), String> {
        self.0.respond(id, answer)
    }
}

pub fn sftp_command(
    profile: &Profile,
    password: Option<Zeroizing<String>>,
    prompt: impl Fn(SessionEvent) -> bool + Send + 'static,
) -> Result<(Command, SubsystemAuthentication), String> {
    subsystem_command(ssh_command(profile)?, password, prompt)
}

#[cfg(feature = "test-support")]
pub fn sftp_command_with_config(
    profile: &Profile,
    config: &std::path::Path,
    password: Option<Zeroizing<String>>,
    prompt: impl Fn(SessionEvent) -> bool + Send + 'static,
) -> Result<(Command, SubsystemAuthentication), String> {
    let mut builder = ssh_command(profile)?;
    builder
        .get_argv_mut()
        .splice(1..1, ["-F".into(), config.as_os_str().to_owned()]);
    subsystem_command(builder, password, prompt)
}

fn subsystem_command(
    mut builder: portable_pty::CommandBuilder,
    password: Option<Zeroizing<String>>,
    prompt: impl Fn(SessionEvent) -> bool + Send + 'static,
) -> Result<(Command, SubsystemAuthentication), String> {
    let terminal = builder
        .get_argv_mut()
        .iter_mut()
        .find(|arg| arg.as_os_str() == std::ffi::OsStr::new("-tt"))
        .ok_or("SSH 终端参数缺失")?;
    *terminal = "-T".into();
    builder.get_argv_mut().insert(1, "-s".into());
    builder.arg("sftp");
    let auth = AskpassBroker::attach_optional(
        &mut builder,
        password,
        move |request_id, text, confirm| {
            prompt(SessionEvent::Auth {
                request_id,
                prompt: text,
                confirm,
            })
        },
    )?;
    let args = builder.get_argv();
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    command.envs(builder.iter_extra_env_as_str());
    // 保留完整的认证助手路径，包括 Unix 下的非 UTF-8 安装路径。
    if let Some(helper) = builder.get_env("SSH_ASKPASS") {
        command.env("SSH_ASKPASS", helper);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Ok((command, SubsystemAuthentication(Arc::new(auth))))
}
