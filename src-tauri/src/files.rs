use hshell_sftp::{
    remote_absolute, remote_join, BrowseRequest, Browser, BrowserEvent, Direction, Transfer,
    TransferEvent, UploadPreview, WorkerControl, MAX_TRANSFERS,
};
use serde::{Deserialize, Serialize};
use ssh_core::Profile;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{ipc::Channel, Emitter, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    id: String,
    name: String,
    host: String,
    direction: Direction,
    local: PathBuf,
    remote: PathBuf,
    partial: PathBuf,
    status: String,
    bytes: u64,
    total: u64,
    verifying: bool,
    error: Option<String>,
}

struct Task {
    transfer: Transfer,
    info: TaskInfo,
    control: Option<WorkerControl>,
}
impl Task {
    fn new(transfer: Transfer) -> Self {
        let info = TaskInfo {
            id: transfer.id.clone(),
            name: transfer.profile.name.clone(),
            host: transfer.profile.host.clone(),
            direction: transfer.direction,
            local: transfer.local.clone(),
            remote: transfer.remote.clone(),
            partial: transfer.partial_path().into(),
            status: "paused".into(),
            bytes: transfer.done,
            total: transfer.total,
            verifying: false,
            error: None,
        };
        Self {
            transfer,
            info,
            control: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TransferNotice {
    Task {
        task: TaskInfo,
    },
    Auth {
        id: String,
        request_id: String,
        prompt: String,
        confirm: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    name: String,
    directory: bool,
    symlink: bool,
    size: Option<u64>,
    modified: Option<u32>,
    owner_uid: Option<u32>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowseNotice {
    Ready,
    Entries {
        generation: u64,
        entries: Vec<FileEntry>,
    },
    Listed {
        generation: u64,
        path: Option<String>,
        error: Option<String>,
    },
    Changed {
        error: Option<String>,
    },
    Auth {
        request_id: String,
        prompt: String,
        confirm: bool,
    },
    Closed {
        error: String,
    },
}

#[derive(Default)]
struct Inner {
    browser: Option<(String, Browser)>,
    tasks: BTreeMap<String, Task>,
    queue: VecDeque<String>,
    restore_error: Option<String>,
    shutting_down: bool,
    preparing_upload: bool,
}

#[derive(Clone)]
pub struct Files {
    inner: Arc<Mutex<Inner>>,
    profiles: PathBuf,
    journals: PathBuf,
    app: tauri::AppHandle,
}

struct UploadReservation(Files);
impl Drop for UploadReservation {
    fn drop(&mut self) {
        self.0.inner.lock().unwrap().preparing_upload = false;
    }
}

#[derive(Serialize)]
pub struct UploadReport {
    added: usize,
    folders: usize,
    skipped: usize,
}

impl Files {
    pub fn new(app: tauri::AppHandle, profiles: PathBuf, journals: PathBuf) -> Self {
        let mut inner = Inner::default();
        match Transfer::restore(&journals) {
            Ok(tasks) => {
                for transfer in tasks {
                    inner.tasks.insert(transfer.id.clone(), Task::new(transfer));
                }
            }
            Err(error) => inner.restore_error = Some(error),
        }
        Self {
            inner: Arc::new(Mutex::new(inner)),
            profiles,
            journals,
            app,
        }
    }

    fn notify(&self, info: TaskInfo) {
        let _ = self
            .app
            .emit("sftp-transfer", TransferNotice::Task { task: info });
    }

    fn pump(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.shutting_down {
            return;
        }
        let running = inner.tasks.values().filter(|t| t.control.is_some()).count();
        let mut ids = Vec::new();
        while ids.len() < 2usize.saturating_sub(running) {
            let Some(id) = inner.queue.pop_front() else {
                break;
            };
            if inner
                .tasks
                .get(&id)
                .is_some_and(|task| task.info.status == "queued")
            {
                ids.push(id);
            }
        }
        for id in ids {
            let task = inner.tasks.get_mut(&id).unwrap();
            task.info.status = "running".into();
            task.info.error = None;
            let files = self.clone();
            task.control = Some(hshell_sftp::start_transfer(
                task.transfer.clone(),
                self.profiles.clone(),
                move |event| {
                    if let TransferEvent::Auth {
                        id: request_id,
                        prompt,
                        confirm,
                    } = event
                    {
                        return files
                            .app
                            .emit(
                                "sftp-transfer",
                                TransferNotice::Auth {
                                    id: id.clone(),
                                    request_id,
                                    prompt,
                                    confirm,
                                },
                            )
                            .is_ok();
                    }
                    let finished = matches!(event, TransferEvent::Finished(..));
                    let info = {
                        let mut inner = files.inner.lock().unwrap();
                        let Some(task) = inner.tasks.get_mut(&id) else {
                            return false;
                        };
                        match event {
                            TransferEvent::Progress(progress) => {
                                task.info.bytes = progress.bytes;
                                task.info.total = progress.total;
                                task.info.verifying = progress.verifying;
                            }
                            TransferEvent::Finished(transfer, result) => {
                                task.transfer = *transfer;
                                task.control.take();
                                task.info.bytes = task.transfer.done;
                                task.info.total = task.transfer.total;
                                task.info.verifying = false;
                                task.info.status =
                                    if result.is_ok() { "complete" } else { "paused" }.into();
                                task.info.error = result.err();
                            }
                            TransferEvent::Auth { .. } => unreachable!(),
                        }
                        task.info.clone()
                    };
                    files.notify(info);
                    if finished {
                        files.pump();
                    }
                    true
                },
            ));
            self.notify(task.info.clone());
        }
    }

    pub fn busy(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.preparing_upload
            || inner
                .tasks
                .values()
                .any(|t| t.control.is_some() || t.info.status == "queued")
    }

    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.shutting_down = true;
        inner.browser.take();
        for task in inner.tasks.values_mut() {
            if let Some(control) = &task.control {
                control.stop();
            }
        }
    }
}

#[tauri::command]
pub fn sftp_open(
    files: State<Files>,
    id: String,
    profile: Profile,
    on_event: Channel<BrowseNotice>,
) -> Result<(), String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "文件栏标识无效")?;
    profile.validate()?;
    let mut inner = files.inner.lock().unwrap();
    if inner.shutting_down {
        return Err("应用正在退出".into());
    }
    inner.browser.take();
    let browser = Browser::start(profile, files.profiles.clone(), move |event| {
        let notice = match event {
            BrowserEvent::Ready => BrowseNotice::Ready,
            BrowserEvent::Entries(generation, entries) => BrowseNotice::Entries {
                generation,
                entries: entries
                    .into_iter()
                    .map(|entry| FileEntry {
                        name: entry.name.to_string_lossy().into(),
                        directory: entry.directory,
                        symlink: entry.symlink,
                        size: entry.size,
                        modified: entry.modified,
                        owner_uid: entry.owner_uid,
                    })
                    .collect(),
            },
            BrowserEvent::Listed(generation, result) => match result {
                Ok(path) => BrowseNotice::Listed {
                    generation,
                    path: Some(path.to_string_lossy().into()),
                    error: None,
                },
                Err(error) => BrowseNotice::Listed {
                    generation,
                    path: None,
                    error: Some(error),
                },
            },
            BrowserEvent::Changed(result) => BrowseNotice::Changed {
                error: result.err(),
            },
            BrowserEvent::Auth {
                id,
                prompt,
                confirm,
            } => BrowseNotice::Auth {
                request_id: id,
                prompt,
                confirm,
            },
            BrowserEvent::Closed(error) => BrowseNotice::Closed { error },
        };
        on_event.send(notice).is_ok()
    });
    inner.browser = Some((id, browser));
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FileRequest {
    List { generation: u64, path: String },
    Mkdir { path: String, name: String },
    Rename { path: String, name: String },
    Remove { path: String, directory: bool },
}

fn remote(path: String) -> Result<PathBuf, String> {
    if path.len() > 32768 || !remote_absolute(Path::new(&path)) {
        return Err("请输入 Unix 绝对路径".into());
    }
    Ok(path.into())
}

#[tauri::command]
pub fn sftp_request(files: State<Files>, id: String, request: FileRequest) -> Result<(), String> {
    let request = match request {
        FileRequest::List { generation, path } => BrowseRequest::List {
            generation,
            path: if path == "." {
                path.into()
            } else {
                remote(path)?
            },
        },
        FileRequest::Mkdir { path, name } => {
            BrowseRequest::Mkdir(remote_join(&remote(path)?, &name)?)
        }
        FileRequest::Rename { path, name } => {
            let path = remote(path)?;
            let to = remote_join(&hshell_sftp::remote_parent(&path)?, &name)?;
            BrowseRequest::Rename(path, to)
        }
        FileRequest::Remove { path, directory } => {
            if path.trim_end_matches('/').is_empty() {
                return Err("不能删除远程根目录".into());
            }
            BrowseRequest::Remove(remote(path)?, directory)
        }
    };
    let inner = files.inner.lock().unwrap();
    let (_, browser) = inner
        .browser
        .as_ref()
        .filter(|(current, _)| current == &id)
        .ok_or("文件栏已断开")?;
    browser.request(request)
}

#[tauri::command]
pub fn sftp_close(files: State<Files>, id: String) {
    let mut inner = files.inner.lock().unwrap();
    if inner
        .browser
        .as_ref()
        .is_some_and(|(current, _)| current == &id)
    {
        inner.browser.take();
    }
}

#[tauri::command]
pub fn sftp_auth(
    files: State<Files>,
    id: String,
    request_id: String,
    answer: Option<String>,
    transfer: bool,
) -> Result<(), String> {
    let inner = files.inner.lock().unwrap();
    if transfer {
        inner
            .tasks
            .get(&id)
            .and_then(|t| t.control.as_ref())
            .ok_or("传输已结束")?
            .respond(&request_id, answer)
    } else {
        inner
            .browser
            .as_ref()
            .filter(|(current, _)| current == &id)
            .ok_or("文件栏已断开")?
            .1
            .control
            .respond(&request_id, answer)
    }
}

#[tauri::command]
pub fn sftp_tasks(files: State<Files>) -> Result<Vec<TaskInfo>, String> {
    let inner = files.inner.lock().unwrap();
    if let Some(error) = &inner.restore_error {
        return Err(error.clone());
    }
    Ok(inner.tasks.values().map(|task| task.info.clone()).collect())
}

#[tauri::command]
pub fn sftp_local_upload_drives() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        // 位掩码来自系统盘符表，列出磁盘时无需逐盘读取根目录。
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        if mask == 0 {
            return Err(format!("无法读取盘符：{}", std::io::Error::last_os_error()));
        }
        Ok((b'A'..=b'Z')
            .filter(|letter| mask & (1 << (letter - b'A')) != 0)
            .map(|letter| format!("{}:\\", char::from(letter)))
            .collect())
    }
    #[cfg(not(windows))]
    Ok(Vec::new())
}

#[tauri::command]
pub async fn sftp_local_upload_directory(
    path: Option<PathBuf>,
) -> Result<hshell_sftp::LocalDirectory, String> {
    tauri::async_runtime::spawn_blocking(move || hshell_sftp::list_local_upload(path))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn sftp_inspect_upload(
    files: State<'_, Files>,
    id: String,
    directory: String,
    paths: Vec<PathBuf>,
    name: Option<String>,
) -> Result<UploadPreview, String> {
    let answer = {
        let inner = files.inner.lock().unwrap();
        let (_, browser) = inner
            .browser
            .as_ref()
            .filter(|(current, _)| current == &id)
            .ok_or("文件栏已断开")?;
        browser.inspect_upload(remote(directory)?, paths, name)?
    };
    answer
        .await
        .map_err(|_| "SFTP 连接已关闭，未完成上传检查")?
}

#[tauri::command]
pub async fn sftp_prepare_upload(
    files: State<'_, Files>,
    id: String,
    plan: String,
    replace: bool,
) -> Result<UploadReport, String> {
    let files = files.inner().clone();
    let (answer, profile, reservation) = {
        let mut inner = files.inner.lock().unwrap();
        if inner.shutting_down || inner.preparing_upload {
            return Err("当前无法准备上传，请稍后重试".into());
        }
        if inner.restore_error.is_some() {
            return Err("请先修复损坏的续传记录，再创建新任务".into());
        }
        let (_, browser) = inner
            .browser
            .as_ref()
            .filter(|(current, _)| current == &id)
            .ok_or("文件栏已断开")?;
        let profile = browser.profile.clone();
        let answer = browser.prepare_upload(
            plan,
            replace,
            MAX_TRANSFERS.saturating_sub(inner.tasks.len()),
        )?;
        inner.preparing_upload = true;
        (answer, profile, UploadReservation(files.clone()))
    };
    let prepared = answer
        .await
        .map_err(|_| "SFTP 连接已关闭，未完成上传准备")??;
    tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        if files.inner.lock().unwrap().shutting_down {
            return Err("应用正在退出".into());
        }
        let (folders, skipped) = (prepared.folders, prepared.skipped);
        let tasks = prepared.create_transfers(profile, &files.journals)?;
        let mut inner = files.inner.lock().unwrap();
        if inner.shutting_down {
            return Err("应用正在退出，上传任务已保留在续传记录中".into());
        }
        let added = tasks.len();
        for transfer in tasks {
            let mut task = Task::new(transfer);
            task.info.status = "queued".into();
            let id = task.info.id.clone();
            files.notify(task.info.clone());
            inner.tasks.insert(id.clone(), task);
            inner.queue.push_back(id);
        }
        drop(inner);
        files.pump();
        Ok(UploadReport {
            added,
            folders,
            skipped,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_enqueue(
    files: State<'_, Files>,
    profile: Profile,
    direction: Direction,
    local: PathBuf,
    remote_path: String,
    replace: bool,
    skip_existing: Option<bool>,
) -> Result<Option<String>, String> {
    profile.validate()?;
    let files = files.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if direction == Direction::Upload {
            let metadata =
                std::fs::symlink_metadata(&local).map_err(|e| format!("读取本地文件失败：{e}"))?;
            if !metadata.is_file() {
                return Err("上传仅支持普通文件".into());
            }
        } else if skip_existing == Some(true) {
            match std::fs::symlink_metadata(&local) {
                Ok(_) => return Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("检查下载目标失败：{e}")),
            }
        }
        let mut inner = files.inner.lock().unwrap();
        if inner.shutting_down {
            return Err("应用正在退出".into());
        }
        if inner.restore_error.is_some() {
            return Err("请先修复损坏的续传记录，再创建新任务".into());
        }
        if inner.preparing_upload {
            return Err("正在准备上传，请稍后重试".into());
        }
        if inner.tasks.len() >= MAX_TRANSFERS {
            return Err(format!(
                "最多保留 {MAX_TRANSFERS} 个任务，请先移除已完成任务"
            ));
        }
        let mut transfer = Transfer::new(
            direction,
            profile,
            local,
            remote(remote_path)?,
            &files.journals,
        )?;
        if replace {
            transfer.allow_replace()?;
        }
        let id = transfer.id.clone();
        let mut task = Task::new(transfer);
        task.info.status = "queued".into();
        files.notify(task.info.clone());
        inner.tasks.insert(id.clone(), task);
        inner.queue.push_back(id.clone());
        drop(inner);
        files.pump();
        Ok(Some(id))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_task_action(
    files: State<'_, Files>,
    id: String,
    action: String,
) -> Result<(), String> {
    let files = files.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = files.inner.lock().unwrap();
        if inner.shutting_down {
            return Err("应用正在退出".into());
        }
        let task = inner.tasks.get_mut(&id).ok_or("任务不存在")?;
        match action.as_str() {
            "pause" => {
                if let Some(control) = &task.control {
                    control.stop();
                    task.info.status = "pausing".into();
                } else {
                    task.info.status = "paused".into();
                }
            }
            "resume" => {
                if task.control.is_some()
                    || matches!(task.info.status.as_str(), "complete" | "queued")
                {
                    return Err("任务正在运行、排队或已经完成".into());
                }
                task.info.status = "queued".into();
                task.info.error = None;
            }
            "remove" => {
                if task.control.is_some() {
                    return Err("请先暂停任务，等待断点保存后再移除".into());
                }
                task.transfer.forget()?;
                inner.tasks.remove(&id);
                inner.queue.retain(|queued| queued != &id);
                return Ok(());
            }
            _ => return Err("未知任务操作".into()),
        }
        files.notify(task.info.clone());
        inner.queue.retain(|queued| queued != &id);
        if action == "resume" {
            inner.queue.push_back(id);
        }
        drop(inner);
        files.pump();
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn sftp_busy(files: State<Files>) -> bool {
    files.busy()
}

#[tauri::command]
pub fn sftp_shutdown(files: State<Files>) {
    files.shutdown();
}
