use crate::{load_profiles, save_profiles, snippets::read_bounded, Profile};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    version: u32,
    connections: Vec<Connection>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    name: String,
    host: String,
    username: String,
    port: Option<u16>,
    group: String,
    favorite: bool,
}

impl From<Profile> for Connection {
    fn from(value: Profile) -> Self {
        Self {
            name: value.name,
            host: value.host,
            username: value.username,
            port: value.port,
            group: value.group,
            favorite: value.favorite,
        }
    }
}

impl From<Connection> for Profile {
    fn from(value: Connection) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: value.name,
            host: value.host,
            username: value.username,
            port: value.port,
            group: value.group,
            favorite: value.favorite,
            identity_file: String::new(),
            password_saved: false,
        }
    }
}

#[derive(Serialize)]
pub struct ImportResult {
    pub profiles: Vec<Profile>,
    pub added: usize,
    pub skipped: usize,
}

pub fn export_connections(config: &Path, destination: &Path) -> Result<usize, String> {
    let connections: Vec<_> = load_profiles(config)?
        .into_iter()
        .map(Connection::from)
        .collect();
    let count = connections.len();
    let parent = destination.parent().ok_or("导出目录无效")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(
        &mut file,
        &Backup {
            version: 1,
            connections,
        },
    )
    .map_err(|e| e.to_string())?;
    file.flush()
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    // 导出使用新文件名，避免把连接配置或其他本机文件替换成备份格式。
    file.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            "导出文件已存在，请选择新的文件名。".into()
        } else {
            error.to_string()
        }
    })?;
    Ok(count)
}

fn key(profile: &Profile) -> (String, String, String, Option<u16>) {
    (
        profile.name.clone(),
        profile.host.to_ascii_lowercase(),
        profile.username.clone(),
        profile.port,
    )
}

pub fn import_connections(config: &Path, source: &Path) -> Result<ImportResult, String> {
    let file = fs::File::open(source).map_err(|e| format!("读取连接备份失败：{e}"))?;
    let backup: Backup = serde_json::from_slice(&read_bounded(file)?)
        .map_err(|e| format!("连接备份格式无效：{e}"))?;
    if backup.version != 1 {
        return Err("不支持此连接备份版本".into());
    }
    if backup.connections.len() > 1000 {
        return Err("一个备份最多包含 1000 个连接".into());
    }
    let mut profiles = load_profiles(config)?;
    let previous = profiles.len();
    let mut existing: HashSet<_> = profiles.iter().map(key).collect();
    let total = backup.connections.len();
    for item in backup.connections {
        let profile = Profile::from(item);
        profile.validate()?;
        if existing.insert(key(&profile)) {
            profiles.push(profile);
        }
    }
    let added = profiles.len() - previous;
    if added > 0 {
        save_profiles(config, &profiles)?;
    }
    Ok(ImportResult {
        profiles,
        added,
        skipped: total - added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_backup_never_overwrites_or_copies_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("source.json");
        let destination = dir.path().join("target.json");
        let backup = dir.path().join("connections.json");
        let mut profile = Profile::from(Connection {
            name: "开发环境".into(),
            host: "EXAMPLE.com".into(),
            username: "deploy".into(),
            port: Some(2222),
            group: "服务器".into(),
            favorite: true,
        });
        profile.password_saved = true;
        profile.identity_file = "/home/test/.ssh/private-key".into();
        save_profiles(&config, std::slice::from_ref(&profile)).unwrap();
        assert_eq!(export_connections(&config, &backup).unwrap(), 1);
        let json = fs::read_to_string(&backup).unwrap();
        for private in ["passwordSaved", "identityFile", "private-key", &profile.id] {
            assert!(!json.contains(private));
        }
        assert!(export_connections(&config, &backup).is_err());
        assert!(export_connections(&config, &config).is_err());
        let first = import_connections(&destination, &backup).unwrap();
        assert_eq!((first.added, first.skipped), (1, 0));
        let copied = &first.profiles[0];
        assert_ne!(copied.id, profile.id);
        assert_eq!(
            (&copied.group, copied.port, copied.favorite),
            (&profile.group, profile.port, true)
        );
        assert!(!copied.password_saved && copied.identity_file.is_empty());
        let again = import_connections(&config, &backup).unwrap();
        assert_eq!((again.added, again.skipped), (0, 1));
        assert!(again.profiles[0].password_saved);
        assert_eq!(again.profiles[0].identity_file, profile.identity_file);
    }

    #[test]
    fn invalid_backup_cannot_partially_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        let config = dir.path().join("profiles.json");
        save_profiles(&config, &[]).unwrap();
        let old = fs::read(&config).unwrap();
        let valid = r#"{"name":"示例","host":"fixture","username":"","port":null,"group":"","favorite":false}"#;
        let invalid = valid.replace("fixture", "-invalid");
        for json in [
            format!(r#"{{"version":1,"connections":[{valid},{invalid}]}}"#),
            format!(r#"{{"version":2,"connections":[{valid}]}}"#),
            format!(
                r#"{{"version":1,"connections":[{}]}}"#,
                valid.replace(
                    "\"favorite\":false",
                    "\"passwordSaved\":true,\"favorite\":false"
                )
            ),
        ] {
            fs::write(&path, json).unwrap();
            assert!(import_connections(&config, &path).is_err());
            assert_eq!(fs::read(&config).unwrap(), old);
        }
    }
}
