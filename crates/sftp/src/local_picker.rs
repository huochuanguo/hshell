use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Serialize)]
pub struct LocalEntry {
    pub name: String,
    pub path: PathBuf,
    pub directory: bool,
}

#[derive(Serialize)]
pub struct LocalDirectory {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<LocalEntry>,
}

pub fn list_local_upload(path: Option<PathBuf>) -> Result<LocalDirectory, String> {
    let path = match path {
        Some(path) => path,
        None => std::env::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .ok_or("找不到本地主目录")?,
    };
    if !path.is_absolute() {
        return Err("请输入本地目录的绝对路径".into());
    }
    let items = fs::read_dir(&path).map_err(|error| format!("无法读取本地目录：{error}"))?;
    let mut entries: Vec<_> = items
        .filter_map(|item| {
            let item = item.ok()?;
            let kind = item.file_type().ok()?;
            if !kind.is_dir() && !kind.is_file() {
                return None;
            }
            let path = item.path();
            #[cfg(windows)]
            path.to_str()?;
            Some(LocalEntry {
                name: item.file_name().to_string_lossy().into_owned(),
                path,
                directory: kind.is_dir(),
            })
        })
        .collect();
    entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
    Ok(LocalDirectory {
        parent: path.parent().map(PathBuf::from),
        path,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同一目录返回可混选的文件夹和文件() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("空目录");
        let file = temp.path().join("中文 文件.txt");
        fs::create_dir(&folder).unwrap();
        fs::write(&file, "fixture").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&folder, temp.path().join("目录链接")).unwrap();

        let listing = list_local_upload(Some(temp.path().into())).unwrap();
        assert_eq!(listing.path, temp.path());
        assert_eq!(listing.parent.as_deref(), temp.path().parent());
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries[0].directory);
        assert_eq!(listing.entries[0].path, folder);
        assert!(!listing.entries[1].directory);
        assert_eq!(listing.entries[1].path, file);
    }
}
