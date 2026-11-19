mod app_directory;
mod askpass;
mod connection_backup;
mod credentials;
mod diagnostics;
#[cfg(unix)]
mod direct;
mod platform;
mod profile_cache;
mod profiles;
mod sessions;
mod snippets;
mod subsystem;

#[cfg(unix)]
pub use direct::DirectSession;
pub use platform::ssh_program;
#[cfg(windows)]
pub use platform::WindowsJob;
pub use profile_cache::ProfileCache;
pub use profiles::{load_profiles, save_profiles, Profile};
pub use sessions::{ssh_command, SessionEvent, SessionManager};
pub use snippets::{load_snippets, save_snippets, validate_snippet_text, Snippet};
#[cfg(feature = "test-support")]
pub use subsystem::sftp_command_with_config;
pub use subsystem::{sftp_command, SubsystemAuthentication};

pub use app_directory::{prepare_app_directory, APP_ID};
pub use askpass::run_askpass_helper;
pub use connection_backup::{export_connections, import_connections, ImportResult};
pub use credentials::{
    password_for, store_connections, CredentialStore, PasswordUpdate, SystemCredentials,
};
pub use diagnostics::ssh_failure_hint;
