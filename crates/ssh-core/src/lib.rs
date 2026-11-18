mod askpass;
mod credentials;
#[cfg(unix)]
mod direct;
mod platform;
mod profile_cache;
mod profiles;
mod sessions;
mod subsystem;

#[cfg(unix)]
pub use direct::DirectSession;
pub use platform::ssh_program;
#[cfg(windows)]
pub use platform::WindowsJob;
pub use profile_cache::ProfileCache;
pub use profiles::{load_profiles, save_profiles, Profile};
pub use sessions::{ssh_command, SessionEvent, SessionManager};
#[cfg(feature = "test-support")]
pub use subsystem::sftp_command_with_config;
pub use subsystem::{sftp_command, SubsystemAuthentication};

pub use askpass::run_askpass_helper;
pub use credentials::{
    password_for, store_connections, CredentialStore, PasswordUpdate, SystemCredentials,
};
