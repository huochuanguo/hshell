//! Validated connection snapshots. Passwords are never cached here.
use crate::{load_profiles, Profile};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
struct Revision {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn revision(path: &Path) -> Result<Option<Revision>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取连接配置失败：{error}")),
    };
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Some(Revision {
        size: metadata.len(),
        modified: metadata.modified().map_err(|error| error.to_string())?,
        #[cfg(unix)]
        identity: (
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ),
    }))
}

struct Snapshot {
    path: PathBuf,
    revision: Option<Revision>,
    profiles: Arc<[Profile]>,
}

#[derive(Default)]
pub struct ProfileCache {
    snapshot: Option<Snapshot>,
    #[cfg(test)]
    loads: usize,
}

impl ProfileCache {
    pub fn load(&mut self, path: &Path) -> Result<Arc<[Profile]>, String> {
        for _ in 0..3 {
            let before = revision(path)?;
            if let Some(snapshot) = &self.snapshot {
                // Native Linux has inode + ctime identity. Other platforms keep
                // fresh reads until an equivalent file-identity check is added.
                if cfg!(unix) && snapshot.path == path && snapshot.revision == before {
                    return Ok(snapshot.profiles.clone());
                }
            }
            self.snapshot = None;
            #[cfg(test)]
            {
                self.loads += 1;
            }
            let profiles: Arc<[Profile]> = load_profiles(path)?.into();
            if before == revision(path)? {
                self.snapshot = Some(Snapshot {
                    path: path.into(),
                    revision: before,
                    profiles: profiles.clone(),
                });
                return Ok(profiles);
            }
        }
        Err("连接配置正在变化，请稍后重试".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_profiles;
    use std::{fs, fs::FileTimes};

    fn profile(host: &str) -> Profile {
        Profile {
            id: "saved".into(),
            name: "服务器".into(),
            host: host.into(),
            username: String::new(),
            port: None,
            identity_file: String::new(),
            group: String::new(),
            favorite: false,
            password_saved: false,
        }
    }

    #[test]
    fn repeated_connections_reuse_validated_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        save_profiles(&path, &[profile("first")]).unwrap();
        let mut cache = ProfileCache::default();
        for _ in 0..25 {
            assert_eq!(cache.load(&path).unwrap()[0].host, "first");
        }
        assert_eq!(
            cache.loads,
            if cfg!(unix) { 1 } else { 25 },
            "Unchanged connection config was read/parsed repeatedly"
        );
    }

    #[test]
    fn observes_external_replacement_corruption_deletion_and_recreation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        save_profiles(&path, &[profile("first")]).unwrap();
        let mut cache = ProfileCache::default();
        assert_eq!(cache.load(&path).unwrap()[0].host, "first");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        // Atomic replacement with identical size and mtime must still invalidate.
        save_profiles(&path, &[profile("other")]).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(cache.load(&path).unwrap()[0].host, "other");
        fs::write(&path, b"broken").unwrap();
        assert!(
            cache.load(&path).is_err(),
            "Corruption must not reuse a stale target"
        );
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::remove_file(&path).unwrap();
        assert!(cache.load(&path).unwrap().is_empty());
        save_profiles(&path, &[profile("again")]).unwrap();
        assert_eq!(cache.load(&path).unwrap()[0].host, "again");
        let other = dir.path().join("other.json");
        save_profiles(&other, &[profile("third")]).unwrap();
        assert_eq!(cache.load(&other).unwrap()[0].host, "third");
    }

    #[test]
    #[cfg(unix)]
    fn observes_in_place_edit_even_when_mtime_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        save_profiles(&path, &[profile("first")]).unwrap();
        let mut cache = ProfileCache::default();
        assert_eq!(cache.load(&path).unwrap()[0].host, "first");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let changed = fs::read_to_string(&path).unwrap().replace("first", "other");
        fs::write(&path, changed).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(cache.load(&path).unwrap()[0].host, "other");
    }
}
