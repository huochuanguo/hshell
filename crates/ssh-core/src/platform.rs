use std::path::PathBuf;

pub fn ssh_program() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        // 优先使用 Windows OpenSSH，避免 PATH 中 Git/MSYS 版本混入 ConPTY 会话。
        let root = std::env::var_os("SystemRoot").ok_or("无法确定 Windows 系统目录")?;
        let program = PathBuf::from(root).join("System32/OpenSSH/ssh.exe");
        if !program.is_file() {
            return Err("未安装 Windows OpenSSH 客户端。请在“设置 → 可选功能”中安装 OpenSSH 客户端，然后重新打开 HShell。".into());
        }
        Ok(program)
    }
    #[cfg(not(windows))]
    Ok(PathBuf::from("ssh"))
}

#[cfg(windows)]
pub(crate) fn require_askpass_support() -> Result<(), String> {
    use std::{os::windows::process::CommandExt, sync::OnceLock};
    static SUPPORTED: OnceLock<Result<(), String>> = OnceLock::new();
    SUPPORTED.get_or_init(|| {
        let output = std::process::Command::new(ssh_program()?).arg("-V").creation_flags(0x08000000).output().map_err(|e| format!("无法检测 Windows OpenSSH：{e}"))?;
        let version = String::from_utf8_lossy(&output.stderr);
        let version = version.strip_prefix("OpenSSH_for_Windows_").or_else(|| version.strip_prefix("OpenSSH_")).unwrap_or("");
        let mut parts = version.split(['.', 'p', ',']);
        let major = parts.next().and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
        if output.status.success() && (major, minor) >= (8, 4) { Ok(()) }
        else { Err("保存密码和 SFTP 需要 Windows OpenSSH 8.4 或更新版本。请更新 OpenSSH 客户端后重启 HShell；旧版仍可使用终端内手动认证。".into()) }
    }).clone()
}

#[cfg(windows)]
pub struct WindowsJob(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl WindowsJob {
    pub fn attach(pid: u32) -> Result<Self, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        };
        // 不可继承的 Job 句柄归会话所有，关闭窗口或应用异常退出时回收整个进程树。
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(format!(
                    "创建 SSH 进程组失败：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let job = OwnedHandle::from_raw_handle(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(format!(
                    "配置 SSH 进程组失败：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return Err(format!(
                    "SSH 进程已退出或不可管理：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let process = OwnedHandle::from_raw_handle(handle);
            if AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) == 0 {
                return Err(format!(
                    "管理 SSH 进程树失败：{}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self(job))
        }
    }

    pub fn terminate(&self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 1);
        }
    }
}
