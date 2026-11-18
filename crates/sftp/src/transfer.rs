use crate::Connection;
use bytes::BytesMut;
use futures_util::{stream::FuturesOrdered, StreamExt};
use openssh_sftp_client::{file::File as RemoteFile, metadata::MetaData};
use serde::{Deserialize, Serialize};
use ssh_core::Profile;
use std::{
    io::{Read, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const BLOCK: usize = 64 * 1024;
const PIPELINE: usize = 32;
const CHECKPOINT: u64 = 8 * 1024 * 1024;

fn is_missing(error: &openssh_sftp_client::Error) -> bool {
    matches!(
        error,
        openssh_sftp_client::Error::SftpError(
            openssh_sftp_client::error::SftpErrorKind::NoSuchFile,
            _
        )
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Upload,
    Download,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub bytes: u64,
    pub total: u64,
    pub verifying: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Revision {
    size: u64,
    modified: u128,
}

/// A journal contains paths and connection identity, never authentication data.
/// Only a private partial file is modified; the destination appears on success.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transfer {
    pub id: String,
    pub profile: Profile,
    pub direction: Direction,
    pub local: PathBuf,
    pub remote: PathBuf,
    pub total: u64,
    pub done: u64,
    #[serde(default)]
    pub replace: bool,
    partial: PathBuf,
    source: Option<Revision>,
    journal: PathBuf,
    #[serde(default)]
    publishing: bool,
}

fn local_revision(metadata: &std::fs::Metadata) -> Result<Revision, String> {
    if !metadata.is_file() {
        return Err("只支持普通文件传输".into());
    }
    Ok(Revision {
        size: metadata.len(),
        modified: metadata
            .modified()
            .map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos(),
    })
}
fn remote_revision(metadata: MetaData) -> Result<Revision, String> {
    if !metadata.file_type().is_some_and(|t| t.is_file()) {
        return Err("远程路径不是普通文件".into());
    }
    Ok(Revision {
        size: metadata.len().ok_or("服务器未提供文件大小")?,
        modified: metadata
            .modified()
            .ok_or("服务器未提供修改时间，无法安全续传")?
            .into_raw() as u128,
    })
}

impl Transfer {
    pub fn new(
        direction: Direction,
        profile: Profile,
        local: PathBuf,
        remote: PathBuf,
        state: &Path,
    ) -> Result<Self, String> {
        if !local.is_absolute() || !crate::remote_absolute(&remote) {
            return Err("传输路径必须为绝对路径".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let partial = if direction == Direction::Upload {
            crate::remote_join(
                &crate::remote_parent(&remote)?,
                &format!(".hshell-{id}.part"),
            )?
        } else {
            local
                .parent()
                .ok_or("目标路径无父目录")?
                .join(format!(".hshell-{id}.part"))
        };
        let job = Self {
            id: id.clone(),
            profile,
            direction,
            local,
            remote,
            total: 0,
            done: 0,
            replace: false,
            partial,
            source: None,
            journal: state.join(format!("{id}.json")),
            publishing: false,
        };
        job.save()?;
        Ok(job)
    }
    pub fn restore(state: &Path) -> Result<Vec<Self>, String> {
        if !state.exists() {
            return Ok(Vec::new());
        }
        let mut jobs = Vec::new();
        for file in std::fs::read_dir(state).map_err(|e| e.to_string())? {
            let file = file.map_err(|e| e.to_string())?.path();
            if file.extension().is_none_or(|e| e != "json") {
                continue;
            }
            if jobs.len() >= 128 {
                return Err("未完成传输超过 128 条，请先处理已有任务".into());
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&file)
                .map_err(|e| e.to_string())?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("续传记录过大".into());
            }
            let mut job: Self =
                serde_json::from_slice(&bytes).map_err(|e| format!("续传记录损坏：{e}"))?;
            let id = uuid::Uuid::parse_str(&job.id).map_err(|_| "无效续传编号")?;
            if file.file_stem().and_then(|s| s.to_str()) != Some(job.id.as_str())
                || id.to_string() != job.id
            {
                return Err("续传记录身份不匹配".into());
            }
            job.profile.validate()?;
            let expected = if job.direction == Direction::Upload {
                crate::remote_join(
                    &crate::remote_parent(&job.remote)?,
                    &format!(".hshell-{}.part", job.id),
                )?
            } else {
                job.local
                    .parent()
                    .ok_or("续传目标无父目录")?
                    .join(format!(".hshell-{}.part", job.id))
            };
            // Windows Path 的组件比较会把反斜杠视为分隔符，远程路径必须比较原始字符串。
            let partial_valid = if job.direction == Direction::Upload {
                job.partial.as_os_str() == expected.as_os_str()
            } else {
                job.partial == expected
            };
            if !partial_valid
                || !job.local.is_absolute()
                || !crate::remote_absolute(&job.remote)
                || job.done > job.total
            {
                return Err("续传记录路径或进度无效".into());
            }
            job.journal = file;
            jobs.push(job);
        }
        Ok(jobs)
    }
    fn save(&self) -> Result<(), String> {
        let parent = self.journal.parent().ok_or("续传记录路径无效")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, self).map_err(|e| e.to_string())?;
        file.flush()
            .and_then(|_| file.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        file.persist(&self.journal).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            std::fs::File::open(parent)
                .and_then(|d| d.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn forget(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.journal) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn partial_path(&self) -> &Path {
        &self.partial
    }
    pub fn allow_replace(&mut self) -> Result<(), String> {
        self.replace = true;
        self.save()
    }

    /// Full prefix comparison is intentionally performed on resumption only.
    /// A size match alone is insufficient to safely append a changed file.
    pub async fn run(
        &mut self,
        connection: &Connection,
        mut progress: impl FnMut(Progress),
    ) -> Result<(), String> {
        // A crash can occur after atomic publication and before journal removal.
        // Only accept the final name after checking every byte against the
        // unchanged source. Never append to the public destination.
        if self.publishing && self.recover_publication(connection, &mut progress).await? {
            return Ok(());
        }
        let upload = self.direction == Direction::Upload;
        let remote_source = if upload { &self.partial } else { &self.remote };
        let mut local_options = tokio::fs::OpenOptions::new();
        local_options.read(true);
        #[cfg(unix)]
        local_options.custom_flags(libc::O_NONBLOCK | if upload { 0 } else { libc::O_NOFOLLOW });
        if !upload {
            local_options.write(true);
        }
        let mut local = if !upload {
            match tokio::fs::symlink_metadata(&self.partial).await {
                Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                    return Err("临时文件不是普通文件，已停止续传".into())
                }
                Ok(_) => local_options.open(&self.partial).await,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && self.done == 0 => {
                    local_options.create_new(true).open(&self.partial).await
                }
                Err(error) => return Err(format!("读取临时文件失败：{error}")),
            }
        } else {
            local_options.open(&self.local).await
        }
        .map_err(|e| format!("打开本地文件失败：{e}"))?;
        if !local.metadata().await.map_err(|e| e.to_string())?.is_file() {
            return Err("本地路径不是普通文件".into());
        }
        let initial = if upload {
            local_revision(&local.metadata().await.map_err(|e| e.to_string())?)?
        } else {
            remote_revision(
                connection
                    .sftp
                    .fs()
                    .metadata(&self.remote)
                    .await
                    .map_err(|e| e.to_string())?,
            )?
        };
        if let Some(source) = &self.source {
            if source != &initial {
                return Err("源文件已改变，已停止续传。请新建传输任务".into());
            }
        }
        let fresh = self.source.is_none();
        if fresh {
            self.total = initial.size;
            self.source = Some(initial.clone());
            self.save()?;
        }
        // Symlink partials are never accepted as application-owned data.
        if !upload
            && tokio::fs::symlink_metadata(&self.partial)
                .await
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("临时文件变为符号链接，已停止续传".into());
        }
        let mut remote = if upload {
            let mut options = connection.sftp.options();
            options.read(true).write(true);
            {
                match connection.sftp.fs().symlink_metadata(&self.partial).await {
                    Ok(meta) if meta.file_type().is_some_and(|t| t.is_file()) => {}
                    Ok(_) => return Err("远程临时文件无效".into()),
                    Err(error) if self.done == 0 && is_missing(&error) => {
                        options.create_new(true);
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            options.open(&self.partial).await
        } else {
            connection.sftp.open(remote_source).await
        }
        .map_err(|e| format!("打开远程文件失败：{e}"))?;
        let present = if upload {
            remote
                .metadata()
                .await
                .map_err(|e| e.to_string())?
                .len()
                .ok_or("临时文件大小不可用")?
        } else {
            local.metadata().await.map_err(|e| e.to_string())?.len()
        };
        let offset = self.done.min(present).min(self.total);
        if offset > 0 {
            let mut checked = 0;
            while checked < offset {
                let batch = remote_batch(&remote, checked, offset).await?;
                for bytes in batch {
                    let mut existing = vec![0; bytes.len()];
                    local
                        .read_exact(&mut existing)
                        .await
                        .map_err(|e| e.to_string())?;
                    if bytes.as_ref() != existing {
                        return Err("已传部分与源文件不一致，已停止续传。请新建任务".into());
                    }
                    checked += bytes.len() as u64;
                }
                progress(Progress {
                    bytes: checked,
                    total: offset,
                    verifying: true,
                });
            }
        }
        self.done = offset;
        local
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
        remote
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
        if upload {
            remote.set_len(offset).await.map_err(|e| e.to_string())?;
        } else {
            local.set_len(offset).await.map_err(|e| e.to_string())?;
        }
        self.save()?;
        let mut last_saved = self.done;
        let mut last_time = Instant::now();
        let mut buffer = vec![0; BLOCK * 8];
        while self.done < self.total {
            if upload {
                let length = (self.total - self.done).min(buffer.len() as u64) as usize;
                local
                    .read_exact(&mut buffer[..length])
                    .await
                    .map_err(|e| e.to_string())?;
                remote_write_batch(&remote, self.done, &buffer[..length]).await?;
                self.done += length as u64;
            } else {
                for bytes in remote_batch(&remote, self.done, self.total).await? {
                    local.write_all(&bytes).await.map_err(|e| e.to_string())?;
                    self.done += bytes.len() as u64;
                }
            }
            if (self.done - last_saved >= CHECKPOINT
                && last_time.elapsed() >= Duration::from_millis(500))
                || last_time.elapsed() >= Duration::from_secs(2)
                || self.done == self.total
            {
                if upload {
                    if connection.sftp.support_fsync() {
                        remote.sync_all().await.map_err(|e| e.to_string())?;
                    }
                } else {
                    local.sync_data().await.map_err(|e| e.to_string())?;
                }
                self.save()?;
                last_saved = self.done;
                last_time = Instant::now();
            }
            progress(Progress {
                bytes: self.done,
                total: self.total,
                verifying: false,
            });
        }
        if upload && connection.sftp.support_fsync() {
            remote.sync_all().await.map_err(|e| e.to_string())?;
        }
        if !upload {
            local.sync_all().await.map_err(|e| e.to_string())?;
        }
        let final_revision = if upload {
            local_revision(&local.metadata().await.map_err(|e| e.to_string())?)?
        } else {
            remote_revision(remote.metadata().await.map_err(|e| e.to_string())?)?
        };
        if final_revision != initial {
            return Err("源文件在传输期间改变，未替换目标文件".into());
        }
        remote.close().await.map_err(|e| e.to_string())?;
        drop(local);
        self.publishing = true;
        self.save()?;
        if upload {
            // Standard rename without posix-rename refuses an existing target.
            // hardlink is atomic and no-clobber on OpenSSH, including concurrent
            // destination creation after the user selected a name.
            if self.replace {
                if !connection.sftp.support_posix_rename() {
                    return Err("服务器不支持原子替换，已保留临时文件；请使用新的目标名称".into());
                }
                connection
                    .sftp
                    .fs()
                    .rename(&self.partial, &self.remote)
                    .await
                    .map_err(|e| e.to_string())?;
            } else if connection.sftp.support_hardlink() {
                connection
                    .sftp
                    .fs()
                    .hard_link(&self.partial, &self.remote)
                    .await
                    .map_err(|e| format!("目标已存在或发布失败：{e}；已保留临时文件"))?;
                connection
                    .sftp
                    .fs()
                    .remove_file(&self.partial)
                    .await
                    .map_err(|e| e.to_string())?;
            } else if !connection.sftp.support_posix_rename() {
                connection
                    .sftp
                    .fs()
                    .rename(&self.partial, &self.remote)
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                return Err("服务器不支持安全创建目标；请确认替换或使用新名称".into());
            }
        } else {
            if self.replace {
                tokio::fs::rename(&self.partial, &self.local)
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                tokio::fs::hard_link(&self.partial, &self.local)
                    .await
                    .map_err(|e| format!("目标已存在或发布失败：{e}；已保留临时文件"))?;
                tokio::fs::remove_file(&self.partial)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            #[cfg(unix)]
            {
                let parent = tokio::fs::File::open(self.local.parent().ok_or("目标目录无效")?)
                    .await
                    .map_err(|e| e.to_string())?;
                parent.sync_all().await.map_err(|e| e.to_string())?;
            }
        }
        self.forget()?;
        Ok(())
    }

    async fn recover_publication(
        &self,
        connection: &Connection,
        progress: &mut impl FnMut(Progress),
    ) -> Result<bool, String> {
        let upload = self.direction == Direction::Upload;
        let remote_path = &self.remote;
        let local_path = &self.local;
        let destination_exists = if upload {
            match connection.sftp.fs().symlink_metadata(&self.remote).await {
                Ok(metadata) => metadata.file_type().is_some_and(|t| t.is_file()),
                Err(error) if is_missing(&error) => false,
                Err(error) => return Err(error.to_string()),
            }
        } else {
            match tokio::fs::symlink_metadata(&self.local).await {
                Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.to_string()),
            }
        };
        if !destination_exists {
            return Ok(false);
        }
        let mut local = tokio::fs::File::open(local_path)
            .await
            .map_err(|e| e.to_string())?;
        let mut remote = connection
            .sftp
            .open(remote_path)
            .await
            .map_err(|e| e.to_string())?;
        let local_meta = local.metadata().await.map_err(|e| e.to_string())?;
        let remote_meta = remote.metadata().await.map_err(|e| e.to_string())?;
        let revision = if upload {
            local_revision(&local_meta)?
        } else {
            remote_revision(remote_meta)?
        };
        if self.source.as_ref() != Some(&revision) {
            return Err("源文件已改变，无法核对已发布文件".into());
        }
        if local_meta.len() != self.total || remote_meta.len() != Some(self.total) {
            return Ok(false);
        }
        let mut checked = 0;
        while checked < self.total {
            for bytes in remote_batch(&remote, checked, self.total).await? {
                let mut buffer = vec![0; bytes.len()];
                local
                    .read_exact(&mut buffer)
                    .await
                    .map_err(|e| e.to_string())?;
                if bytes.as_ref() != buffer {
                    return Ok(false);
                }
                checked += bytes.len() as u64;
            }
            progress(Progress {
                bytes: checked,
                total: self.total,
                verifying: true,
            });
        }
        let after = if upload {
            local_revision(&local.metadata().await.map_err(|e| e.to_string())?)?
        } else {
            remote_revision(remote.metadata().await.map_err(|e| e.to_string())?)?
        };
        if after != revision {
            return Err("核对期间源文件改变".into());
        }
        remote.close().await.map_err(|e| e.to_string())?;
        if upload {
            if let Err(error) = connection.sftp.fs().remove_file(&self.partial).await {
                if !is_missing(&error) {
                    return Err(error.to_string());
                }
            }
        } else if let Err(error) = tokio::fs::remove_file(&self.partial).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.to_string());
            }
        }
        self.forget()?;
        Ok(true)
    }
}

async fn remote_write_batch(file: &RemoteFile, offset: u64, bytes: &[u8]) -> Result<(), String> {
    // 每批等待所有写入确认并释放克隆句柄，确保发布前的显式关闭真正完成。
    let mut pending = FuturesOrdered::new();
    for (index, chunk) in bytes.chunks(BLOCK).enumerate() {
        let mut writer = file.clone();
        pending.push_back(async move {
            writer
                .seek(SeekFrom::Start(offset + (index * BLOCK) as u64))
                .await?;
            writer.write_all(chunk).await
        });
    }
    while let Some(result) = pending.next().await {
        result.map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn remote_batch(file: &RemoteFile, offset: u64, total: u64) -> Result<Vec<BytesMut>, String> {
    let block = BLOCK;
    let mut pending = FuturesOrdered::new();
    let mut position = offset;
    for _ in 0..PIPELINE {
        if position >= total {
            break;
        }
        let length = (total - position).min(block as u64) as usize;
        let mut reader = file.clone();
        let start = position;
        pending.push_back(async move {
            reader.seek(SeekFrom::Start(start)).await?;
            reader.read_all(length, BytesMut::new()).await
        });
        position += length as u64;
    }
    let mut result = Vec::with_capacity(pending.len());
    while let Some(bytes) = pending.next().await {
        let bytes = bytes.map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Err("源文件提前结束，已保留断点".into());
        }
        result.push(bytes);
    }
    Ok(result)
}
