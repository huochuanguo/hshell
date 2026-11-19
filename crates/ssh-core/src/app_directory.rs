use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const APP_ID: &str = "io.hshell.ssh";
const LEGACY_APP_ID: &str = "io.qizhan.ssh";

fn directory_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(metadata) if metadata.file_type().is_symlink() && fs::metadata(path)?.is_dir() => {
            Ok(true)
        }
        Ok(_) => Err(io::Error::new(io::ErrorKind::NotADirectory, "不是目录")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn prepare_app_directory(root: &Path) -> Result<PathBuf, String> {
    let current = root.join(APP_ID);
    let legacy = root.join(LEGACY_APP_ID);
    let migrate = || -> io::Result<()> {
        // 同级目录整体迁移，连接、命令片段及续传记录要一起切换。
        // 若新版已经使用此目录，保留两边的数据，避免恢复已删除的记录。
        if !directory_exists(&current)? && directory_exists(&legacy)? {
            match fs::rename(&legacy, &current) {
                Ok(()) => (),
                // 另一个客户端可能刚完成了同一次迁移。
                Err(_) if directory_exists(&current)? && !directory_exists(&legacy)? => (),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    };
    migrate().map_err(|error| {
        format!(
            "无法准备 HShell 数据目录\n{} → {}\n{}\n请退出其他 HShell 窗口，检查目录权限后重试。",
            legacy.display(),
            current.display(),
            error
        )
    })?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_whole_workspace_and_can_reopen_it() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_APP_ID);
        fs::create_dir_all(legacy.join("transfers")).unwrap();
        for name in ["profiles.json", "commands.json", "transfers/task.json"] {
            fs::write(legacy.join(name), name).unwrap();
        }
        let current = prepare_app_directory(root.path()).unwrap();
        assert_eq!(current, root.path().join(APP_ID));
        assert!(!legacy.exists());
        for name in ["profiles.json", "commands.json", "transfers/task.json"] {
            assert_eq!(fs::read_to_string(current.join(name)).unwrap(), name);
        }
        assert_eq!(prepare_app_directory(root.path()).unwrap(), current);
    }

    #[test]
    fn leaves_both_workspaces_alone_if_current_already_exists() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_APP_ID);
        let current = root.path().join(APP_ID);
        fs::create_dir(&legacy).unwrap();
        fs::create_dir(&current).unwrap();
        fs::write(legacy.join("profiles.json"), "旧数据").unwrap();
        fs::write(current.join("profiles.json"), "新数据").unwrap();
        assert_eq!(prepare_app_directory(root.path()).unwrap(), current);
        assert_eq!(
            fs::read_to_string(legacy.join("profiles.json")).unwrap(),
            "旧数据"
        );
        assert_eq!(
            fs::read_to_string(current.join("profiles.json")).unwrap(),
            "新数据"
        );
    }

    #[test]
    fn reports_blocked_destination_without_touching_existing_data() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_APP_ID);
        let current = root.path().join(APP_ID);
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("profiles.json"), "旧数据").unwrap();
        fs::write(&current, "阻塞目录的文件").unwrap();
        let error = prepare_app_directory(root.path()).unwrap_err();
        assert!(error.contains(&current.display().to_string()));
        assert_eq!(
            fs::read_to_string(legacy.join("profiles.json")).unwrap(),
            "旧数据"
        );
        assert_eq!(fs::read_to_string(current).unwrap(), "阻塞目录的文件");
    }

    #[test]
    fn simultaneous_startups_find_the_same_workspace() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_APP_ID);
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("profiles.json"), "连接").unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let start = || {
                barrier.wait();
                prepare_app_directory(root.path()).unwrap()
            };
            let first = scope.spawn(start);
            let second = start();
            assert_eq!(first.join().unwrap(), second);
            assert_eq!(
                fs::read_to_string(second.join("profiles.json")).unwrap(),
                "连接"
            );
        });
    }
}
