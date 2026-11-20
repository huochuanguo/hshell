//! Disposable real-server verification. Not part of the installed binary.
use hshell_sftp::{Connection, Direction, Transfer};
use sha2::{Digest, Sha256};
use ssh_core::Profile;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

fn profile() -> Profile {
    Profile {
        id: "sftp-fixture".into(),
        name: "SFTP 验证".into(),
        host: "fixture".into(),
        username: String::new(),
        port: None,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    }
}
fn digest(path: &Path) -> String {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 65536];
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    format!("{:x}", hash.finalize())
}
fn data(path: &Path, mib: usize) {
    let mut file = std::fs::File::create(path).unwrap();
    let block: Vec<u8> = (0..65536)
        .map(|i| ((i * 73 + i / 251) % 256) as u8)
        .collect();
    for _ in 0..mib * 16 {
        file.write_all(&block).unwrap();
    }
    file.sync_all().unwrap();
}
async fn connect() -> Connection {
    let (command, auth) = ssh_core::sftp_command(&profile(), None, |_| false).unwrap();
    Connection::open(command, auth).await.unwrap()
}
fn task(root: &Path, name: &str, direction: Direction) -> Transfer {
    Transfer::new(
        direction,
        profile(),
        root.join(format!("{name}.local")),
        PathBuf::from(format!("/fixture/data/{name}")),
        &root.join(format!("state-{name}")),
    )
    .unwrap()
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Some(code) = ssh_core::run_askpass_helper() {
        std::process::exit(code);
    }
    let mode = std::env::args().nth(1).unwrap();
    let root = PathBuf::from(std::env::var_os("HSHELL_SFTP_ROOT").unwrap());
    if mode == "authentication" {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        };
        let mut checks = Vec::new();
        for saved in [
            Some("fixture-only-test-123"),
            None,
            Some("wrong-fixture-password"),
        ] {
            let mut profile = profile();
            profile.host = "fixture-password".into();
            let slot: Arc<Mutex<Option<ssh_core::SubsystemAuthentication>>> = Arc::default();
            let response = slot.clone();
            let trust = Arc::new(AtomicUsize::new(0));
            let prompts = Arc::new(AtomicUsize::new(0));
            let confirmed = trust.clone();
            let requested = prompts.clone();
            let (command, auth) = ssh_core::sftp_command(
                &profile,
                saved.map(|s| zeroize::Zeroizing::new(s.to_string())),
                move |event| {
                    if let ssh_core::SessionEvent::Auth {
                        request_id,
                        confirm,
                        ..
                    } = event
                    {
                        if confirm {
                            confirmed.fetch_add(1, Ordering::Relaxed);
                        } else {
                            requested.fetch_add(1, Ordering::Relaxed);
                        }
                        response
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .respond(
                                &request_id,
                                Some(
                                    if confirm {
                                        "yes"
                                    } else {
                                        "fixture-only-test-123"
                                    }
                                    .into(),
                                ),
                            )
                            .is_ok()
                    } else {
                        false
                    }
                },
            )
            .unwrap();
            *slot.lock().unwrap() = Some(auth.clone());
            let connection = Connection::open(command, auth).await.unwrap();
            connection
                .list(Path::new("/fixture/data"), |_| Ok(()))
                .await
                .unwrap();
            connection.close().await;
            slot.lock().unwrap().take();
            let prompts = prompts.load(Ordering::Relaxed);
            assert_eq!(prompts, usize::from(saved != Some("fixture-only-test-123")));
            checks.push(serde_json::json!({"saved":saved.is_some(),"passwordPrompts":prompts,"fingerprintConfirmations":trust.load(Ordering::Relaxed)}));
        }
        assert_eq!(checks[0]["fingerprintConfirmations"], 1);
        println!("{}", serde_json::json!({"sftpAuthentication":checks}));
    } else if mode == "folder-upload" {
        #[cfg(unix)]
        {
            use hshell_sftp::UploadPlan;
            use std::{fs, os::unix::fs::symlink};
            let connection = connect().await;
            let directory = connection
                .list(Path::new("/fixture/data"), |_| Ok(()))
                .await
                .unwrap();
            let source = root.join("目录 样例");
            let target = root.join("data/目录 样例");
            let outside = root.join("data/外部目录");
            fs::create_dir_all(source.join("嵌套/空目录")).unwrap();
            fs::create_dir_all(source.join("冲突的目录")).unwrap();
            fs::create_dir_all(source.join("链接的目录")).unwrap();
            fs::create_dir_all(target.join("嵌套")).unwrap();
            fs::create_dir_all(&outside).unwrap();
            fs::write(source.join("嵌套/保留.txt"), "新内容").unwrap();
            fs::write(source.join("嵌套/新增.txt"), "新增内容").unwrap();
            fs::write(source.join("冲突的目录/不上传.txt"), "不写入").unwrap();
            fs::write(source.join("链接的目录/不上传.txt"), "不写入").unwrap();
            fs::write(target.join("嵌套/保留.txt"), "旧内容").unwrap();
            fs::write(target.join("冲突的目录"), "不能替换").unwrap();
            // 容器内只挂载了夹具目录，相对链接在宿主机与容器内均有效。
            symlink("../外部目录", target.join("链接的目录")).unwrap();
            symlink("../data/外部目录", source.join("本地链接")).unwrap();
            let state = root.join("state-folder");
            let inspect =
                || UploadPlan::inspect(&connection, directory.clone(), vec![source.clone()], None);
            let plan = inspect().await.unwrap();
            assert_eq!(plan.preview().files, 2);
            assert_eq!(plan.preview().conflicts, 1);
            assert_eq!(plan.preview().skipped, 5);
            assert!(!target.join("嵌套/空目录").exists());
            let prepared = plan.prepare(&connection, false, 1).await.unwrap();
            assert_eq!(prepared.folders, 1);
            assert_eq!(prepared.skipped, 6);
            for mut task in prepared.create_transfers(profile(), &state).unwrap() {
                task.run(&connection, |_| {}).await.unwrap();
            }
            assert_eq!(
                fs::read_to_string(target.join("嵌套/保留.txt")).unwrap(),
                "旧内容"
            );
            assert_eq!(
                fs::read_to_string(target.join("嵌套/新增.txt")).unwrap(),
                "新增内容"
            );
            assert!(target.join("嵌套/空目录").is_dir());
            let plan = inspect().await.unwrap();
            assert_eq!(plan.preview().conflicts, 2);
            for mut task in plan
                .prepare(&connection, true, 2)
                .await
                .unwrap()
                .create_transfers(profile(), &state)
                .unwrap()
            {
                task.run(&connection, |_| {}).await.unwrap();
            }
            assert_eq!(
                fs::read_to_string(target.join("嵌套/保留.txt")).unwrap(),
                "新内容"
            );
            let prepared = inspect()
                .await
                .unwrap()
                .prepare(&connection, true, 2)
                .await
                .unwrap();
            prepared.create_transfers(profile(), &state).unwrap();
            fs::rename(target.join("嵌套"), target.join("改名后")).unwrap();
            symlink("../外部目录", target.join("嵌套")).unwrap();
            for mut task in Transfer::restore(&state).unwrap() {
                assert!(task
                    .run(&connection, |_| {})
                    .await
                    .unwrap_err()
                    .contains("变成链接"));
                task.forget().unwrap();
            }
            assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
            connection.close().await;
            println!(
                "{}",
                serde_json::json!({"folderUpload":true,"emptyFolder":true,"merge":true,"overwriteConfirmation":true,"linksSkipped":true,"changedRemoteParentRefused":true})
            );
        }
        #[cfg(not(unix))]
        panic!("该夹具需要 Unix 符号链接");
    } else if mode == "suite" {
        let connection = connect().await;
        let mut output = serde_json::Map::new();
        let mut upload = task(&root, "roundtrip", Direction::Upload);
        data(&upload.local, 64);
        let started = Instant::now();
        upload.run(&connection, |_| {}).await.unwrap();
        output.insert(
            "uploadMiBs".into(),
            serde_json::json!(64.0 / started.elapsed().as_secs_f64()),
        );
        let mut download = Transfer::new(
            Direction::Download,
            profile(),
            root.join("roundtrip-download.local"),
            upload.remote.clone(),
            &root.join("state-download"),
        )
        .unwrap();
        let started = Instant::now();
        download.run(&connection, |_| {}).await.unwrap();
        output.insert(
            "downloadMiBs".into(),
            serde_json::json!(64.0 / started.elapsed().as_secs_f64()),
        );
        assert_eq!(digest(&upload.local), digest(&download.local));
        output.insert(
            "sha256Roundtrip".into(),
            serde_json::json!(digest(&download.local)),
        );
        // Existing targets survive a transfer without explicit overwrite.
        let mut collision = task(&root, "roundtrip", Direction::Upload);
        std::fs::write(&collision.local, b"replacement").unwrap();
        let original = digest(&root.join("data/roundtrip"));
        assert!(collision.run(&connection, |_| {}).await.is_err());
        assert_eq!(digest(&root.join("data/roundtrip")), original);
        collision.allow_replace().unwrap();
        collision.run(&connection, |_| {}).await.unwrap();
        assert_eq!(
            std::fs::read(root.join("data/roundtrip")).unwrap(),
            b"replacement"
        );
        // Download collision and explicitly permitted atomic replacement.
        let mut collision = Transfer::new(
            Direction::Download,
            profile(),
            download.local.clone(),
            upload.remote.clone(),
            &root.join("state-collision-down"),
        )
        .unwrap();
        assert!(collision.run(&connection, |_| {}).await.is_err());
        collision.allow_replace().unwrap();
        collision.run(&connection, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&download.local).unwrap(), b"replacement");
        // Emulate a crash between successful publication and journal unlink.
        for direction in [Direction::Upload, Direction::Download] {
            let state = root.join(format!("publication-{direction:?}"));
            let mut transfer = Transfer::new(
                direction,
                profile(),
                root.join(format!("publication-{direction:?}.local")),
                PathBuf::from(format!("/fixture/data/publication-{direction:?}")),
                &state,
            )
            .unwrap();
            if direction == Direction::Upload {
                data(&transfer.local, 2);
            } else {
                data(&root.join("data/publication-Download"), 2);
            }
            transfer.run(&connection, |_| {}).await.unwrap();
            let mut record = serde_json::to_value(&transfer).unwrap();
            record["publishing"] = true.into();
            std::fs::write(
                state.join(format!("{}.json", transfer.id)),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            let mut restored = Transfer::restore(&state).unwrap().remove(0);
            restored.run(&connection, |_| {}).await.unwrap();
            assert!(Transfer::restore(&state).unwrap().is_empty());
        }
        // Empty files, Unicode/spaces/metacharacters are protocol paths.
        for name in ["empty", "中文 空格;$()"] {
            let mut transfer = task(&root, name, Direction::Upload);
            std::fs::write(&transfer.local, b"").unwrap();
            transfer.run(&connection, |_| {}).await.unwrap();
            assert_eq!(
                std::fs::metadata(root.join("data").join(name))
                    .unwrap()
                    .len(),
                0
            );
        }
        let mut full = Transfer::new(
            Direction::Upload,
            profile(),
            root.join("full.local"),
            PathBuf::from("/limited/full"),
            &root.join("state-full"),
        )
        .unwrap();
        data(&full.local, 4);
        assert!(full.run(&connection, |_| {}).await.is_err());
        let mut names = Vec::new();
        connection
            .list(Path::new("/limited"), |batch| {
                names.extend(batch);
                Ok(())
            })
            .await
            .unwrap();
        assert!(!names.iter().any(|e| e.name == Path::new("full")));
        let mut missing = Transfer::new(
            Direction::Upload,
            profile(),
            full.local.clone(),
            PathBuf::from("/fixture/does-not-exist/file"),
            &root.join("state-missing"),
        )
        .unwrap();
        assert!(missing.run(&connection, |_| {}).await.is_err());
        #[cfg(unix)]
        {
            let victim = root.join("victim");
            std::fs::write(&victim, b"keep me").unwrap();
            let mut invalid = Transfer::new(
                Direction::Download,
                profile(),
                root.join("invalid.local"),
                PathBuf::from("/fixture/data/roundtrip"),
                &root.join("state-invalid"),
            )
            .unwrap();
            std::os::unix::fs::symlink(&victim, invalid.partial_path()).unwrap();
            assert!(invalid.run(&connection, |_| {}).await.is_err());
            assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        }
        let directory = Path::new("/fixture/data/new-dir");
        connection.mkdir(directory).await.unwrap();
        connection
            .rename(directory, Path::new("/fixture/data/renamed-dir"))
            .await
            .unwrap();
        connection
            .remove(Path::new("/fixture/data/renamed-dir"), true)
            .await
            .unwrap();
        let mut count = 0;
        let mut max_batch = 0;
        let started = Instant::now();
        connection
            .list(Path::new("/fixture/data/large"), |batch| {
                count += batch.len();
                max_batch = max_batch.max(batch.len());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(count, 10000);
        assert!(max_batch <= 128);
        output.insert("largeDirectory".into(),serde_json::json!({"entries":count,"maxBatch":max_batch,"ms":started.elapsed().as_millis()}));
        output.insert(
            "passed".into(),
            serde_json::json!([
                "64MiB bidirectional SHA256",
                "no clobber",
                "atomic replacement",
                "publication crash recovery",
                "empty and Unicode names",
                "directory operations",
                "10000 entries",
                "disk full preserves destination",
                "missing directory",
                "symlink partial rejected"
            ]),
        );
        connection.close().await;
        println!("{}", serde_json::Value::Object(output));
    } else if mode.starts_with("pause-") {
        let name = mode.trim_start_matches("pause-");
        let direction = if name == "upload" {
            Direction::Upload
        } else {
            Direction::Download
        };
        let mut transfer = task(&root, name, direction);
        if direction == Direction::Upload {
            data(&transfer.local, 64);
        } else {
            data(&root.join("data").join(name), 64);
        }
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let connection = connect().await;
        let future = transfer.run(&connection, move |p| {
            std::thread::sleep(Duration::from_millis(45));
            if p.bytes >= 32 * 1024 * 1024 {
                trigger.cancel();
            }
        });
        tokio::select! { result=future=>panic!("completed before cancellation: {result:?}"),_=cancel.cancelled()=>{} }
        connection.close().await;
        let restored = Transfer::restore(&root.join(format!("state-{name}")))
            .unwrap()
            .remove(0);
        assert!(restored.done > 0 && restored.done < restored.total);
        println!(
            "{}",
            serde_json::json!({"paused":name,"checkpoint":restored.done,"total":restored.total})
        );
    } else if mode.starts_with("resume-") || mode.starts_with("reject-") {
        let reject = mode.starts_with("reject-");
        let name = mode.split_once('-').unwrap().1;
        let mut transfer = Transfer::restore(&root.join(format!("state-{name}")))
            .unwrap()
            .remove(0);
        let connection = connect().await;
        let mut verified = 0;
        let result = transfer
            .run(&connection, |p| {
                if p.verifying {
                    verified = p.bytes;
                }
            })
            .await;
        if reject {
            assert!(result.is_err());
            println!(
                "{}",
                serde_json::json!({"rejected":name,"reason":result.unwrap_err()})
            );
        } else {
            result.unwrap();
            assert!(verified > 0);
            assert_eq!(
                digest(&transfer.local),
                digest(&root.join("data").join(name))
            );
            println!(
                "{}",
                serde_json::json!({"resumed":name,"verified":verified,"sha256":digest(&transfer.local)})
            );
        }
        connection.close().await;
    } else if mode == "network" {
        let transfer = task(&root, "network", Direction::Upload);
        data(&transfer.local, 256);
        let marker = root.join("network-progress");
        let state = root.join("state-network");
        let transfer_id = transfer.id.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let control = hshell_sftp::start_transfer(
            transfer,
            root.join("profiles.json"),
            move |event| {
                if matches!(&event, hshell_sftp::TransferEvent::Progress(p) if p.bytes>=32*1024*1024)
                    && Transfer::restore(&state).is_ok_and(|tasks| {
                        tasks.iter().any(|task| {
                            task.id == transfer_id && task.done > 0 && task.done < task.total
                        })
                    })
                {
                    std::fs::write(&marker, b"ready").unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                }
                sender.send(event).is_ok()
            },
        );
        loop {
            match receiver.recv_timeout(Duration::from_secs(45)).unwrap() {
                hshell_sftp::TransferEvent::Finished(transfer, result) => {
                    assert!(result.is_err());
                    let checkpoint = Transfer::restore(&root.join("state-network"))
                        .unwrap()
                        .remove(0);
                    assert!(checkpoint.done > 0 && checkpoint.done < checkpoint.total);
                    println!(
                        "{}",
                        serde_json::json!({"networkFailure":result.unwrap_err(),"retained":transfer.done,"checkpoint":checkpoint.done})
                    );
                    break;
                }
                hshell_sftp::TransferEvent::Auth { .. } => panic!("unexpected auth"),
                _ => {}
            }
        }
        drop(control);
        drop(receiver);
        assert!(hshell_sftp::wait_for_shutdown(Duration::from_secs(4)));
    } else {
        panic!("unknown mode");
    }
}
