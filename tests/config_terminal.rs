use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Exercise the real terminal, including Windows ConPTY and hidden password entry.
fn configure(path: &std::path::Path, answers: &[(&str, &str)]) -> String {
    configure_from(path, answers, None)
}

fn configure_from(
    path: &std::path::Path,
    answers: &[(&str, &str)],
    home: Option<&std::path::Path>,
) -> String {
    configure_named(path, answers, home, None)
}

fn configure_named(
    path: &std::path::Path,
    answers: &[(&str, &str)],
    home: Option<&std::path::Path>,
    name: Option<&str>,
) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 180,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut cmd = CommandBuilder::new(binary);
    cmd.args(["config", "--config", path.to_str().unwrap()]);
    if let Some(name) = name {
        cmd.args(["--name", name]);
    }
    if let Some(home) = home {
        cmd.env("HOME", home);
        cmd.env("USERPROFILE", home);
        cmd.env("CODEX_HOME", home.join(".codex"));
    } else {
        cmd.arg("--manual");
    }
    for name in [
        "ZI_PROFILE",
        "ZI_MODEL",
        "ZI_API",
        "ZI_BASE_URL",
        "ZI_API_KEY",
        "ZI_CONFIG",
        "OPENAI_API_KEY",
    ] {
        cmd.env_remove(name);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut position = 0;
    let mut cursor_queries = 0;
    for (prompt, answer) in answers
        .iter()
        .copied()
        .chain(std::iter::once(("Saved ", "")))
    {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(offset) = transcript[position..].find(prompt) {
                position += offset + prompt.len();
                break;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(bytes) => {
                    transcript.push_str(&String::from_utf8_lossy(&bytes));
                    // ConPTY asks the terminal emulator for its cursor position
                    // before starting console applications.
                    let queries = transcript.matches("\x1b[6n").count();
                    for _ in cursor_queries..queries {
                        writer.write_all(b"\x1b[1;1R").unwrap();
                    }
                    writer.flush().unwrap();
                    cursor_queries = queries;
                }
                Err(_) => {
                    child.kill().ok();
                    panic!("terminal did not reach {prompt:?}; output: {transcript}");
                }
            }
        }
        if prompt != "Saved " {
            // Wait for the end of the label before sending: rpassword must have
            // switched the console to hidden input before the key is typed.
            std::thread::sleep(Duration::from_millis(100));
            write!(writer, "{answer}\r").unwrap();
            writer.flush().unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!("config did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    while let Ok(bytes) = rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&bytes));
    }
    transcript
}

fn source(home: &std::path::Path, path: &str, contents: &str) {
    let path = home.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn detected_workbuddy_import_skips_key_prompt_and_preserves_source() {
    let home = tempfile::tempdir().unwrap();
    let contents = r#"[{"id":"imported-model","url":"https://example.com/v1/chat/completions","apiKey":"import-test-secret"}]"#;
    source(home.path(), ".workbuddy/models.json", contents);
    let path = home.path().join("zi/config.toml");
    let transcript = configure_from(&path, &[("Configuration source", "1")], Some(home.path()));
    assert!(transcript.contains("WorkBuddy"));
    assert!(!transcript.contains("import-test-secret"));
    assert!(!transcript.contains("API Key ["));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["api_key"].as_str(), Some("import-test-secret"));
    assert_eq!(saved["base_url"].as_str(), Some("https://example.com/v1"));
    assert_eq!(
        std::fs::read_to_string(home.path().join(".workbuddy/models.json")).unwrap(),
        contents
    );
}

#[test]
fn detected_codex_without_key_prompts_only_for_key() {
    let home = tempfile::tempdir().unwrap();
    source(
        home.path(),
        ".codex/config.toml",
        "model='imported-model'\nmodel_provider='custom'\n[model_providers.custom]\nbase_url='https://example.com/v1'\nwire_api='responses'\n",
    );
    source(
        home.path(),
        ".codex/auth.json",
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"not-an-api-key"}}"#,
    );
    let path = home.path().join("zi/config.toml");
    let transcript = configure_from(
        &path,
        &[
            ("Configuration source", "1"),
            ("API Key", "supplied-test-secret"),
        ],
        Some(home.path()),
    );
    assert!(transcript.contains("account sign-in"));
    assert!(!transcript.contains("not-an-api-key"));
    assert!(!transcript.contains("supplied-test-secret"));
    assert!(!transcript.contains("Service URL ["));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["api"].as_str(), Some("responses"));
    assert_eq!(saved["api_key"].as_str(), Some("supplied-test-secret"));
}

#[test]
fn detected_dsh_resolves_provider_credential_reference() {
    let home = tempfile::tempdir().unwrap();
    source(
        home.path(),
        ".dsh/profiles/desktop/cordis.patch.yml",
        "- name: '@deepseek-ai/dsh-llm-pi-ai'\n  config:\n    providers:\n      custom:\n        baseURL: https://example.com/anthropic\n        api: anthropic-messages\n        apiKeyEnv: ZISTRAL_TERMINAL_TEST_KEY\n        models: [{id: dsh-model}]\n",
    );
    let credentials = "refs:\n  ZISTRAL_TERMINAL_TEST_KEY: dsh-test-secret\n";
    source(home.path(), ".dsh/.credentials.yaml", credentials);
    let path = home.path().join("zi/config.toml");
    let transcript = configure_from(&path, &[("Configuration source", "1")], Some(home.path()));
    assert!(!transcript.contains("API Key ["));
    assert!(!transcript.contains("dsh-test-secret"));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["model"].as_str(), Some("dsh-model"));
    assert_eq!(saved["api"].as_str(), Some("anthropic"));
    assert_eq!(saved["api_key"].as_str(), Some("dsh-test-secret"));
    assert_eq!(
        std::fs::read_to_string(home.path().join(".dsh/.credentials.yaml")).unwrap(),
        credentials
    );
}

#[test]
fn terminal_setup_hides_key_and_can_keep_existing_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let transcript = configure(
        &path,
        &[
            ("Service URL", "http://localhost:11434/v1"),
            ("Model name", "terminal-test-model"),
            ("API protocol", "chat"),
            ("API Key", "terminal-test-secret"),
        ],
    );
    assert!(
        !transcript.contains("terminal-test-secret"),
        "key was echoed"
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["model"].as_str(), Some("terminal-test-model"));
    assert_eq!(saved["api_key"].as_str(), Some("terminal-test-secret"));
    let transcript = configure(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", ""),
            ("API Key", ""),
        ],
    );
    assert!(!transcript.contains("terminal-test-secret"));
    let updated: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved, updated);
}

#[test]
fn named_imports_preserve_other_models_and_legacy_config() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("config.toml");
    std::fs::write(&path, "model='legacy'\napi_key='legacy-secret'\n").unwrap();
    source(
        home.path(),
        ".workbuddy/models.json",
        r#"[{"id":"one","url":"http://localhost:1/v1","apiKey":"first-secret"}]"#,
    );
    let first = configure_named(
        &path,
        &[("Configuration source", "1")],
        Some(home.path()),
        Some("work"),
    );
    source(
        home.path(),
        ".workbuddy/models.json",
        r#"[{"id":"two","url":"http://localhost:2/v1","apiKey":"second-secret"}]"#,
    );
    let second = configure_named(
        &path,
        &[("Configuration source", "1")],
        Some(home.path()),
        Some("local"),
    );
    let edited = configure_named(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", ""),
            ("API Key", ""),
        ],
        None,
        Some("work"),
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["model"].as_str(), Some("legacy"));
    assert_eq!(saved["api_key"].as_str(), Some("legacy-secret"));
    assert_eq!(saved["profiles"]["work"]["model"].as_str(), Some("one"));
    assert_eq!(saved["profiles"]["local"]["model"].as_str(), Some("two"));
    assert_eq!(saved["active_profile"].as_str(), Some("work"));
    for secret in ["legacy-secret", "first-secret", "second-secret"] {
        assert!(!format!("{first}{second}{edited}").contains(secret));
    }
}
