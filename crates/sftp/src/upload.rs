use crate::{
    remote_join, remote_name, remote_parent, transfer::is_missing, Connection, Transfer,
    MAX_TRANSFERS,
};
use serde::Serialize;
use ssh_core::Profile;
use std::{
    collections::HashSet,
    fs::Metadata,
    path::{Path, PathBuf},
};

const MAX_UPLOAD_ENTRIES: usize = 8192;
const MAX_UPLOAD_DEPTH: usize = 64;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadPreview {
    pub id: String,
    pub files: usize,
    pub folders: usize,
    pub conflicts: usize,
    pub examples: Vec<String>,
    pub skipped: usize,
}

struct UploadEntry {
    root: PathBuf,
    local: PathBuf,
    remote: PathBuf,
    folder: bool,
    depth: usize,
}

pub struct UploadPlan {
    preview: UploadPreview,
    folders: Vec<PathBuf>,
    files: Vec<(UploadEntry, bool)>,
}

pub struct PreparedUpload {
    files: Vec<(UploadEntry, bool)>,
    pub folders: usize,
    pub skipped: usize,
}

fn local_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // 挂载点和目录联接也能跳出所选目录，按链接处理。
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

pub(crate) fn check_local_upload(root: &Path, file: &Path) -> Result<(), String> {
    if !root.is_absolute() || !file.starts_with(root) {
        return Err("上传源路径无效".into());
    }
    let mut current = file;
    loop {
        let metadata = std::fs::symlink_metadata(current)
            .map_err(|e| format!("读取上传源 {} 失败：{e}", current.display()))?;
        if local_link(&metadata)
            || if current == file {
                !metadata.is_file()
            } else {
                !metadata.is_dir()
            }
        {
            return Err(format!("上传源已改变或变成链接：{}", current.display()));
        }
        if current == root {
            return Ok(());
        }
        current = current.parent().ok_or("上传源路径无效")?;
    }
}

fn scan(
    paths: Vec<PathBuf>,
    directory: &Path,
    name: Option<String>,
) -> Result<(Vec<UploadEntry>, usize), String> {
    if paths.len() > MAX_UPLOAD_ENTRIES {
        return Err(format!("每次最多选择 {MAX_UPLOAD_ENTRIES} 个文件和目录"));
    }
    if name
        .as_deref()
        .is_some_and(|name| paths.len() != 1 || !remote_name(name))
    {
        return Err("上传名称无效".into());
    }
    let mut entries = Vec::new();
    let mut skipped = 0;
    let mut count = paths.len();
    let mut names = HashSet::new();
    let mut pending = Vec::new();
    for path in paths.into_iter().rev() {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err("上传源必须为本地绝对路径".into());
        }
        let path: PathBuf = path.components().collect();
        let Some(base) = name
            .as_deref()
            .or_else(|| path.file_name().and_then(|v| v.to_str()))
        else {
            skipped += 1;
            continue;
        };
        let remote = remote_join(directory, base)?;
        pending.push((path.clone(), path, remote, 0));
    }
    while let Some((root, local, remote, depth)) = pending.pop() {
        if depth > MAX_UPLOAD_DEPTH {
            return Err(format!("每次最多扫描 {MAX_UPLOAD_ENTRIES} 个文件和目录，目录深度不能超过 {MAX_UPLOAD_DEPTH} 层"));
        }
        let metadata = std::fs::symlink_metadata(&local)
            .map_err(|e| format!("读取 {} 失败：{e}", local.display()))?;
        if local_link(&metadata)
            || !(metadata.is_file() || metadata.is_dir())
            || !names.insert(remote.as_os_str().to_owned())
        {
            skipped += 1;
            continue;
        }
        if metadata.is_dir() {
            for child in std::fs::read_dir(&local)
                .map_err(|e| format!("读取目录 {} 失败：{e}", local.display()))?
            {
                let child = child.map_err(|e| format!("读取目录 {} 失败：{e}", local.display()))?;
                count += 1;
                if count > MAX_UPLOAD_ENTRIES {
                    return Err(format!("每次最多扫描 {MAX_UPLOAD_ENTRIES} 个文件和目录"));
                }
                let name = child.file_name();
                let Some(name) = name.to_str() else {
                    skipped += 1;
                    continue;
                };
                pending.push((
                    root.clone(),
                    child.path(),
                    remote_join(&remote, name)?,
                    depth + 1,
                ));
            }
        }
        entries.push(UploadEntry {
            root,
            local,
            remote,
            depth,
            folder: metadata.is_dir(),
        });
    }
    Ok((entries, skipped))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemoteKind {
    Missing,
    File,
    Folder,
    Other,
}

async fn remote_kind(connection: &Connection, path: &Path) -> Result<RemoteKind, String> {
    match connection.sftp.fs().symlink_metadata(path).await {
        Ok(metadata) => Ok(match metadata.file_type() {
            Some(kind) if kind.is_file() => RemoteKind::File,
            Some(kind) if kind.is_dir() => RemoteKind::Folder,
            _ => RemoteKind::Other,
        }),
        Err(error) if is_missing(&error) => Ok(RemoteKind::Missing),
        Err(error) => Err(format!("检查远程路径 {} 失败：{error}", path.display())),
    }
}

pub(crate) async fn require_remote_folder(
    connection: &Connection,
    path: &Path,
) -> Result<(), String> {
    let canonical = connection
        .sftp
        .fs()
        .canonicalize(path)
        .await
        .map_err(|e| format!("读取远程目录 {} 失败：{e}", path.display()))?;
    // Windows 的 Path 相等比较会把反斜杠当作分隔符；Unix 路径须按原始值比较。
    if canonical.as_os_str() != path.as_os_str() {
        return Err(format!("远程目录已改变或变成链接：{}", path.display()));
    }
    Ok(())
}

pub(crate) async fn require_replaceable(
    connection: &Connection,
    path: &Path,
) -> Result<(), String> {
    if !matches!(
        remote_kind(connection, path).await?,
        RemoteKind::File | RemoteKind::Missing
    ) {
        return Err(format!(
            "不能替换远程目录、链接或特殊文件：{}",
            path.display()
        ));
    }
    Ok(())
}

impl UploadPlan {
    pub async fn inspect(
        connection: &Connection,
        directory: PathBuf,
        paths: Vec<PathBuf>,
        name: Option<String>,
    ) -> Result<Self, String> {
        require_remote_folder(connection, &directory).await?;
        let (entries, skipped) = tokio::task::spawn_blocking(move || scan(paths, &directory, name))
            .await
            .map_err(|e| e.to_string())??;
        let mut plan = Self {
            preview: UploadPreview {
                id: uuid::Uuid::new_v4().to_string(),
                files: 0,
                folders: 0,
                conflicts: 0,
                examples: Vec::new(),
                skipped,
            },
            folders: Vec::new(),
            files: Vec::new(),
        };
        let mut parents = Vec::new();
        for entry in entries {
            parents.truncate(entry.depth);
            let kind = match parents.last() {
                Some(RemoteKind::Other) => RemoteKind::Other,
                Some(RemoteKind::Missing) => RemoteKind::Missing,
                _ => remote_kind(connection, &entry.remote).await?,
            };
            match (entry.folder, kind) {
                (true, RemoteKind::Folder | RemoteKind::Missing) => {
                    parents.push(kind);
                    plan.folders.push(entry.remote);
                }
                (true, _) => {
                    parents.push(RemoteKind::Other);
                    plan.preview.skipped += 1;
                }
                (false, RemoteKind::Missing | RemoteKind::File) => {
                    let replace = kind == RemoteKind::File;
                    if replace {
                        plan.preview.conflicts += 1;
                        if plan.preview.examples.len() < 6 {
                            plan.preview
                                .examples
                                .push(entry.remote.to_string_lossy().into_owned());
                        }
                    }
                    plan.files.push((entry, replace));
                }
                _ => plan.preview.skipped += 1,
            }
        }
        plan.preview.files = plan.files.len();
        plan.preview.folders = plan.folders.len();
        Ok(plan)
    }

    pub fn preview(&self) -> &UploadPreview {
        &self.preview
    }

    pub async fn prepare(
        mut self,
        connection: &Connection,
        replace: bool,
        capacity: usize,
    ) -> Result<PreparedUpload, String> {
        let declined = if replace { 0 } else { self.preview.conflicts };
        if self.files.len() - declined > capacity.min(MAX_TRANSFERS) {
            return Err(format!(
                "本次需要 {} 个传输名额，队列剩余 {capacity} 个。请移除已完成任务或分批上传。",
                self.files.len() - declined
            ));
        }
        let mut created = 0;
        for folder in &self.folders {
            // 每次创建前重新检查上级目录，防止确认期间被换成链接。
            require_remote_folder(connection, &remote_parent(folder)?).await?;
            match remote_kind(connection, folder).await? {
                RemoteKind::Folder => {}
                RemoteKind::Missing => {
                    connection
                        .mkdir(folder)
                        .await
                        .map_err(|e| format!("创建远程目录 {} 失败：{e}", folder.display()))?;
                    created += 1;
                }
                _ => {
                    return Err(format!(
                        "远程目录与同名文件或链接冲突：{}",
                        folder.display()
                    ))
                }
            }
        }
        self.files.retain(|(_, overwrite)| !overwrite || replace);
        Ok(PreparedUpload {
            files: self.files,
            folders: created,
            skipped: self.preview.skipped + declined,
        })
    }
}

impl PreparedUpload {
    pub fn create_transfers(self, profile: Profile, state: &Path) -> Result<Vec<Transfer>, String> {
        let mut tasks: Vec<Transfer> = Vec::with_capacity(self.files.len());
        for (file, replace) in self.files {
            match Transfer::new_upload(
                profile.clone(),
                file.local,
                file.remote,
                file.root,
                replace,
                state,
            ) {
                Ok(task) => tasks.push(task),
                Err(error) => {
                    for task in tasks {
                        if let Err(cleanup) = task.forget() {
                            return Err(format!(
                                "保存上传任务失败：{error}；清理续传记录失败：{cleanup}"
                            ));
                        }
                    }
                    return Err(format!("保存上传任务失败：{error}"));
                }
            }
        }
        Ok(tasks)
    }
}
