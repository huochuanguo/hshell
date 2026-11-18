#[cfg(feature = "windows-fixture")]
mod fixture {
    use hshell_sftp::{Connection, Direction, Transfer};
    use sha2::{Digest, Sha256};
    use ssh_core::Profile;
    use std::{
        io::Write,
        path::{Path, PathBuf},
    };
    use tokio_util::sync::CancellationToken;

    fn profile() -> Profile {
        Profile {
            id: "windows-sftp-fixture".into(),
            name: "Windows 隔离 SFTP".into(),
            host: "fixture".into(),
            username: String::new(),
            port: None,
            identity_file: String::new(),
            group: String::new(),
            favorite: false,
            password_saved: false,
        }
    }
    fn data(path: &Path) {
        let mut file = std::fs::File::create(path).unwrap();
        let block: Vec<u8> = (0..65536)
            .map(|i| ((i * 73 + i / 251) % 256) as u8)
            .collect();
        for _ in 0..512 {
            file.write_all(&block).unwrap();
        }
        file.sync_all().unwrap();
    }
    fn digest(path: &Path) -> Vec<u8> {
        use std::io::Read;
        let mut file = std::fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut block = [0; 65536];
        loop {
            let length = file.read(&mut block).unwrap();
            if length == 0 {
                break;
            }
            hash.update(&block[..length]);
        }
        hash.finalize().to_vec()
    }
    fn pause_after_checkpoint(
        state: &Path,
        progress: &hshell_sftp::Progress,
        signal: &CancellationToken,
    ) {
        if progress.verifying {
            return;
        }
        // 让回环传输跨过检查点的时间门槛，只在记录已经落盘后模拟退出。
        std::thread::sleep(std::time::Duration::from_millis(60));
        if progress.bytes < progress.total && Transfer::restore(state).unwrap()[0].done > 0 {
            signal.cancel();
        }
    }
    pub async fn run() {
        let args: Vec<_> = std::env::args_os().collect();
        let config = PathBuf::from(&args[1]);
        let root = PathBuf::from(&args[2]);
        let mode = args[3].to_str().unwrap();
        let (command, authentication) = ssh_core::sftp_command_with_config(
            &profile(),
            &config,
            Some(zeroize::Zeroizing::new("fixture-only".into())),
            |_| false,
        )
        .unwrap();
        let connection = Connection::open(command, authentication).await.unwrap();
        let source = root.join("中文 上传文件.bin");
        let destination = root.join("中文 下载文件.bin");
        let state = root.join("journals");
        if mode == "pause-upload" {
            data(&source);
            let mut transfer = Transfer::new(
                Direction::Upload,
                profile(),
                source,
                "/data/中文 远程文件.bin".into(),
                &state,
            )
            .unwrap();
            let cancel = CancellationToken::new();
            let signal = cancel.clone();
            let result = tokio::select! {
                result = transfer.run(&connection, |progress| pause_after_checkpoint(&state, &progress, &signal)) => Some(result),
                _ = cancel.cancelled() => None,
            };
            assert!(result.is_none(), "传输必须在完成前暂停");
            assert!(Transfer::restore(&state).unwrap()[0].done > 0);
        } else if mode == "resume-upload-pause-download" {
            let mut transfer = Transfer::restore(&state).unwrap().remove(0);
            let mut verified = false;
            transfer
                .run(&connection, |progress| {
                    verified |= progress.verifying;
                })
                .await
                .unwrap();
            assert!(verified, "续传必须校验前缀");
            let mut download = Transfer::new(
                Direction::Download,
                profile(),
                destination,
                "/data/中文 远程文件.bin".into(),
                &state,
            )
            .unwrap();
            let cancel = CancellationToken::new();
            let signal = cancel.clone();
            let result = tokio::select! {
                result = download.run(&connection, |progress| pause_after_checkpoint(&state, &progress, &signal)) => Some(result),
                _ = cancel.cancelled() => None,
            };
            assert!(result.is_none());
        } else {
            let mut download = Transfer::restore(&state).unwrap().remove(0);
            let mut verified = false;
            download
                .run(&connection, |progress| {
                    verified |= progress.verifying;
                })
                .await
                .unwrap();
            assert!(verified);
            assert_eq!(digest(&source), digest(&destination));
            assert!(Transfer::restore(&state).unwrap().is_empty());
            let mut names = Vec::new();
            let path = connection
                .list(Path::new("/data"), |batch| {
                    names.extend(batch.into_iter().map(|e| e.name));
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(path.to_str(), Some("/data"));
            assert!(names
                .iter()
                .any(|name| name.to_str() == Some("中文 远程文件.bin")));
            connection.mkdir(Path::new("/data/目录")).await.unwrap();
            connection
                .rename(Path::new("/data/目录"), Path::new("/data/新目录"))
                .await
                .unwrap();
            assert!(connection
                .rename(
                    Path::new("/data/中文 远程文件.bin"),
                    Path::new("/data/新目录")
                )
                .await
                .is_err());
            connection
                .remove(Path::new("/data/新目录"), true)
                .await
                .unwrap();
            connection
                .remove(Path::new("/data/中文 远程文件.bin"), false)
                .await
                .unwrap();
        }
        connection.close().await;
        println!("{mode}: passed");
    }
}

fn main() {
    if let Some(code) = ssh_core::run_askpass_helper() {
        std::process::exit(code);
    }
    #[cfg(feature = "windows-fixture")]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fixture::run());
    #[cfg(not(feature = "windows-fixture"))]
    panic!("请启用 windows-fixture 测试功能");
}
