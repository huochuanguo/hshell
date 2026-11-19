//! Linux 使用隔离 Secret Service，Windows 使用随机条目验证凭据管理器。
use ssh_core::{
    password_for, save_profiles, store_connections, CredentialStore, PasswordUpdate, Profile,
    SystemCredentials,
};
struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        // 即使断言失败，也只清理本次测试创建的临时条目。
        let _ = SystemCredentials.delete(&self.0);
    }
}
fn main() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles.json");
    let id = uuid::Uuid::new_v4().to_string();
    let _cleanup = Cleanup(id.clone());
    let profile: Profile = serde_json::from_value(serde_json::json!({"id":id,"name":"fixture","host":"fixture.invalid","username":"fixture","port":22,"identityFile":"","group":"","favorite":false,"passwordSaved":true})).unwrap();
    let saved = vec![profile];
    save_profiles(&path, &saved).unwrap();
    let legacy = keyring::Entry::new("io.qizhan.ssh.password", &id).unwrap();
    let current = keyring::Entry::new("io.hshell.ssh.password", &id).unwrap();
    let fixture = r#"{"host":"fixture.invalid","username":"fixture","port":22,"password":"disposable-fixture-only"}"#;
    legacy.set_password(fixture).unwrap();
    assert_eq!(
        &**password_for(&SystemCredentials, &saved[0]).unwrap(),
        "disposable-fixture-only"
    );
    assert_eq!(current.get_password().unwrap(), fixture);
    assert!(matches!(
        legacy.get_password(),
        Err(keyring::Error::NoEntry)
    ));
    // 模拟旧版又保存了同一连接，新条目优先，替换后旧条目也应清理。
    legacy.set_password(fixture).unwrap();
    let saved = store_connections(
        &path,
        saved,
        Some(PasswordUpdate::Set {
            id: id.clone(),
            password: "replacement-fixture-only".into(),
        }),
        &SystemCredentials,
    )
    .unwrap();
    assert_eq!(
        &**password_for(&SystemCredentials, &saved[0]).unwrap(),
        "replacement-fixture-only"
    );
    assert!(matches!(
        legacy.get_password(),
        Err(keyring::Error::NoEntry)
    ));
    legacy.set_password(fixture).unwrap();
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("fixture-only"));
    let saved = store_connections(
        &path,
        saved,
        Some(PasswordUpdate::Remove { id }),
        &SystemCredentials,
    )
    .unwrap();
    assert!(!saved[0].password_saved);
    assert!(matches!(
        current.get_password(),
        Err(keyring::Error::NoEntry)
    ));
    assert!(matches!(
        legacy.get_password(),
        Err(keyring::Error::NoEntry)
    ));
    assert!(password_for(&SystemCredentials, &saved[0]).is_err());
    store_connections(&path, vec![], None, &SystemCredentials).unwrap();
    println!("System keyring migration/read/replace/remove and plaintext exclusion: passed");
}
