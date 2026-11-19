#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;
use serde::Serialize;
use ssh_core::{Profile, SessionEvent, SessionManager};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{
    ipc::{Channel, InvokeResponseBody, IpcResponse},
    Manager, State,
};

struct AppState {
    sessions: SessionManager,
    profiles_path: PathBuf,
    config_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TerminalEvent {
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

#[tauri::command]
async fn get_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>, String> {
    let path = state.profiles_path.clone();
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::load_profiles(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_profiles(
    state: State<'_, AppState>,
    profiles: Vec<Profile>,
    password_update: Option<ssh_core::PasswordUpdate>,
) -> Result<Vec<Profile>, String> {
    let path = state.profiles_path.clone();
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::store_connections(
            &path,
            profiles,
            password_update,
            &ssh_core::SystemCredentials,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_snippets(state: State<'_, AppState>) -> Result<Vec<ssh_core::Snippet>, String> {
    let path = state.profiles_path.with_file_name("commands.json");
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::load_snippets(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_snippets(
    state: State<'_, AppState>,
    snippets: Vec<ssh_core::Snippet>,
) -> Result<(), String> {
    let path = state.profiles_path.with_file_name("commands.json");
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::save_snippets(&path, &snippets)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_connections(
    state: State<'_, AppState>,
    destination: PathBuf,
) -> Result<usize, String> {
    let path = state.profiles_path.clone();
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::export_connections(&path, &destination)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn import_connections(
    state: State<'_, AppState>,
    source: PathBuf,
) -> Result<ssh_core::ImportResult, String> {
    let path = state.profiles_path.clone();
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().unwrap();
        ssh_core::import_connections(&path, &source)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_session(
    state: State<'_, AppState>,
    id: String,
    profile: Profile,
    cols: u16,
    rows: u16,
    on_event: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let manager = state.sessions.clone();
    let path = state.profiles_path.clone();
    let lock = state.config_lock.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 只从匹配的已保存连接读取凭据，不能信任前端的 passwordSaved 标记。
        let saved = {
            let _guard = lock.lock().unwrap();
            ssh_core::load_profiles(&path)?.into_iter().find(|p| {
                p.id == profile.id
                    && p.host == profile.host
                    && p.username == profile.username
                    && p.port == profile.port
            })
        };
        let password = saved
            .filter(|p| p.password_saved)
            .map(|p| ssh_core::password_for(&ssh_core::SystemCredentials, &p))
            .transpose()?;
        let sink = move |event| {
            let body = match event {
                SessionEvent::Data(data) => {
                    return on_event.send(InvokeResponseBody::Raw(data)).is_ok()
                }
                SessionEvent::Auth {
                    request_id,
                    prompt,
                    confirm,
                } => TerminalEvent::Auth {
                    request_id,
                    prompt,
                    confirm,
                }
                .body(),
                SessionEvent::Exit { code, error } => TerminalEvent::Exit { code, error }.body(),
            };
            body.is_ok_and(|body| on_event.send(body).is_ok())
        };
        #[cfg(all(feature = "perf-probe", unix))]
        if profile.id == "perf-probe" {
            return manager.start_probe(id, cols, rows, sink);
        }
        match password {
            Some(password) => manager.start_with_password(id, &profile, cols, rows, password, sink),
            None => manager.start(id, &profile, cols, rows, sink),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn write_session(state: State<'_, AppState>, id: String, data: String) -> Result<(), String> {
    let manager = state.sessions.clone();
    tauri::async_runtime::spawn_blocking(move || manager.write(&id, data.as_bytes()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn resize_session(state: State<AppState>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    state.sessions.resize(&id, cols, rows)
}

#[tauri::command]
fn ack_output(state: State<AppState>, id: String) {
    state.sessions.acknowledge(&id);
}

#[tauri::command]
fn close_session(state: State<AppState>, id: String) {
    state.sessions.close(&id);
}

#[tauri::command]
fn auth_respond(
    state: State<AppState>,
    id: String,
    request_id: String,
    answer: Option<String>,
) -> Result<(), String> {
    state.sessions.auth_respond(&id, &request_id, answer)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    platform: &'static str,
    ssh_version: String,
    config_path: String,
}

#[tauri::command]
async fn environment(state: State<'_, AppState>) -> Result<Environment, String> {
    let config_path = state.profiles_path.display().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = std::process::Command::new(ssh_core::ssh_program()?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let result = command
            .arg("-V")
            .output()
            .map_err(|e| format!("未找到 OpenSSH：{e}"))?;
        if !result.status.success() {
            return Err("OpenSSH 版本检测失败".to_string());
        }
        Ok(Environment {
            platform: std::env::consts::OS,
            ssh_version: String::from_utf8_lossy(&result.stderr).trim().to_string(),
            config_path,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(feature = "perf-probe")]
#[tauri::command]
fn perf_mode() -> bool {
    std::env::var_os("HSHELL_PERF_SSH_CONFIG").is_some()
}

#[cfg(feature = "perf-probe")]
#[tauri::command]
fn perf_scenario() -> String {
    std::env::var("HSHELL_PERF_SCENARIO").unwrap_or_default()
}

#[cfg(feature = "perf-probe")]
#[tauri::command]
fn perf_checkpoint(state: State<AppState>, result: String) -> usize {
    std::fs::write(
        std::env::var("HSHELL_PERF_RESULT").expect("probe output path"),
        result,
    )
    .unwrap();
    state.sessions.count()
}

#[cfg(feature = "perf-probe")]
#[tauri::command]
fn perf_result(app: tauri::AppHandle, result: String) {
    std::fs::write(
        std::env::var("HSHELL_PERF_RESULT").expect("probe output path"),
        result,
    )
    .unwrap();
    app.exit(0);
}

fn main() {
    if let Some(code) = ssh_core::run_askpass_helper() {
        std::process::exit(code);
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config = app.path().app_config_dir()?;
            app.manage(files::Files::new(
                app.handle().clone(),
                config.join("profiles.json"),
                config.join("transfers"),
            ));
            app.manage(AppState {
                sessions: SessionManager::default(),
                profiles_path: app.path().app_config_dir()?.join("profiles.json"),
                config_lock: Arc::new(Mutex::new(())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_profiles,
            store_profiles,
            get_snippets,
            store_snippets,
            export_connections,
            import_connections,
            start_session,
            write_session,
            resize_session,
            ack_output,
            close_session,
            environment,
            auth_respond,
            files::sftp_open,
            files::sftp_request,
            files::sftp_close,
            files::sftp_auth,
            files::sftp_tasks,
            files::sftp_enqueue,
            files::sftp_task_action,
            files::sftp_busy,
            files::sftp_shutdown,
            #[cfg(feature = "perf-probe")]
            perf_result,
            #[cfg(feature = "perf-probe")]
            perf_mode,
            #[cfg(feature = "perf-probe")]
            perf_scenario,
            #[cfg(feature = "perf-probe")]
            perf_checkpoint,
        ])
        .build(tauri::generate_context!())
        .expect("无法启动HShell");
    app.run(|handle, event| {
        #[cfg(feature = "perf-probe")]
        if matches!(
            event,
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::Destroyed,
                ..
            }
        ) {
            if let Ok(path) = std::env::var("HSHELL_PERF_RESULT") {
                let _ =
                    std::fs::write(PathBuf::from(path).with_extension("destroyed"), "destroyed");
            }
        }
        if matches!(
            event,
            tauri::RunEvent::Exit
                | tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::Destroyed,
                    ..
                }
        ) {
            handle.state::<AppState>().sessions.close_all();
            handle.state::<files::Files>().shutdown();
        }
    });
    hshell_sftp::wait_for_shutdown(std::time::Duration::from_secs(4));
}
