use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub username: String,
    pub port: Option<u16>,
    pub identity_file: String,
    pub group: String,
    pub favorite: bool,
    #[serde(default)]
    pub password_saved: bool,
}

impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.id.len() > 80 {
            return Err("连接标识无效".into());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err("连接名称须为 1–80 个字符".into());
        }
        // Arguments are passed directly, never interpolated into a shell.
        // Limit destinations to hosts, IPs and SSH aliases, not user@host or options.
        if self.host.is_empty()
            || self.host.len() > 253
            || self.host.starts_with('-')
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_:".contains(&b))
        {
            return Err("主机须为域名、IP 地址或 SSH 配置别名（不含用户名）".into());
        }
        if self.username.len() > 128
            || self.username.starts_with('-')
            || self
                .username
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("SSH 用户名无效".into());
        }
        if self.port == Some(0) {
            return Err("端口范围为 1–65535，留空沿用 SSH 配置".into());
        }
        if self.identity_file.len() > 4096 || self.identity_file.chars().any(char::is_control) {
            return Err("私钥路径无效".into());
        }
        if self.group.chars().count() > 40 || self.group.chars().any(char::is_control) {
            return Err("分组名称无效".into());
        }
        Ok(())
    }
}

pub fn load_profiles(path: &Path) -> Result<Vec<Profile>, String> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("读取连接配置失败：{e}")),
    };
    if data.len() > 4 * 1024 * 1024 {
        return Err("连接配置超过 4 MiB 限制".into());
    }
    let profiles: Vec<Profile> =
        serde_json::from_slice(&data).map_err(|e| format!("连接配置损坏，未覆盖原文件：{e}"))?;
    validate_all(&profiles)?;
    Ok(profiles)
}

fn validate_all(profiles: &[Profile]) -> Result<(), String> {
    if profiles.len() > 1000 {
        return Err("最多保存 1000 个连接".into());
    }
    let mut ids = std::collections::HashSet::new();
    for profile in profiles {
        profile.validate()?;
        if !ids.insert(&profile.id) {
            return Err("连接标识重复".into());
        }
    }
    Ok(())
}

pub fn save_profiles(path: &Path, profiles: &[Profile]) -> Result<(), String> {
    validate_all(profiles)?;
    let parent = path.parent().ok_or("配置目录无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?;
    tmp.write_all(&bytes).map_err(|e| e.to_string())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn profile() -> Profile {
        Profile {
            id: "test".into(),
            name: "开发环境".into(),
            host: "dev-server".into(),
            username: String::new(),
            port: None,
            identity_file: String::new(),
            group: "工作".into(),
            favorite: false,
            password_saved: false,
        }
    }

    #[test]
    fn rejects_option_and_shell_destinations() {
        for host in [
            "-oProxyCommand=touch /tmp/owned",
            "a;ls",
            "a\nb",
            "user@host",
            "",
        ] {
            let mut p = profile();
            p.host = host.into();
            assert!(p.validate().is_err(), "{host}");
        }
        for host in [
            "::1",
            "2001:db8::1",
            "my-alias",
            "127.0.0.1",
            "host.internal",
        ] {
            let mut p = profile();
            p.host = host.into();
            assert!(p.validate().is_ok(), "{host}");
        }
    }

    #[test]
    fn persists_replaces_and_preserves_corrupt_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        save_profiles(&path, &[profile()]).unwrap();
        assert_eq!(load_profiles(&path).unwrap().len(), 1);
        save_profiles(&path, &[]).unwrap();
        assert!(load_profiles(&path).unwrap().is_empty());
        fs::write(&path, "broken").unwrap();
        assert!(load_profiles(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
    }

    #[test]
    fn rejects_duplicate_ids_and_zero_ports() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_profiles(&dir.path().join("p.json"), &[profile(), profile()]).is_err());
        let mut p = profile();
        p.port = Some(0);
        assert!(p.validate().is_err());
    }

    #[test]
    #[cfg(unix)]
    fn config_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        save_profiles(&path, &[profile()]).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
