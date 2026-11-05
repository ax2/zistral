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
    let file = options.open(root.join("config.toml.lock")).unwrap();
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
