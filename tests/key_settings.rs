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
    for (key, _) in std::env::vars() {
        if key.to_uppercase().starts_with("ZI_")
            || matches!(
                key.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"
            )
        {
            c.env_remove(key);
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
fn seed(root: &Path) -> toml::Value {
    let text = "model='root'\napi_key='ROOT_SECRET_993'\napi_key_env='ROOT_REF_993'\nactive_profile='work'\n[profiles.work]\nmodel='work'\nbase_url='http://localhost:1/v1'\napi='chat'\napi_key='WORK_SECRET_993'\napi_key_env='OLD_REF_993'\nendpoint='http://localhost:1/exact'\nmax_output_tokens=128\nchat_token_limit_field='max_tokens'\n[profiles.other]\nmodel='other'\napi_key='OTHER_SECRET_993'\n";
    std::fs::write(root.join("config.toml"), text).unwrap();
    toml::from_str(text).unwrap()
}
fn read(root: &Path) -> toml::Value {
    toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap()
}
fn checked(out: &Output, success: bool) {
    assert_eq!(
        out.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    for key in [
        "ROOT_SECRET_993",
        "WORK_SECRET_993",
        "OTHER_SECRET_993",
        "ENV_VALUE_993",
        "GLOBAL_VALUE_993",
        "PRIVATE_INPUT_993",
    ] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(key));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(key));
    }
}

#[test]
fn reference_edits_follow_explicit_then_invocation_then_saved_profile_selection() {
    for (args, ambient, target) in [
        (vec!["config", "--api-key-env", "NEW_REF_993"], None, "work"),
        (
            vec!["-p", "other", "config", "--api-key-env", "NEW_REF_993"],
            None,
            "other",
        ),
        (
            vec!["config", "--api-key-env", "NEW_REF_993"],
            Some("other"),
            "other",
        ),
        (
            vec![
                "-p",
                "other",
                "config",
                "--name",
                "work",
                "--api-key-env",
                "NEW_REF_993",
            ],
            Some("missing"),
            "work",
        ),
        (
            vec![
                "config",
                "--name",
                "default",
                "--api-key-env",
                "NEW_REF_993",
            ],
            Some("missing"),
            "default",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut expected = seed(root);
        let mut c = command(root);
        c.env("NEW_REF_993", "ENV_VALUE_993")
            .env("ZI_API_KEY", "GLOBAL_VALUE_993");
        if let Some(name) = ambient {
            c.env("ZI_PROFILE", name);
        }
        let out = c.args(&args).output().unwrap();
        checked(&out, true);
        if target == "default" {
            expected["api_key_env"] = toml::Value::String("NEW_REF_993".into());
        } else {
            expected["profiles"][target].as_table_mut().unwrap().insert(
                "api_key_env".into(),
                toml::Value::String("NEW_REF_993".into()),
            );
        }
        assert_eq!(read(root), expected);
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
        let lock = root.join("config.toml.lock");
        assert!(lock.is_file());
        assert_eq!(std::fs::metadata(lock).unwrap().len(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.join("config.toml"))
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
fn explicit_key_removal_preserves_references_and_all_unrelated_settings() {
    for target in ["default", "work", "other"] {
        for new_reference in [None, Some("NEXT_REF_993")] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let mut expected = seed(root);
            let mut c = command(root);
            c.args(["config", "--name", target, "--clear-saved-key"]);
            if let Some(name) = new_reference {
                c.args(["--api-key-env", name]);
            }
            checked(&c.output().unwrap(), true);
            let updated = if target == "default" {
                &mut expected
            } else {
                &mut expected["profiles"][target]
            };
            updated.as_table_mut().unwrap().remove("api_key");
            if let Some(name) = new_reference {
                updated
                    .as_table_mut()
                    .unwrap()
                    .insert("api_key_env".into(), toml::Value::String(name.into()));
            }
            assert_eq!(read(root), expected);
            assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
            let lock = root.join("config.toml.lock");
            assert!(lock.is_file());
            assert_eq!(std::fs::metadata(lock).unwrap().len(), 0);
        }
    }
}

fn doctor(root: &Path, selected: &str, global: &str) -> serde_json::Value {
    let before = std::fs::read(root.join("config.toml")).unwrap();
    let out = command(root)
        .env("NEW_REF_993", selected)
        .env("ZI_API_KEY", global)
        .env("OPENAI_API_KEY", "UNRELATED_OPENAI_993")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(std::fs::read(root.join("config.toml")).unwrap(), before);
    for secret in [
        "ENV_VALUE_993",
        "GLOBAL_VALUE_993",
        "WORK_SECRET_993",
        "UNRELATED_OPENAI_993",
    ] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(secret));
    }
    report["configuration"]["details"]["credential"].clone()
}

#[test]
fn reference_edit_preserves_fallback_until_explicit_clear_and_does_not_copy_environment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed(root);
    checked(
        &command(root)
            .args(["config", "--api-key-env", "NEW_REF_993"])
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(
        doctor(root, "ENV_VALUE_993", "")["environment_variable"],
        "NEW_REF_993"
    );
    assert_eq!(doctor(root, "", "")["source"], "profile");
    assert_eq!(
        doctor(root, "ENV_VALUE_993", "GLOBAL_VALUE_993")["environment_variable"],
        "ZI_API_KEY"
    );
    checked(
        &command(root)
            .args(["config", "--clear-saved-key"])
            .output()
            .unwrap(),
        true,
    );
    let report = doctor(root, " ", "");
    assert_eq!(report["configured"], false);
    assert_eq!(report["source"], "none");
    assert_eq!(
        read(root)["profiles"]["work"]["api_key_env"].as_str(),
        Some("NEW_REF_993")
    );
    checked(
        &command(root)
            .args(["config", "--api-key-env", "OPENAI_API_KEY"])
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(
        doctor(root, "", "")["environment_variable"],
        "OPENAI_API_KEY"
    );
}

#[test]
fn invalid_names_modes_missing_profiles_and_parse_errors_do_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed(root);
    let before = std::fs::read(root.join("config.toml")).unwrap();
    std::fs::write(root.join("out"), b"KEEP").unwrap();
    let long = "A".repeat(129);
    for name in [
        "", "1KEY", "$KEY", "${KEY}", "A=B", "A B", "A\nB", "A/B", "Key-Name", "变量", &long,
    ] {
        checked(
            &command(root)
                .args(["config", "--api-key-env", name])
                .output()
                .unwrap(),
            false,
        );
        assert_eq!(std::fs::read(root.join("config.toml")).unwrap(), before);
    }
    for args in [
        vec!["config", "--api-key-env", "KEY", "--manual"],
        vec!["config", "--clear-saved-key", "--list"],
        vec![
            "config",
            "--api-key-env",
            "KEY",
            "--max-output-tokens",
            "32",
        ],
        vec!["config", "--clear-saved-key", "--clear-output-budget"],
        vec![
            "config",
            "--clear-saved-key",
            "--chat-token-limit-field",
            "max_tokens",
        ],
        vec!["config", "--api-key-env", "KEY", "--rename", "work", "new"],
        vec!["config", "--clear-saved-key", "--remove", "other"],
        vec!["config", "--name", "missing", "--clear-saved-key"],
        vec!["config", "--name", "../bad", "--api-key-env", "KEY"],
        vec!["config", "--clear-saved-key", "-i", "MUST_NOT_READ"],
        vec!["config", "--clear-saved-key", "-o", "out", "--force"],
        vec!["--json", "config", "--clear-saved-key"],
        vec!["--preview", "config", "--clear-saved-key"],
        vec!["--agent", "config", "--clear-saved-key"],
        vec!["--stream", "config", "--clear-saved-key"],
    ] {
        checked(&command(root).args(&args).output().unwrap(), false);
        assert_eq!(std::fs::read(root.join("config.toml")).unwrap(), before);
        assert_eq!(std::fs::read(root.join("out")).unwrap(), b"KEEP");
    }
    for invalid in [
        "api_key='WORK_SECRET_993' unknown",
        "api_key='WORK_SECRET_993'\nunknown='OTHER_SECRET_993'\n",
    ] {
        std::fs::write(root.join("config.toml"), invalid).unwrap();
        checked(
            &command(root)
                .args(["config", "--clear-saved-key"])
                .output()
                .unwrap(),
            false,
        );
        assert_eq!(
            std::fs::read_to_string(root.join("config.toml")).unwrap(),
            invalid
        );
    }
    std::fs::remove_file(root.join("config.toml")).unwrap();
    checked(
        &command(root)
            .args(["config", "--api-key-env", "KEY"])
            .output()
            .unwrap(),
        false,
    );
    assert!(!root.join("config.toml").exists());
}

#[test]
fn obsolete_generation_settings_and_nonportable_imported_references_remain_repairable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let text = "api_key='WORK_SECRET_993'\napi_key_env='old-ref'\napi='obsolete'\nmax_output_tokens=0\nchat_token_limit_field='obsolete'\n";
    std::fs::write(root.join("config.toml"), text).unwrap();
    let mut expected: toml::Value = toml::from_str(text).unwrap();
    checked(
        &command(root)
            .args(["config", "--clear-saved-key"])
            .output()
            .unwrap(),
        true,
    );
    expected.as_table_mut().unwrap().remove("api_key");
    assert_eq!(read(root), expected);
    checked(
        &command(root)
            .args(["config", "--api-key-env", "_NEW_123"])
            .output()
            .unwrap(),
        true,
    );
    expected["api_key_env"] = toml::Value::String("_NEW_123".into());
    assert_eq!(read(root), expected);
}

#[test]
fn key_settings_finish_without_reading_open_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed(root);
    let mut child = command(root)
        .args([
            "config",
            "--api-key-env",
            "NEW_REF_993",
            "--clear-saved-key",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut held = child.stdin.take().unwrap();
    held.write_all(b"PRIVATE_INPUT_993").unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("key settings read stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    checked(&child.wait_with_output().unwrap(), true);
    drop(held);
    assert!(read(root)["profiles"]["work"].get("api_key").is_none());
}
