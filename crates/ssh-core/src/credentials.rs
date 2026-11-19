//! Passwords live in the OS credential store, never in profiles.json.
use crate::{load_profiles, save_profiles, Profile};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};
use zeroize::Zeroizing;

const SERVICE: &str = "io.hshell.ssh.password";
const LEGACY_SERVICE: &str = "io.qizhan.ssh.password";
// 防止后台会话读取旧条目时，与同一客户端的密码替换或删除交错。
static CREDENTIAL_LOCK: Mutex<()> = Mutex::new(());

pub trait CredentialStore {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String>;
    fn set(&self, id: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

pub struct SystemCredentials;

struct KeyringStore(&'static str);
fn entry(service: &str, id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, id).map_err(|_| "无法访问系统凭据库".into())
}
#[cfg(not(windows))]
const STORE_ERROR: &str = "系统凭据库不可用或未解锁。请启动并解锁“密码和密钥”（GNOME Keyring / Secret Service），或取消保存密码后重试。";
#[cfg(windows)]
const STORE_ERROR: &str = "Windows 凭据管理器不可用，请检查系统凭据服务，或取消保存密码后重试。";
impl CredentialStore for KeyringStore {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
        match entry(self.0, id)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(STORE_ERROR.into()),
        }
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        entry(self.0, id)?
            .set_password(secret)
            .map_err(|_| STORE_ERROR.into())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        match entry(self.0, id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(STORE_ERROR.into()),
        }
    }
}

impl CredentialStore for SystemCredentials {
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
        let _guard = CREDENTIAL_LOCK.lock().map_err(|_| STORE_ERROR)?;
        let current = KeyringStore(SERVICE);
        if let Some(secret) = current.get(id)? {
            return Ok(Some(secret));
        }
        let legacy = KeyringStore(LEGACY_SERVICE);
        let saved = legacy.get(id)?;
        if let Some(secret) = &saved {
            // 先确认新条目保存成功，再清理旧条目；无法写入时仍可使用旧密码。
            if current.set(id, secret).is_ok() {
                let _ = legacy.delete(id);
            }
        }
        Ok(saved)
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        let _guard = CREDENTIAL_LOCK.lock().map_err(|_| STORE_ERROR)?;
        // 新条目写好后再清理旧凭据，写入失败时旧密码仍在。
        KeyringStore(SERVICE).set(id, secret)?;
        KeyringStore(LEGACY_SERVICE).delete(id)
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let _guard = CREDENTIAL_LOCK.lock().map_err(|_| STORE_ERROR)?;
        KeyringStore(LEGACY_SERVICE).delete(id)?;
        KeyringStore(SERVICE).delete(id)
    }
}

#[derive(Serialize, Deserialize)]
struct SavedPassword {
    host: String,
    username: String,
    port: Option<u16>,
    password: String,
}
impl Drop for SavedPassword {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
    }
}
fn same_target(a: &Profile, b: &Profile) -> bool {
    a.host == b.host && a.username == b.username && a.port == b.port
}

pub fn password_for(
    store: &impl CredentialStore,
    profile: &Profile,
) -> Result<Zeroizing<String>, String> {
    let value = store
        .get(&profile.id)?
        .ok_or("已保存的密码不存在，请编辑连接重新保存，或取消保存密码")?;
    let saved: SavedPassword =
        serde_json::from_str(&value).map_err(|_| "系统凭据格式无效，请重新保存密码")?;
    if saved.host != profile.host
        || saved.username != profile.username
        || saved.port != profile.port
    {
        return Err("连接目标已改变，请重新保存密码".into());
    }
    Ok(Zeroizing::new(saved.password.clone()))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum PasswordUpdate {
    Set { id: String, password: String },
    Remove { id: String },
}

// File writes and keyring operations form a transaction. Restore the old keyring
// entries if an operation fails; surface rollback failure rather than claiming success.
pub fn store_connections(
    path: &Path,
    mut next: Vec<Profile>,
    update: Option<PasswordUpdate>,
    store: &impl CredentialStore,
) -> Result<Vec<Profile>, String> {
    let previous = load_profiles(path)?;
    let mut operations: Vec<(String, Option<Zeroizing<String>>)> = Vec::new();
    for old in &previous {
        if old.password_saved && !next.iter().any(|p| p.id == old.id && same_target(old, p)) {
            operations.push((old.id.clone(), None));
        }
    }
    for p in &mut next {
        p.validate()?;
        p.password_saved = previous
            .iter()
            .any(|old| old.id == p.id && old.password_saved && same_target(old, p));
    }
    if next.len() > 1000
        || next
            .iter()
            .map(|p| &p.id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            != next.len()
    {
        return Err("连接数量超限或标识重复".into());
    }
    if let Some(update) = update {
        let (id, secret) = match update {
            PasswordUpdate::Set { id, password } => {
                let password = Zeroizing::new(password);
                if password.is_empty()
                    || password.len() > 4096
                    || password.contains(['\0', '\r', '\n'])
                {
                    return Err("密码须为 1–4096 字节，不能包含换行或空字符".into());
                }
                let p = next.iter_mut().find(|p| p.id == id).ok_or("连接不存在")?;
                p.password_saved = true;
                let saved = SavedPassword {
                    host: p.host.clone(),
                    username: p.username.clone(),
                    port: p.port,
                    password: password.to_string(),
                };
                (
                    id,
                    Some(Zeroizing::new(
                        serde_json::to_string(&saved).map_err(|_| "凭据编码失败")?,
                    )),
                )
            }
            PasswordUpdate::Remove { id } => {
                next.iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("连接不存在")?
                    .password_saved = false;
                (id, None)
            }
        };
        operations.retain(|(other, _)| other != &id);
        operations.push((id, secret));
    }
    let mut backups = Vec::new();
    // Read all backups before making any mutation.
    for (id, _) in &operations {
        backups.push((id.clone(), store.get(id)?));
    }
    let result = (|| {
        for (id, secret) in &operations {
            match secret {
                Some(value) => store.set(id, value)?,
                None => store.delete(id)?,
            }
        }
        save_profiles(path, &next)
    })();
    if let Err(error) = result {
        let mut failed = false;
        for (id, secret) in backups {
            failed |= match secret {
                Some(value) => store.set(&id, &value),
                None => store.delete(&id),
            }
            .is_err();
        }
        return Err(if failed {
            format!("{error}；凭据回滚失败，请在系统凭据库中检查此连接")
        } else {
            error
        });
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};
    #[derive(Default)]
    struct Memory(RefCell<HashMap<String, String>>);
    impl CredentialStore for Memory {
        fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
            Ok(self.0.borrow().get(id).cloned().map(Zeroizing::new))
        }
        fn set(&self, id: &str, secret: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(id.into(), secret.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.0.borrow_mut().remove(id);
            Ok(())
        }
    }
    fn profile() -> Profile {
        serde_json::from_str(r#"{"id":"test","name":"test","host":"server","username":"user","port":22,"identityFile":"","group":"","favorite":false}"#).unwrap()
    }
    #[test]
    fn password_lifecycle_target_binding_and_no_plaintext_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let store = Memory::default();
        let saved = store_connections(
            &path,
            vec![profile()],
            Some(PasswordUpdate::Set {
                id: "test".into(),
                password: "fixture-secret".into(),
            }),
            &store,
        )
        .unwrap();
        assert!(saved[0].password_saved);
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("fixture-secret"));
        assert_eq!(
            &**password_for(&store, &saved[0]).unwrap(),
            "fixture-secret"
        );
        let mut renamed = saved.clone();
        renamed[0].name = "renamed".into();
        let saved = store_connections(&path, renamed, None, &store).unwrap();
        assert!(saved[0].password_saved);
        let mut changed = saved.clone();
        changed[0].host = "other-server".into();
        assert!(password_for(&store, &changed[0]).is_err());
        let changed = store_connections(&path, changed, None, &store).unwrap();
        assert!(!changed[0].password_saved);
        assert!(store.0.borrow().is_empty());
        store_connections(
            &path,
            changed,
            Some(PasswordUpdate::Set {
                id: "test".into(),
                password: "new-secret".into(),
            }),
            &store,
        )
        .unwrap();
        store_connections(&path, vec![], None, &store).unwrap();
        assert!(store.0.borrow().is_empty());
    }
    #[test]
    fn file_failure_rolls_back_password() {
        struct Sabotage {
            memory: Memory,
            path: std::path::PathBuf,
        }
        impl CredentialStore for Sabotage {
            fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
                self.memory.get(id)
            }
            fn set(&self, id: &str, secret: &str) -> Result<(), String> {
                self.memory.set(id, secret)?;
                std::fs::create_dir(&self.path).unwrap(); // Force atomic rename to fail after the keyring write.
                Ok(())
            }
            fn delete(&self, id: &str) -> Result<(), String> {
                self.memory.delete(id)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let store = Sabotage {
            memory: Memory::default(),
            path: path.clone(),
        };
        assert!(store_connections(
            &path,
            vec![profile()],
            Some(PasswordUpdate::Set {
                id: "test".into(),
                password: "secret".into()
            }),
            &store
        )
        .is_err());
        assert!(path.is_dir()); // Confirms the mutation seam was reached.
        assert!(store.memory.0.borrow().is_empty());
    }
    #[test]
    fn unavailable_keyring_never_claims_password_saved() {
        struct Unavailable;
        impl CredentialStore for Unavailable {
            fn get(&self, _: &str) -> Result<Option<Zeroizing<String>>, String> {
                Ok(None)
            }
            fn set(&self, _: &str, _: &str) -> Result<(), String> {
                Err("locked keyring".into())
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        assert!(store_connections(
            &path,
            vec![profile()],
            Some(PasswordUpdate::Set {
                id: "test".into(),
                password: "secret".into()
            }),
            &Unavailable
        )
        .is_err());
        assert!(!path.exists());
    }
}
