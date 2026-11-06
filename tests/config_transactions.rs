use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

fn command(root: &Path) -> Command {
    let mut command = Command::new(
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into()),
    );
    for (name, _) in std::env::vars() {
        if name.to_uppercase().starts_with("ZI_")
            || matches!(
                name.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"
            )
        {
            command.env_remove(name);
        }
    }
    command
        .current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .arg("--config")
        .arg(root.join("config.toml"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn seed(root: &Path) -> Vec<u8> {
    let text = b"model='root'\nbase_url='http://localhost:1/v1'\napi_key='TRANSACTION_SECRET_993'\nactive_profile='work'\n[profiles.work]\nmodel='work'\nbase_url='http://localhost:1/v1'\napi_key='TRANSACTION_SECRET_993'\napi='chat'\nmax_output_tokens=128\n[profiles.other]\nmodel='other'\n";
    std::fs::write(root.join("config.toml"), text).unwrap();
    text.to_vec()
}

fn hold_lock(root: &Path) -> File {
    hold_coordination(&root.join("config.toml.lock"))
}

fn hold_coordination(path: &Path) -> File {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: file owns the live descriptor until the returned guard drops.
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }
    file
}

fn check(output: &Output, success: bool) -> String {
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.success(), success, "{message}");
    assert!(output.stdout.is_empty());
    assert!(!message.contains("TRANSACTION_SECRET_993"));
    message
}

#[test]
fn every_offline_writer_obeys_the_os_lock_and_retries_after_release() {
    for args in [
        vec!["use", "other"],
        vec!["config", "--rename", "other", "renamed"],
        vec!["config", "--remove", "other"],
        vec!["config", "--name", "work", "--max-output-tokens", "256"],
        vec!["config", "--name", "work", "--clear-output-budget"],
        vec![
            "config",
            "--name",
            "work",
            "--api-key-env",
            "TRANSACTION_REF_993",
        ],
        vec!["config", "--name", "default", "--clear-saved-key"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let before = seed(root.path());
        let guard = hold_lock(root.path());
        let output = command(root.path()).args(&args).output().unwrap();
        assert!(
            check(&output, false).contains("configuration is busy"),
            "{args:?}"
        );
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
        drop(guard);
        check(&command(root.path()).args(&args).output().unwrap(), true);
        let metadata = std::fs::symlink_metadata(root.path().join("config.toml.lock")).unwrap();
        assert!(metadata.is_file() && !metadata.file_type().is_symlink());
        assert_eq!(metadata.len(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
fn readonly_commands_never_create_coordination_files() {
    let root = tempfile::tempdir().unwrap();
    let before = seed(root.path());
    for args in [
        vec!["config", "--list"],
        vec!["models"],
        vec!["doctor", "--json"],
    ] {
        let output = command(root.path()).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
fn unexpected_coordination_entries_are_never_replaced_or_truncated() {
    for kind in 0..3 {
        if kind == 2 && !cfg!(unix) {
            continue;
        }
        let root = tempfile::tempdir().unwrap();
        let before = seed(root.path());
        let lock = root.path().join("config.toml.lock");
        match kind {
            0 => std::fs::write(&lock, b"USER_OWNED_CONTENT_993").unwrap(),
            1 => std::fs::create_dir(&lock).unwrap(),
            _ => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(root.path().join("config.toml"), &lock).unwrap();
            }
        }
        let output = command(root.path())
            .args(["use", "other"])
            .output()
            .unwrap();
        assert!(check(&output, false).contains("regular empty file"));
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
        let metadata = std::fs::symlink_metadata(&lock).unwrap();
        match kind {
            0 => assert_eq!(std::fs::read(&lock).unwrap(), b"USER_OWNED_CONTENT_993"),
            1 => assert!(metadata.is_dir()),
            _ => assert!(metadata.file_type().is_symlink()),
        }
    }
}

#[test]
fn killing_a_lock_holder_releases_the_lock_without_deleting_the_file() {
    const MARKER: &str = "ZISTRAL_TRANSACTION_LOCK_HOLDER";
    if let Some(root) = std::env::var_os(MARKER) {
        let _guard = hold_lock(Path::new(&root));
        println!("LOCK_READY_993");
        std::io::stdout().flush().unwrap();
        // A bounded channel keeps the guard alive without depending on stdin.
        let (_sender, receiver) = mpsc::channel::<()>();
        let _ = receiver.recv_timeout(Duration::from_secs(30));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let before = seed(root.path());
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "killing_a_lock_holder_releases_the_lock_without_deleting_the_file",
            "--nocapture",
        ])
        .env(MARKER, root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = holder.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap().contains("LOCK_READY_993") {
                let _ = sender.send(());
                break;
            }
        }
    });
    let ready = receiver.recv_timeout(Duration::from_secs(10));
    let output = ready.as_ref().ok().map(|_| {
        command(root.path())
            .args(["use", "other"])
            .output()
            .unwrap()
    });
    holder.kill().unwrap();
    holder.wait().unwrap();
    reader.join().unwrap();
    ready.unwrap();
    assert!(check(&output.unwrap(), false).contains("configuration is busy"));
    assert_eq!(
        std::fs::read(root.path().join("config.toml")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::metadata(root.path().join("config.toml.lock"))
            .unwrap()
            .len(),
        0
    );
    check(
        &command(root.path())
            .args(["use", "other"])
            .output()
            .unwrap(),
        true,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path().join("config.toml.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

fn preset_source(root: &Path, name: &str) -> std::path::PathBuf {
    let source = root.join(format!("{name}.toml"));
    std::fs::write(
        &source,
        format!(
            "version=1\n[presets.{name}]\ndescription='Task'\nprompt='PRIVATE_PRESET_BODY_993'\n"
        ),
    )
    .unwrap();
    source
}

fn import_preset(root: &Path, source: &Path, dry_run: bool) -> Output {
    let mut c = command(root);
    c.arg("--presets")
        .arg(root.join("library.toml"))
        .args(["presets", "import"])
        .arg(source);
    if dry_run {
        c.arg("--dry-run");
    }
    c.output().unwrap()
}

#[test]
fn preset_import_obeys_native_lock_while_preview_stays_readonly_and_retry_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let source = preset_source(root.path(), "first");
    let original = std::fs::read(&source).unwrap();
    let target = root.path().join("library.toml");
    let before = b"version=1\n[presets]\n";
    std::fs::write(&target, before).unwrap();
    let guard = hold_coordination(&root.path().join("library.toml.lock"));
    let preview = import_preset(root.path(), &source, true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Would import"));
    assert_eq!(std::fs::read(&target).unwrap(), before);
    let blocked = import_preset(root.path(), &source, false);
    assert!(
        !blocked.status.success(),
        "preset import ignored another process's native coordination lock"
    );
    assert!(blocked.stdout.is_empty());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("configuration is busy"));
    assert_eq!(std::fs::read(&target).unwrap(), before);
    drop(guard);
    let retry = import_preset(root.path(), &source, false);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(
        saved["presets"]["first"]["prompt"].as_str(),
        Some("PRIVATE_PRESET_BODY_993")
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(
        std::fs::metadata(root.path().join("library.toml.lock"))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    for output in [preview, blocked, retry] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_PRESET_BODY_993"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_PRESET_BODY_993"));
    }
}

#[test]
fn concurrent_preset_imports_never_both_succeed_and_lose_a_definition() {
    let root = tempfile::tempdir().unwrap();
    let sources = [
        preset_source(root.path(), "left"),
        preset_source(root.path(), "right"),
    ];
    let target = root.path().join("library.toml");
    for _ in 0..8 {
        std::fs::write(&target, "version=1\n[presets]\n").unwrap();
        let barrier = std::sync::Barrier::new(3);
        let results = std::thread::scope(|scope| {
            let left = scope.spawn(|| {
                barrier.wait();
                import_preset(root.path(), &sources[0], false)
            });
            let right = scope.spawn(|| {
                barrier.wait();
                import_preset(root.path(), &sources[1], false)
            });
            barrier.wait();
            [left.join().unwrap(), right.join().unwrap()]
        });
        let mut saved: toml::Value =
            toml::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        for (index, name) in ["left", "right"].iter().enumerate() {
            let result = &results[index];
            if result.status.success() {
                assert!(
                    saved["presets"].get(name).is_some(),
                    "two imports reported success but one definition was lost"
                );
            } else {
                assert!(result.stdout.is_empty());
                let error = String::from_utf8_lossy(&result.stderr);
                assert!(
                    error.contains("configuration is busy") || error.contains("changed"),
                    "{error}"
                );
            }
        }
        // Explicit fresh retry retains the winner; there is no hidden merge/retry.
        for (index, name) in ["left", "right"].iter().enumerate() {
            if saved["presets"].get(name).is_none() {
                let retry = import_preset(root.path(), &sources[index], false);
                assert!(
                    retry.status.success(),
                    "{}",
                    String::from_utf8_lossy(&retry.stderr)
                );
                saved = toml::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
            }
        }
        assert!(saved["presets"].get("left").is_some() && saved["presets"].get("right").is_some());
        for output in results {
            assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_PRESET_BODY_993"));
            assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_PRESET_BODY_993"));
        }
    }
}

#[cfg(unix)]
#[test]
fn linked_configuration_is_readable_but_offline_writers_preserve_the_link() {
    for args in [
        vec!["use", "other"],
        vec!["config", "--rename", "other", "renamed"],
        vec!["config", "--remove", "other"],
        vec!["config", "--name", "work", "--max-output-tokens", "256"],
        vec!["config", "--name", "work", "--clear-output-budget"],
        vec![
            "config",
            "--name",
            "work",
            "--api-key-env",
            "TRANSACTION_REF_993",
        ],
        vec!["config", "--name", "default", "--clear-saved-key"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let before = seed(root.path());
        let path = root.path().join("config.toml");
        let target = root.path().join("tracked.toml");
        std::fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink("tracked.toml", &path).unwrap();
        for read in [
            vec!["config", "--list"],
            vec!["models"],
            vec!["doctor", "--json"],
        ] {
            let output = command(root.path()).args(read).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!String::from_utf8_lossy(&output.stdout).contains("TRANSACTION_SECRET_993"));
        }
        let output = command(root.path()).args(&args).output().unwrap();
        assert!(
            check(&output, false).contains("actual regular file path"),
            "{args:?}"
        );
        assert_eq!(
            std::fs::read_link(&path).unwrap(),
            Path::new("tracked.toml")
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read(&target).unwrap(), before);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        // Selecting the real file is the explicit, supported way to edit it.
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&target, &path).unwrap();
        check(&command(root.path()).args(&args).output().unwrap(), true);
    }
}
