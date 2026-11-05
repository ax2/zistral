use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path) -> Command {
    let mut c = Command::new(
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into()),
    );
    for (k, _) in std::env::vars() {
        if k.to_uppercase().starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"
            )
        {
            c.env_remove(k);
        }
    }
    c.current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .args(["--config", root.join("config.toml").to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}
fn check(out: &Output, success: bool) {
    assert_eq!(
        out.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    for secret in [
        "ROOT_SECRET_993",
        "WORK_SECRET_993",
        "OTHER_SECRET_993",
        "PRIVATE_FIELD_993",
    ] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(secret));
    }
}
fn seed(root: &Path, active: bool) -> toml::Value {
    let text = format!(
        "model='root'\nbase_url='http://localhost:1/v1'\napi_key='ROOT_SECRET_993'\n{}[profiles.work]\nmodel='work-model'\nbase_url='http://localhost:1/custom'\napi='chat'\napi_key='WORK_SECRET_993'\napi_key_env='UNSET_MAINTENANCE_ENV_993'\nendpoint='http://localhost:1/custom/chat/completions'\nmax_output_tokens=128\nchat_token_limit_field='max_tokens'\n[profiles.other]\nmodel='other-model'\napi_key='OTHER_SECRET_993'\n",
        if active {
            "active_profile='work'\n"
        } else {
            ""
        }
    );
    std::fs::write(root.join("config.toml"), &text).unwrap();
    toml::from_str(&text).unwrap()
}
fn read(root: &Path) -> toml::Value {
    toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap()
}

#[test]
fn rename_preserves_every_profile_field_and_moves_only_saved_selection() {
    for active in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut expected = seed(root.path(), active);
        let value = expected["profiles"]
            .as_table_mut()
            .unwrap()
            .remove("work")
            .unwrap();
        expected["profiles"]
            .as_table_mut()
            .unwrap()
            .insert("renamed".into(), value);
        if active {
            expected["active_profile"] = toml::Value::String("renamed".into());
        }
        let out = command(root.path())
            .args(["config", "--rename", "work", "renamed"])
            .output()
            .unwrap();
        check(&out, true);
        assert_eq!(read(root.path()), expected);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        let lock = root.path().join("config.toml.lock");
        assert!(lock.is_file());
        assert_eq!(std::fs::metadata(lock).unwrap().len(), 0);
        let out = command(root.path())
            .args(["--profile", "renamed", "doctor", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let d = &v["configuration"]["details"];
        assert_eq!(d["profile"], "renamed");
        assert_eq!(d["model"], "work-model");
        assert_eq!(d["output_budget"]["tokens_per_turn"], 128);
        assert_eq!(d["output_budget"]["field"], "max_tokens");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("WORK_SECRET_993"));
        let out = command(root.path())
            .env("ZI_PROFILE", "work")
            .args(["--profile", "renamed", "doctor", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(read(root.path()), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.path().join("config.toml"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn remove_is_explicit_inactive_only_and_preserves_default_and_unrelated_keys() {
    let root = tempfile::tempdir().unwrap();
    let mut expected = seed(root.path(), true);
    for name in ["work", "default", "missing"] {
        let before = std::fs::read(root.path().join("config.toml")).unwrap();
        let out = command(root.path())
            .args(["config", "--remove", name])
            .output()
            .unwrap();
        check(&out, false);
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
    }
    let out = command(root.path())
        .env("ZI_PROFILE", "other")
        .args(["config", "--remove", "other"])
        .output()
        .unwrap();
    check(&out, true);
    expected["profiles"].as_table_mut().unwrap().remove("other");
    assert_eq!(read(root.path()), expected);
    let out = command(root.path())
        .args(["use", "default"])
        .output()
        .unwrap();
    check(&out, true);
    expected.as_table_mut().unwrap().remove("active_profile");
    let out = command(root.path())
        .args(["config", "--remove", "work"])
        .output()
        .unwrap();
    check(&out, true);
    expected.as_table_mut().unwrap().remove("profiles");
    assert_eq!(read(root.path()), expected);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    let lock = root.path().join("config.toml.lock");
    assert!(lock.is_file());
    assert_eq!(std::fs::metadata(lock).unwrap().len(), 0);
    let after = std::fs::read(root.path().join("config.toml")).unwrap();
    assert!(!String::from_utf8_lossy(&after).contains("WORK_SECRET_993"));
    assert!(String::from_utf8_lossy(&after).contains("ROOT_SECRET_993"));
}

#[test]
fn collisions_reserved_names_invalid_names_and_conflicting_modes_are_no_write() {
    let root = tempfile::tempdir().unwrap();
    seed(root.path(), true);
    let before = std::fs::read(root.path().join("config.toml")).unwrap();
    std::fs::write(root.path().join("out"), "KEEP").unwrap();
    let long = "x".repeat(65);
    let cases = vec![
        vec!["config", "--rename", "work", "other"],
        vec!["config", "--rename", "work", "work"],
        vec!["config", "--rename", "default", "new"],
        vec!["config", "--rename", "work", "default"],
        vec!["config", "--rename", "missing", "new"],
        vec!["config", "--rename", "work", "bad/name"],
        vec!["config", "--rename", "work", &long],
        vec!["config", "--remove", "../work"],
        vec!["config", "--remove", "work", "--rename", "work", "new"],
        vec!["config", "--rename", "work", "new", "--manual"],
        vec!["config", "--rename", "work", "new", "--name", "other"],
        vec!["config", "--remove", "other", "--list"],
        vec!["config", "--remove", "other", "--max-output-tokens", "64"],
        vec!["config", "--rename", "work", "new", "--clear-output-budget"],
        vec![
            "config",
            "--rename",
            "work",
            "new",
            "--chat-token-limit-field",
            "max_tokens",
        ],
        vec!["config", "--rename", "work"],
        vec!["config", "--remove", "other", "-i", "DO_NOT_READ"],
        vec!["config", "--remove", "other", "-o", "out", "--force"],
        vec!["--json", "config", "--remove", "other"],
        vec!["--agent", "config", "--remove", "other"],
        vec!["--preview", "config", "--remove", "other"],
    ];
    for args in cases {
        let out = command(root.path()).args(&args).output().unwrap();
        check(&out, false);
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before,
            "{args:?}"
        );
        assert_eq!(std::fs::read(root.path().join("out")).unwrap(), b"KEEP");
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn metadata_edits_handle_obsolete_settings_full_catalogues_and_safe_parse_errors() {
    let root = tempfile::tempdir().unwrap();
    seed(root.path(), true);
    let mut doc = read(root.path());
    doc["profiles"]["other"].as_table_mut().unwrap().insert(
        "chat_token_limit_field".into(),
        toml::Value::String("PRIVATE_FIELD_993".into()),
    );
    for n in 0..126 {
        doc["profiles"]
            .as_table_mut()
            .unwrap()
            .insert(format!("p{n}"), toml::Value::Table(toml::map::Map::new()));
    }
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&doc).unwrap(),
    )
    .unwrap();
    let out = command(root.path())
        .args(["config", "--rename", "other", "archived"])
        .output()
        .unwrap();
    check(&out, true);
    assert_eq!(read(root.path())["profiles"].as_table().unwrap().len(), 128);
    assert_eq!(
        read(root.path())["profiles"]["archived"]["chat_token_limit_field"].as_str(),
        Some("PRIVATE_FIELD_993")
    );
    let out = command(root.path())
        .args(["config", "--remove", "archived"])
        .output()
        .unwrap();
    check(&out, true);
    assert_eq!(read(root.path())["profiles"].as_table().unwrap().len(), 127);
    std::fs::write(
        root.path().join("config.toml"),
        "api_key='WORK_SECRET_993'\nunknown='PRIVATE_FIELD_993'\n",
    )
    .unwrap();
    let before = std::fs::read(root.path().join("config.toml")).unwrap();
    for args in [
        ["config", "--remove", "other"].as_slice(),
        ["config", "--rename", "work", "new"].as_slice(),
    ] {
        let out = command(root.path()).args(args).output().unwrap();
        check(&out, false);
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
    }
    std::fs::remove_file(root.path().join("config.toml")).unwrap();
    let out = command(root.path())
        .args(["config", "--remove", "other"])
        .output()
        .unwrap();
    check(&out, false);
    assert!(!root.path().join("config.toml").exists());
}

#[test]
fn explicit_maintenance_does_not_wait_for_open_stdin_or_follow_ambient_profile() {
    let root = tempfile::tempdir().unwrap();
    seed(root.path(), false);
    let mut child = command(root.path())
        .env("ZI_PROFILE", "unknown")
        .args(["config", "--rename", "work", "new"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut held = child.stdin.take().unwrap();
    held.write_all(b"PRIVATE_INPUT_993").unwrap();
    let end = Instant::now() + Duration::from_secs(6);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > end {
            child.kill().unwrap();
            panic!("profile edit read stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().unwrap();
    check(&out, true);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_INPUT_993"));
    drop(held);
    let out = command(root.path())
        .env("ZI_PROFILE", "other")
        .args(["config", "--remove", "new"])
        .output()
        .unwrap();
    check(&out, true);
    let value = read(root.path());
    assert!(value["profiles"].get("other").is_some());
    assert!(value["profiles"].get("new").is_none());
    assert!(value.get("active_profile").is_none());
}
