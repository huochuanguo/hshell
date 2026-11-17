//! Isolated local preparation benchmark; does not connect or access credentials.
use ssh_core::{load_profiles, save_profiles, Profile, ProfileCache};
use std::{hint::black_box, time::Instant};

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.json");
    let profiles: Vec<_> = (0..1000)
        .map(|i| Profile {
            id: format!("fixture-{i}"),
            name: format!("测试主机 {i}"),
            host: format!("host-{i}"),
            username: "fixture".into(),
            port: None,
            identity_file: String::new(),
            group: format!("Group {}", i % 10),
            favorite: false,
            password_saved: false,
        })
        .collect();
    save_profiles(&path, &profiles).unwrap();
    let count = 1000;
    let started = Instant::now();
    for _ in 0..count {
        black_box(load_profiles(&path).unwrap());
    }
    let uncached = started.elapsed().as_secs_f64() * 1000.0;
    let mut cache = ProfileCache::default();
    let started = Instant::now();
    for _ in 0..count {
        black_box(cache.load(&path).unwrap());
    }
    let cached = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{}",
        serde_json::json!({"profiles":profiles.len(),"preparations":count,"uncachedTotalMs":uncached,"cachedTotalMs":cached,
        "notes":"Same release binary, temporary local config, includes cache cold load and metadata checks. Does not measure SSH handshake or system credential reads."})
    );
}
