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
    configure_outcome(path, answers, home, name, "Saved ", true)
}

fn configure_outcome(
    path: &std::path::Path,
    answers: &[(&str, &str)],
    home: Option<&std::path::Path>,
    name: Option<&str>,
    finish: &str,
    expected_success: bool,
) -> String {
    configure_outcome_env(path, answers, home, name, finish, expected_success, &[])
}

fn configure_outcome_env(
    path: &std::path::Path,
    answers: &[(&str, &str)],
    home: Option<&std::path::Path>,
    name: Option<&str>,
    finish: &str,
    expected_success: bool,
    environment: &[(&str, &str)],
) -> String {
    let (transcript, success) =
        configure_with_hook(path, answers, home, name, &[finish], environment, || {});
    assert_eq!(success, expected_success, "{transcript}");
    transcript
}

fn configure_with_hook(
    path: &std::path::Path,
    answers: &[(&str, &str)],
    home: Option<&std::path::Path>,
    name: Option<&str>,
    finishes: &[&str],
    environment: &[(&str, &str)],
    before_first_answer: impl FnOnce(),
) -> (String, bool) {
    let mut hook = Some(before_first_answer);
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
    for (name, value) in environment {
        cmd.env(*name, *value);
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
    for (step, (prompt, answer)) in answers
        .iter()
        .copied()
        .chain(std::iter::once(("", "")))
        .enumerate()
    {
        let wanted = if step < answers.len() {
            std::slice::from_ref(&prompt)
        } else {
            finishes
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some((offset, length)) = wanted.iter().find_map(|text| {
                transcript[position..]
                    .find(text)
                    .map(|offset| (offset, text.len()))
            }) {
                position += offset + length;
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
                    panic!("terminal did not reach {wanted:?}; output: {transcript}");
                }
            }
        }
        // The final entry only observes completion (including expected errors).
        // Do not send a synthetic Enter after the child has already exited.
        if step < answers.len() {
            if step == 0 {
                hook.take().unwrap()();
            }
            // Wait for the end of the label before sending: rpassword must have
            // switched the console to hidden input before the key is typed.
            std::thread::sleep(Duration::from_millis(100));
            write!(writer, "{answer}\r").unwrap();
            writer.flush().unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let success = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.success();
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!("config did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    while let Ok(bytes) = rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&bytes));
    }
    (transcript, success)
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

#[test]
fn manual_model_setup_preserves_saved_budgets_and_other_profiles() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("config.toml");
    std::fs::write(&path, "model='fixture'\nbase_url='http://localhost:1/v1'\napi='chat'\napi_key='manual-budget-secret'\nmax_output_tokens=128\nchat_token_limit_field='max_tokens'\n[profiles.other]\nmodel='other'\napi='responses'\nmax_output_tokens=32\n").unwrap();
    let output = configure(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", ""),
            ("API Key", ""),
        ],
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["max_output_tokens"].as_integer(), Some(128));
    assert_eq!(saved["chat_token_limit_field"].as_str(), Some("max_tokens"));
    assert_eq!(
        saved["profiles"]["other"]["max_output_tokens"].as_integer(),
        Some(32)
    );
    assert_eq!(saved["api_key"].as_str(), Some("manual-budget-secret"));
    assert!(!output.contains("manual-budget-secret"));
}

#[test]
fn importing_a_replacement_service_resets_only_its_saved_budget() {
    let home = tempfile::tempdir().unwrap();
    source(
        home.path(),
        ".workbuddy/models.json",
        r#"[{"id":"replacement","url":"https://example.com/v1/chat/completions","apiKey":"replacement-secret"}]"#,
    );
    let path = home.path().join("config.toml");
    std::fs::write(&path,"model='old'\nbase_url='http://localhost:1/v1'\napi='chat'\nmax_output_tokens=128\nchat_token_limit_field='max_tokens'\n[profiles.other]\nmodel='other'\nmax_output_tokens=64\n").unwrap();
    let output = configure_from(&path, &[("Configuration source", "1")], Some(home.path()));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["model"].as_str(), Some("replacement"));
    assert!(saved.get("max_output_tokens").is_none());
    assert!(saved.get("chat_token_limit_field").is_none());
    assert_eq!(
        saved["profiles"]["other"]["max_output_tokens"].as_integer(),
        Some(64)
    );
    assert!(!output.contains("replacement-secret"));
}

#[test]
fn incompatible_protocol_edit_preserves_original_config() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("config.toml");
    let original = "model='fixture'\nbase_url='http://localhost:1/v1'\napi='chat'\napi_key='protocol-edit-secret'\nmax_output_tokens=128\nchat_token_limit_field='max_tokens'\n";
    std::fs::write(&path, original).unwrap();
    let output = configure_outcome(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", "responses"),
            ("API Key", ""),
        ],
        None,
        None,
        "requires the chat protocol",
        false,
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(!output.contains("protocol-edit-secret"));
}

fn environment_import_fixture(
    home: &std::path::Path,
    case: usize,
    url: &str,
    name: &str,
) -> (&'static str, Vec<(std::path::PathBuf, Vec<u8>)>) {
    let (api, files):(&str,Vec<(&str,String)>)=match case {
        0 => ("responses",vec![(".codex/config.toml",format!("model='env-model'\nmodel_provider='custom'\n[model_providers.custom]\nbase_url='{url}'\nwire_api='responses'\nenv_key='{name}'\n"))]),
        1 => ("responses",vec![(".codex/config.toml",format!("model='env-model'\nmodel_provider='custom'\n[model_providers.custom]\nbase_url='{url}'\nwire_api='responses'\nexperimental_bearer_token='${{{name}}}'\n"))]),
        2 => ("responses",vec![(".codex/config.toml",format!("model='env-model'\n[model_providers.openai]\nbase_url='{url}'\n")),(".codex/auth.json",r#"{"OPENAI_API_KEY":"AUTH_FILE_FALLBACK_993","tokens":{"access_token":"NEVER_IMPORT_SESSION_993"}}"#.into())]),
        3 => ("chat",vec![(".workbuddy/models.json",serde_json::json!([{"id":"env-model","url":format!("{url}/chat/completions"),"apiKeyEnv":name}]).to_string())]),
        4 => ("chat",vec![(".codebuddy/models.json",serde_json::json!({"models":[{"id":"env-model","url":url,"apiKey":format!("${{{name}}}")}]}).to_string())]),
        5 => ("anthropic",vec![(".dsh/profiles/desktop/cordis.yml",format!("- id: llm\n  config:\n    providers:\n      custom:\n        baseURL: {url}\n        api: anthropic-messages\n        apiKeyEnv: {name}\n        models: [env-model]\n"))]),
        _ => ("anthropic",vec![(".dsh/profiles/desktop/cordis.yml",format!("- id: llm\n  config:\n    providers:\n      custom:\n        baseURL: {url}\n        api: anthropic-messages\n        apiKeyEnv: ZISTRAL_DSH_ALIAS_993\n        models: [env-model]\n")),(".dsh/.credentials.yaml",format!("refs:\n  ZISTRAL_DSH_ALIAS_993: '${{{name}}}'\n"))]),
    };
    let originals = files
        .into_iter()
        .map(|(path, text)| {
            source(home, path, &text);
            (home.join(path), text.into_bytes())
        })
        .collect();
    (api, originals)
}

fn imported_command(
    home: &std::path::Path,
    path: &std::path::Path,
    name: &str,
    key: &str,
) -> std::process::Command {
    let mut c = std::process::Command::new(
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into()),
    );
    for (k, _) in std::env::vars() {
        if k.to_uppercase().starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(k);
        }
    }
    if name != "OPENAI_API_KEY" {
        c.env("OPENAI_API_KEY", "UNRELATED_OPENAI_993");
    }
    c.current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("NO_PROXY", "*")
        .env(name, key)
        .args(["--config", path.to_str().unwrap(), "--timeout", "3"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    c
}

#[test]
fn imported_environment_key_is_a_reference_not_a_plaintext_snapshot() {
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
    };
    for case in 0..7 {
        let home = tempfile::tempdir().unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", l.local_addr().unwrap());
        let name = if case == 2 {
            "OPENAI_API_KEY"
        } else {
            "ZISTRAL_IMPORTED_REF_993"
        };
        let (api, originals) = environment_import_fixture(home.path(), case, &url, name);
        let path = home.path().join("zi/config.toml");
        source(
            home.path(),
            "zi/config.toml",
            "model='root-model'\nbase_url='http://localhost:1/v1'\napi_key='ROOT_EXISTING_993'\n[profiles.kept]\nmodel='keep-model'\napi_key='OTHER_EXISTING_993'\n",
        );
        let output = configure_outcome_env(
            &path,
            &[("Configuration source", "1")],
            Some(home.path()),
            Some("work"),
            "Saved ",
            true,
            &[
                (name, "IMPORTED_FIRST_SECRET_993"),
                ("ZISTRAL_DSH_ALIAS_993", ""),
            ],
        );
        assert!(
            !output.contains("IMPORTED_FIRST_SECRET_993")
                && !output.contains("AUTH_FILE_FALLBACK_993")
                && !output.contains("NEVER_IMPORT_SESSION_993")
        );
        assert!(!output.contains("API Key ["));
        let saved = std::fs::read(&path).unwrap();
        let file: toml::Value = toml::from_str(std::str::from_utf8(&saved).unwrap()).unwrap();
        let p = &file["profiles"]["work"];
        assert_eq!(
            p.get("api_key_env").and_then(toml::Value::as_str),
            Some(name),
            "import must retain the environment reference"
        );
        assert!(p.get("api_key").is_none());
        assert_eq!(p["base_url"].as_str(), Some(url.as_str()));
        assert!(p.get("endpoint").is_none());
        assert_eq!(p["api"].as_str(), Some(api));
        assert_eq!(file["active_profile"].as_str(), Some("work"));
        assert_eq!(file["api_key"].as_str(), Some("ROOT_EXISTING_993"));
        assert_eq!(
            file["profiles"]["kept"]["api_key"].as_str(),
            Some("OTHER_EXISTING_993")
        );
        assert!(!String::from_utf8_lossy(&saved).contains("IMPORTED_FIRST_SECRET_993"));
        let worker = std::thread::spawn(move || {
            for expected_key in ["IMPORTED_FIRST_SECRET_993", "IMPORTED_ROTATED_SECRET_993"] {
                let end = Instant::now() + Duration::from_secs(8);
                let mut s = loop {
                    match l.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < end);
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                s.set_nonblocking(false).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut reader = BufReader::new(s.try_clone().unwrap());
                let mut header = String::new();
                let mut size = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        if k.eq_ignore_ascii_case("content-length") {
                            size = v.trim().parse::<usize>().unwrap();
                        }
                    }
                    header.push_str(&line);
                }
                assert!(size < 131072);
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"], "env-model");
                let auth = header
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| {
                        k.eq_ignore_ascii_case(if api == "anthropic" {
                            "x-api-key"
                        } else {
                            "authorization"
                        })
                    })
                    .map(|(_, v)| v.trim());
                let expected = if api == "anthropic" {
                    expected_key.to_owned()
                } else {
                    format!("Bearer {expected_key}")
                };
                assert!(
                    auth == Some(expected.as_str()),
                    "imported request used wrong credential"
                );
                let reply=match api{"responses"=>serde_json::json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}),"anthropic"=>serde_json::json!({"stop_reason":"end_turn","content":[{"type":"text","text":"OK"}]}),_=>serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]})}.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                s.write_all(response.as_bytes()).unwrap();
            }
        });
        for key in ["IMPORTED_FIRST_SECRET_993", "IMPORTED_ROTATED_SECRET_993"] {
            let out = imported_command(home.path(), &path, name, key)
                .args(["--no-tools", "--no-stream", "Reply with OK."])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"OK\n");
            assert!(out.stderr.is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), saved);
        }
        worker.join().unwrap();
        let out = imported_command(home.path(), &path, name, "")
            .args(["doctor", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            report["configuration"]["details"]["credential"]["configured"],
            false
        );
        assert_eq!(
            report["configuration"]["details"]["credential"]["source"],
            "none"
        );
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn missing_import_environment_prompts_for_literal_key_without_claiming_a_reference() {
    for case in [0, 3, 5] {
        let home = tempfile::tempdir().unwrap();
        let (_, originals) = environment_import_fixture(
            home.path(),
            case,
            "http://localhost:1/v1",
            "ZISTRAL_MISSING_IMPORTED_REF_993",
        );
        let path = home.path().join("zi/config.toml");
        let output = configure_outcome_env(
            &path,
            &[
                ("Configuration source", "1"),
                ("API Key [", "SUPPLIED_FALLBACK_993"),
            ],
            Some(home.path()),
            None,
            "Saved ",
            true,
            &[("ZISTRAL_MISSING_IMPORTED_REF_993", "")],
        );
        assert!(!output.contains("SUPPLIED_FALLBACK_993"));
        let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["api_key"].as_str(), Some("SUPPLIED_FALLBACK_993"));
        assert!(saved.get("api_key_env").is_none());
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn unavailable_source_environment_keeps_existing_literal_fallback_precedence() {
    let home = tempfile::tempdir().unwrap();
    source(
        home.path(),
        ".workbuddy/models.json",
        r#"[{"id":"env-model","url":"http://localhost:1/v1","apiKeyEnv":"ZISTRAL_MISSING_IMPORTED_REF_993","apiKey":"SOURCE_LITERAL_FALLBACK_993"}]"#,
    );
    let path = home.path().join("zi/config.toml");
    let output = configure_outcome_env(
        &path,
        &[("Configuration source", "1")],
        Some(home.path()),
        None,
        "Saved ",
        true,
        &[("ZISTRAL_MISSING_IMPORTED_REF_993", "")],
    );
    assert!(!output.contains("API Key [") && !output.contains("SOURCE_LITERAL_FALLBACK_993"));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        saved["api_key"].as_str(),
        Some("SOURCE_LITERAL_FALLBACK_993")
    );
    assert!(saved.get("api_key_env").is_none());
}

#[test]
fn manual_setup_keeps_available_environment_credentials_without_saving_values() {
    for (host, reference, environment, saved_key, answer) in [
        (
            "api.openai.com",
            Some("ZISTRAL_MANUAL_REF_993"),
            "ZISTRAL_MANUAL_REF_993",
            None,
            "",
        ),
        ("api.openai.com", None, "OPENAI_API_KEY", None, ""),
        (
            "api.openai.com",
            Some("ZISTRAL_MANUAL_REF_993"),
            "ZI_API_KEY",
            None,
            "",
        ),
        (
            "api.anthropic.com",
            Some("ZISTRAL_MANUAL_REF_993"),
            "ZISTRAL_MANUAL_REF_993",
            None,
            "",
        ),
        (
            "api.openai.com",
            Some("ZISTRAL_MANUAL_REF_993"),
            "ZISTRAL_MANUAL_REF_993",
            Some("SAVED_FALLBACK_993"),
            "",
        ),
        (
            "api.openai.com",
            Some("ZISTRAL_MANUAL_REF_993"),
            "ZISTRAL_MANUAL_REF_993",
            Some("SAVED_FALLBACK_993"),
            "-",
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("config.toml");
        let api = if host == "api.anthropic.com" {
            "anthropic"
        } else {
            "chat"
        };
        let mut document = format!(
            "model='root'\napi_key='ROOT_KEEP_993'\nactive_profile='work'\n[profiles.work]\nmodel='before'\nbase_url='https://{host}/v1'\napi='{api}'\nmax_output_tokens=128\n[profiles.other]\nmodel='other'\napi_key='OTHER_KEEP_993'\n"
        );
        // Add fields within the selected profile, never to the unrelated table.
        let mut fields = String::new();
        if let Some(reference) = reference {
            fields.push_str(&format!("api_key_env='{reference}'\n"));
        }
        if let Some(key) = saved_key {
            fields.push_str(&format!("api_key='{key}'\n"));
        }
        document = document.replace("[profiles.other]", &format!("{fields}[profiles.other]"));
        std::fs::write(&path, &document).unwrap();
        let mut expected: toml::Value = toml::from_str(&document).unwrap();
        expected["profiles"]["work"]["model"] = toml::Value::String("after".into());
        if answer == "-" {
            expected["profiles"]["work"]
                .as_table_mut()
                .unwrap()
                .remove("api_key");
        }
        let transcript = configure_outcome_env(
            &path,
            &[
                ("Service URL", ""),
                ("Model name", "after"),
                ("API protocol", ""),
                ("API Key", answer),
            ],
            None,
            Some("work"),
            "Saved ",
            true,
            &[(environment, "ENV_VALUE_NOT_SAVED_993")],
        );
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(toml::from_str::<toml::Value>(&raw).unwrap(), expected);
        for secret in [
            "ENV_VALUE_NOT_SAVED_993",
            "SAVED_FALLBACK_993",
            "ROOT_KEEP_993",
            "OTHER_KEEP_993",
        ] {
            assert!(!transcript.contains(secret));
        }
        assert!(!raw.contains("ENV_VALUE_NOT_SAVED_993"));
        assert!(!transcript.contains("API Key is required"));
        let out = imported_command(home.path(), &path, environment, "ENV_VALUE_NOT_SAVED_993")
            .args(["-p", "work", "doctor", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let credential = &report["configuration"]["details"]["credential"];
        assert_eq!(credential["configured"], true);
        assert_eq!(credential["source"], "environment");
        assert_eq!(credential["environment_variable"], environment);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        assert!(!String::from_utf8_lossy(&out.stdout).contains("ENV_VALUE_NOT_SAVED_993"));
    }
}

#[test]
fn manual_official_setup_requires_effective_key_but_local_services_remain_optional() {
    for (host, api, key_required) in [
        ("api.openai.com", "chat", true),
        ("api.anthropic.com", "anthropic", true),
        ("localhost:11434", "chat", false),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("config.toml");
        let scheme = if key_required { "https" } else { "http" };
        std::fs::write(&path, format!("model='before'\nbase_url='{scheme}://{host}/v1'\napi='{api}'\napi_key_env='ZISTRAL_MANUAL_REF_993'\n")).unwrap();
        let mut answers = vec![
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", ""),
            ("API Key", ""),
        ];
        if key_required {
            answers.push((
                "API Key is required for this service.",
                "FALLBACK_ENTERED_993",
            ));
        }
        let transcript = configure_outcome_env(
            &path,
            &answers,
            None,
            None,
            "Saved ",
            true,
            &[
                ("ZISTRAL_MANUAL_REF_993", " "),
                ("OPENAI_API_KEY", "UNRELATED_OPENAI_993"),
            ],
        );
        let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.get("api_key").and_then(toml::Value::as_str),
            if key_required {
                Some("FALLBACK_ENTERED_993")
            } else {
                None
            }
        );
        assert_eq!(
            saved["api_key_env"].as_str(),
            Some("ZISTRAL_MANUAL_REF_993")
        );
        for secret in ["FALLBACK_ENTERED_993", "UNRELATED_OPENAI_993"] {
            assert!(!transcript.contains(secret));
        }
    }
}

#[test]
fn manual_configuration_quotes_legacy_environment_names_without_changing_references() {
    for (name, expected) in [
        ("ZISTRAL_SETUP\nKEY", r#""ZISTRAL_SETUP\nKEY""#),
        ("ZISTRAL_SETUP\x1b[2JKEY", r#""ZISTRAL_SETUP\u001b[2JKEY""#),
        (
            "ZISTRAL_SETUP\u{009b}2JKEY",
            r#""ZISTRAL_SETUP\u009b2JKEY""#,
        ),
        ("ZISTRAL_SETUP\u{007f}KEY", r#""ZISTRAL_SETUP\u007fKEY""#),
        ("ZISTRAL_SETUP_中\"KEY", r#""ZISTRAL_SETUP_中\"KEY""#),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let config = format!(
            "model='fixture'\nbase_url='http://localhost:1/v1'\napi='chat'\napi_key_env={}\n[profiles.other]\nmodel='other'\napi_key='OTHER_DISPLAY_SECRET_993'\n",
            expected
        );
        let fixture: toml::Value = toml::from_str(&config).unwrap();
        assert_eq!(fixture["api_key_env"].as_str(), Some(name));
        std::fs::write(&path, &config).unwrap();
        let finish = format!("Environment variables still override saved settings: {expected}");
        let output = configure_outcome_env(
            &path,
            &[
                ("Service URL", ""),
                ("Model name", ""),
                ("API protocol", ""),
                ("API Key", ""),
            ],
            None,
            None,
            &finish,
            true,
            &[(name, "DISPLAY_ENV_VALUE_993")],
        );
        assert_eq!(
            output.matches(expected).count(),
            2,
            "both source notice and final override warning must quote the exact variable name"
        );
        for secret in ["DISPLAY_ENV_VALUE_993", "OTHER_DISPLAY_SECRET_993"] {
            assert!(!output.contains(secret));
        }
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            toml::from_str::<toml::Value>(&saved).unwrap(),
            toml::from_str::<toml::Value>(&config).unwrap()
        );
        assert!(!saved.contains("DISPLAY_ENV_VALUE_993"));
    }
}

#[test]
fn repair_invalid_url_and_display_control_defaults_without_rewriting_metadata() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config\u{009b}2J.toml");
    let model = "before\nline\t中\x1b[2J\u{009b}2J\u{007f}\"\\tail";
    let shown = r#""before\nline\t中\u001b[2J\u009b2J\u007f\"\\tail""#;
    let original = format!(
        "model={shown}\nbase_url='https://user:URL_DEFAULT_SECRET_993@service.example/v1'\napi='chat'\napi_key='SAVED_DEFAULT_SECRET_993'\napi_key_env='UNSET_DISPLAY_REF_993'\nendpoint='https://service.example/exact'\nmax_output_tokens=128\n[profiles.other]\nmodel='other'\napi_key='OTHER_DEFAULT_SECRET_993'\n"
    );
    std::fs::write(&path, &original).unwrap();
    let mut expected: toml::Value = toml::from_str(&original).unwrap();
    assert_eq!(expected["model"].as_str(), Some(model));
    expected["base_url"] = toml::Value::String("http://localhost:1/v1".into());
    expected.as_table_mut().unwrap().remove("endpoint");
    let model_prompt = format!("Model name [{shown}]");
    let shown_path = serde_json::to_string(path.to_str().unwrap())
        .unwrap()
        .replace('\u{009b}', "\\u009b");
    let finish = format!("Saved {shown_path}.");
    let output = configure_outcome_env(
        &path,
        &[
            ("Service URL []", ""),
            ("Service URL []", "http://localhost:1/v1"),
            (&model_prompt, ""),
            ("API protocol", ""),
            ("API Key", ""),
        ],
        None,
        None,
        &finish,
        true,
        &[("UNSET_DISPLAY_REF_993", "")],
    );
    for secret in [
        "URL_DEFAULT_SECRET_993",
        "SAVED_DEFAULT_SECRET_993",
        "OTHER_DEFAULT_SECRET_993",
    ] {
        assert!(!output.contains(secret));
    }
    assert_eq!(output.matches(r"config\u009b2J.toml").count(), 2);
    assert!(!output.contains(model));
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(toml::from_str::<toml::Value>(&saved).unwrap(), expected);
    for args in [["doctor"].as_slice(), ["models"].as_slice()] {
        let out = imported_command(root.path(), &path, "UNSET_DISPLAY_REF_993", "")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(shown), "{text}");
        assert!(!text.contains(model));
        for secret in ["SAVED_DEFAULT_SECRET_993", "OTHER_DEFAULT_SECRET_993"] {
            assert!(!text.contains(secret));
        }
    }
    let out = imported_command(root.path(), &path, "UNSET_DISPLAY_REF_993", "")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["configuration"]["details"]
            ["model"],
        model
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn stale_wizard_preserves_a_concurrent_cli_key_removal_and_can_be_restarted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    std::fs::write(&path, "model='root'\nactive_profile='work'\n[profiles.work]\nmodel='work'\nbase_url='http://localhost:1/v1'\napi='chat'\napi_key='WORK_CONCURRENT_SECRET_993'\n[profiles.other]\nmodel='other'\napi_key='REMOVED_CONCURRENT_SECRET_993'\n").unwrap();
    let mut newer = None;
    let (output, success) = configure_with_hook(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", "edited"),
            ("API protocol", ""),
            ("API Key", ""),
        ],
        None,
        Some("work"),
        &["Saved ", "zi: "],
        &[],
        || {
            let out = imported_command(root.path(), &path, "UNUSED_CONCURRENT_ENV_993", "")
                .args([
                    "config",
                    "--name",
                    "other",
                    "--api-key-env",
                    "NEW_CONCURRENT_REF_993",
                    "--clear-saved-key",
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stdout.is_empty());
            let observed: toml::Value =
                toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(observed["profiles"]["other"].get("api_key").is_none());
            assert_eq!(
                observed["profiles"]["other"]["api_key_env"].as_str(),
                Some("NEW_CONCURRENT_REF_993")
            );
            newer = Some(std::fs::read(&path).unwrap());
        },
    );
    assert!(
        std::fs::read(&path).unwrap() == newer.unwrap(),
        "stale wizard overwrote a newer CLI configuration and restored a removed key"
    );
    assert!(!success, "stale wizard must report a save conflict");
    assert!(output.contains("configuration changed"));
    assert!(!output.contains("Saved "));
    let retry = configure_named(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", "edited"),
            ("API protocol", ""),
            ("API Key", ""),
        ],
        None,
        Some("work"),
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["profiles"]["work"]["model"].as_str(), Some("edited"));
    assert!(saved["profiles"]["other"].get("api_key").is_none());
    assert_eq!(
        saved["profiles"]["other"]["api_key_env"].as_str(),
        Some("NEW_CONCURRENT_REF_993")
    );
    for secret in [
        "WORK_CONCURRENT_SECRET_993",
        "REMOVED_CONCURRENT_SECRET_993",
    ] {
        assert!(!output.contains(secret));
        assert!(!retry.contains(secret));
    }
}

#[test]
fn stale_wizard_preserves_external_replacement_deletion_and_first_setup_creation() {
    for scenario in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        if scenario != 2 {
            std::fs::write(&path, "base_url='http://localhost:1/v1'\nmodel='old'\napi='chat'\napi_key='OLD_TRANSACTION_SECRET_993'\n").unwrap();
        }
        let (output, success) = configure_with_hook(
            &path,
            &[
                ("Service URL", "http://localhost:1/v1"),
                ("Model name", "edited"),
                ("API protocol", "chat"),
                ("API Key", "FAKE_TRANSACTION_SECRET_993"),
            ],
            Some(root.path()),
            None,
            &["Saved ", "zi: "],
            &[],
            || match scenario {
                0 => std::fs::write(&path, [0xff, 0xfe]).unwrap(),
                1 => std::fs::remove_file(&path).unwrap(),
                _ => std::fs::write(&path, b"# externally created\nmodel='new'\n").unwrap(),
            },
        );
        assert!(!success, "{output}");
        assert!(output.contains("configuration changed"), "{output}");
        assert!(!output.contains("Saved "));
        assert!(!output.contains("FAKE_TRANSACTION_SECRET_993"));
        assert!(!output.contains("OLD_TRANSACTION_SECRET_993"));
        match scenario {
            0 => assert_eq!(std::fs::read(&path).unwrap(), [0xff, 0xfe]),
            1 => assert!(!path.exists()),
            _ => assert_eq!(
                std::fs::read(&path).unwrap(),
                b"# externally created\nmodel='new'\n"
            ),
        }
        assert!(!root.path().join("config.toml.lock").exists());
    }
}
