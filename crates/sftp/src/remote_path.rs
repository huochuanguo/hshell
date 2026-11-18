use std::path::{Path, PathBuf};

// 远程路径始终按 Unix 规则处理，不能使用 Windows 的盘符或分隔符语义。
pub fn remote_absolute(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|text| text.starts_with('/') && !text.contains('\0'))
}

pub fn remote_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0'])
}

pub fn remote_join(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if !remote_absolute(parent) || !remote_name(name) {
        return Err("远程路径或文件名无效".into());
    }
    let parent = parent.to_str().ok_or("远程路径须为 UTF-8")?;
    Ok(PathBuf::from(format!(
        "{}/{name}",
        parent.trim_end_matches('/')
    )))
}

pub fn remote_parent(path: &Path) -> Result<PathBuf, String> {
    if !remote_absolute(path) {
        return Err("远程路径须为 Unix 绝对路径".into());
    }
    let text = path
        .to_str()
        .ok_or("远程路径须为 UTF-8")?
        .trim_end_matches('/');
    let (parent, _) = text.rsplit_once('/').ok_or("远程根目录没有父目录")?;
    Ok(PathBuf::from(if parent.is_empty() { "/" } else { parent }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_paths_keep_their_meaning_on_windows() {
        assert!(remote_absolute(Path::new("/home/用户/文件.txt")));
        assert!(!remote_absolute(Path::new("C:\\Users\\用户")));
        assert!(!remote_absolute(Path::new("relative/file")));
        assert_eq!(
            remote_join(Path::new("/"), "文件.txt").unwrap().to_str(),
            Some("/文件.txt")
        );
        assert_eq!(
            remote_join(Path::new("/home/用户"), "a:b\\c")
                .unwrap()
                .to_str(),
            Some("/home/用户/a:b\\c")
        );
        assert_eq!(
            remote_parent(Path::new("/home/用户/file"))
                .unwrap()
                .to_str(),
            Some("/home/用户")
        );
        assert!(!remote_name("../file"));
        assert!(!remote_name(".."));
    }

    #[test]
    fn upload_journals_preserve_remote_slashes_when_restored() {
        use crate::{Direction, Transfer};
        let root = tempfile::tempdir().unwrap();
        let profile = ssh_core::Profile {
            id: "windows-path-fixture".into(),
            name: "路径验证".into(),
            host: "fixture".into(),
            username: String::new(),
            port: None,
            identity_file: String::new(),
            group: String::new(),
            favorite: false,
            password_saved: false,
        };
        let state = root.path().join("state");
        let transfer = Transfer::new(
            Direction::Upload,
            profile,
            root.path().join("中文 文件.txt"),
            "/home/用户/中文 文件.txt".into(),
            &state,
        )
        .unwrap();
        assert_eq!(
            transfer.partial_path().to_str().unwrap(),
            format!("/home/用户/.hshell-{}.part", transfer.id)
        );
        let restored = Transfer::restore(&state).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(
            restored[0].remote.to_str(),
            Some("/home/用户/中文 文件.txt")
        );
        assert_eq!(restored[0].partial_path(), transfer.partial_path());
        assert_eq!(restored[0].local, transfer.local);
        let journal = state.join(format!("{}.json", transfer.id));
        let mut corrupted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
        corrupted["partial"] = transfer
            .partial_path()
            .to_str()
            .unwrap()
            .replace('/', "\\")
            .into();
        std::fs::write(&journal, serde_json::to_vec(&corrupted).unwrap()).unwrap();
        assert!(Transfer::restore(&state).is_err());
    }
}
