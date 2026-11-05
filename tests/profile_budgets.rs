use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    for (key, _) in std::env::vars() {
        if key.starts_with("ZI_")
            || matches!(
                key.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(key);
        }
    }
    c.current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .env("NO_PROXY", "*")
        .args([
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--timeout",
            "5",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}
fn listener() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    (l, url)
}
fn accept(l: &TcpListener) -> (TcpStream, Value) {
    let end = Instant::now() + Duration::from_secs(12);
    let stream = loop {
        match l.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut n = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                n = v.trim().parse::<usize>().unwrap();
            }
        }
    }
    assert!(n < 1_048_576);
    let mut data = vec![0; n];
    reader.read_exact(&mut data).unwrap();
    (stream, serde_json::from_slice(&data).unwrap())
}
fn send(mut s: TcpStream, api: &str) {
    let body=match api{"responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}),"anthropic"=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":"OK"}]}),_=>json!({"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]})}.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(response.as_bytes()).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
}
fn cap(body: &Value, expected: Option<(&str, u32)>) {
    for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        if let Some((f, n)) = expected {
            if f == field {
                assert_eq!(body[field], n);
                continue;
            }
        }
        assert!(body.get(field).is_none(), "unexpected budget field");
    }
}
fn preview(root: &Path, args: &[&str]) -> Value {
    let out = command(root)
        .args(["--preview", "--no-tools", "--no-stream"])
        .args(args)
        .arg("task")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["network_requests"], 0);
    value["request"]["body"].clone()
}

#[test]
fn saved_budgets_match_actual_requests_and_cli_overrides() {
    for (api, saved_field, wire) in [
        ("chat", None, "max_completion_tokens"),
        ("chat", Some("max_tokens"), "max_tokens"),
        ("responses", None, "max_output_tokens"),
        ("anthropic", None, "max_tokens"),
    ] {
        for override_limit in [None, Some("512")] {
            let root = tempfile::tempdir().unwrap();
            let (l, url) = listener();
            let mut config = format!(
                "model='fixture'\nbase_url='{url}'\napi='{api}'\napi_key='saved-key-secret'\nmax_output_tokens=128\n"
            );
            if let Some(f) = saved_field {
                config += &format!("chat_token_limit_field='{f}'\n");
            }
            std::fs::write(root.path().join("config.toml"), &config).unwrap();
            let flags: Vec<_> = override_limit.map_or(vec![], |n| vec!["--max-output-tokens", n]);
            let expected = preview(root.path(), &flags);
            cap(
                &expected,
                Some((wire, if override_limit.is_some() { 512 } else { 128 })),
            );
            assert!(l.accept().is_err());
            let protocol = api.to_owned();
            let worker = std::thread::spawn(move || {
                let (s, request) = accept(&l);
                assert_eq!(request, expected);
                send(s, &protocol);
            });
            let out = command(root.path())
                .args(["--no-tools", "--no-stream"])
                .args(&flags)
                .arg("task")
                .output()
                .unwrap();
            worker.join().unwrap();
            assert!(out.status.success());
            assert_eq!(out.stdout, b"OK\n");
            assert_eq!(
                std::fs::read_to_string(root.path().join("config.toml")).unwrap(),
                config
            );
        }
    }
}

#[test]
fn saved_budgets_are_profile_local_and_can_be_ignored_or_overridden() {
    let root = tempfile::tempdir().unwrap();
    let config = "model='root'\nbase_url='http://localhost:1/v1'\napi='chat'\nmax_output_tokens=64\nchat_token_limit_field='max_tokens'\nactive_profile='response'\n[profiles.response]\nmodel='r'\nbase_url='http://localhost:1/v1'\napi='responses'\nmax_output_tokens=32\n[profiles.plain]\nmodel='p'\nbase_url='http://localhost:1/v1'\napi='chat'\n[profiles.anthropic]\nmodel='a'\nbase_url='http://localhost:1/v1'\napi='anthropic'\n";
    std::fs::write(root.path().join("config.toml"), config).unwrap();
    for (args, model, expected) in [
        (vec![], "r", Some(("max_output_tokens", 32))),
        (
            vec!["--profile", "default"],
            "root",
            Some(("max_tokens", 64)),
        ),
        (vec!["--profile", "plain"], "p", None),
        (
            vec!["--profile", "anthropic"],
            "a",
            Some(("max_tokens", 4096)),
        ),
        (
            vec![
                "--profile",
                "default",
                "--chat-token-limit-field",
                "max_completion_tokens",
            ],
            "root",
            Some(("max_completion_tokens", 64)),
        ),
        (
            vec!["--profile", "default", "--no-profile-budget"],
            "root",
            None,
        ),
        (
            vec![
                "--profile",
                "default",
                "--no-profile-budget",
                "--max-output-tokens",
                "256",
            ],
            "root",
            Some(("max_completion_tokens", 256)),
        ),
        (
            vec![
                "--profile",
                "default",
                "--no-profile-budget",
                "--api",
                "responses",
                "--max-output-tokens",
                "256",
            ],
            "root",
            Some(("max_output_tokens", 256)),
        ),
    ] {
        let body = preview(root.path(), &args);
        assert_eq!(body["model"], model);
        cap(&body, expected);
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("config.toml")).unwrap(),
        config
    );
}

#[test]
fn invalid_saved_budgets_fail_before_input_output_and_hide_values() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("config.toml");
    let target = root.path().join("keep.txt");
    std::fs::write(&target, "keep").unwrap();
    for (api, prefs) in [
        ("chat", "max_output_tokens=0"),
        ("chat", "max_output_tokens=65537"),
        ("responses", "max_output_tokens=15"),
        (
            "chat",
            "max_output_tokens=128\nchat_token_limit_field='PRIVATE_FIELD_SECRET'",
        ),
        ("chat", "chat_token_limit_field='max_tokens'"),
        (
            "responses",
            "max_output_tokens=128\nchat_token_limit_field='max_tokens'",
        ),
    ] {
        let config = format!(
            "model='fixture'\nbase_url='http://localhost:1/v1'\napi='{api}'\napi_key='PRIVATE_KEY_SECRET'\n{prefs}\n"
        );
        std::fs::write(&file, &config).unwrap();
        let mut child = command(root.path())
            .args(["-i", "-", "-o", target.to_str().unwrap(), "--force", "task"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() > end {
                child.kill().unwrap();
                panic!("invalid saved budget read stdin");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!error.contains("PRIVATE_"));
        assert!(
            error.contains("output")
                || error.contains("chat_token_limit_field")
                || error.contains("chat-token-limit-field")
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        let ignored = preview(root.path(), &["--no-profile-budget"]);
        cap(&ignored, None);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), config);
    }
}

#[test]
fn budget_edit_is_noninteractive_atomic_and_preserves_other_settings() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("config.toml");
    let config = "model='root'\nbase_url='https://service.example/v1'\napi='chat'\napi_key='root-key-secret'\nendpoint='https://service.example/custom'\nactive_profile='work'\n[profiles.work]\nmodel='work'\nbase_url='https://service.example/v1'\napi='responses'\napi_key_env='CUSTOM_KEY'\nmax_output_tokens=32\n[profiles.other]\nmodel='other'\napi_key='other-key-secret'\nmax_output_tokens=64\n";
    std::fs::write(&file, config).unwrap();
    let original: toml::Value = toml::from_str(config).unwrap();
    let out = command(root.path())
        .args([
            "config",
            "--name",
            "default",
            "--max-output-tokens",
            "256",
            "--chat-token-limit-field",
            "max_tokens",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("secret"));
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["active_profile"].as_str(), Some("work"));
    assert_eq!(saved["profiles"], original["profiles"]);
    assert_eq!(saved["api_key"], original["api_key"]);
    assert_eq!(saved["endpoint"], original["endpoint"]);
    assert_eq!(saved["max_output_tokens"].as_integer(), Some(256));
    let mut expected = original.clone();
    expected
        .as_table_mut()
        .unwrap()
        .insert("max_output_tokens".into(), toml::Value::Integer(256));
    expected.as_table_mut().unwrap().insert(
        "chat_token_limit_field".into(),
        toml::Value::String("max_tokens".into()),
    );
    assert_eq!(saved, expected);
    assert_eq!(saved["chat_token_limit_field"].as_str(), Some("max_tokens"));
    let previous = std::fs::read(&file).unwrap();
    for args in [
        vec!["config", "--name", "work", "--max-output-tokens", "8"],
        vec!["config", "--name", "missing", "--max-output-tokens", "128"],
        vec!["config", "--manual", "--max-output-tokens", "128"],
        vec![
            "config",
            "--clear-output-budget",
            "--max-output-tokens",
            "128",
        ],
    ] {
        let out = command(root.path()).args(args).output().unwrap();
        assert!(!out.status.success());
        assert_eq!(std::fs::read(&file).unwrap(), previous);
        assert!(!String::from_utf8_lossy(&out.stderr).contains("secret"));
    }
    let out = command(root.path())
        .args(["config", "--name", "default", "--clear-output-budget"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let cleared: toml::Value = toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(cleared.get("max_output_tokens").is_none());
    assert!(cleared.get("chat_token_limit_field").is_none());
    assert_eq!(cleared["profiles"], original["profiles"]);
    assert_eq!(cleared["active_profile"].as_str(), Some("work"));
    assert_eq!(cleared, original);
}

#[test]
fn doctor_shows_effective_saved_budget_and_probe_uses_fixed_cap() {
    for field in [None, Some("max_tokens"), Some("max_completion_tokens")] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        let mut config = format!("model='fixture'\nbase_url='{url}'\nmax_output_tokens=256\n");
        if let Some(f) = field {
            config += &format!("chat_token_limit_field='{f}'\n");
        }
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        let out = command(root.path()).arg("doctor").output().unwrap();
        assert!(out.status.success());
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(report.contains("Output cap: 256 tokens per turn (source: profile)"));
        assert!(report.contains(field.unwrap_or("max_completion_tokens")));
        assert!(l.accept().is_err());
        let wire = field.unwrap_or("max_completion_tokens");
        let worker = std::thread::spawn(move || {
            let (s, request) = accept(&l);
            cap(&request, Some((wire, 16)));
            send(s, "chat");
        });
        let out = command(root.path())
            .args(["doctor", "--probe"])
            .output()
            .unwrap();
        worker.join().unwrap();
        assert!(out.status.success());
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(report.contains("Probe cap: 16 tokens per turn"));
    }
}

#[test]
fn invalid_budget_selection_does_not_persist_and_clear_can_repair_preferences() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let original = "model='root'\nbase_url='http://localhost:1/v1'\n[profiles.bad]\nmodel='bad'\nbase_url='http://localhost:1/v1'\napi='responses'\nmax_output_tokens=8\n";
    std::fs::write(&path, original).unwrap();
    let out = command(root.path()).args(["use", "bad"]).output().unwrap();
    assert!(!out.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let out = command(root.path())
        .args(["config", "--name", "bad", "--clear-output-budget"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = command(root.path()).args(["use", "bad"]).output().unwrap();
    assert!(out.status.success());
    let body = preview(root.path(), &[]);
    cap(&body, None);
    assert_eq!(body["model"], "bad");
}

#[test]
fn saved_field_without_cap_cannot_borrow_doctor_probe_exception() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    std::fs::write(
        root.path().join("config.toml"),
        format!(
            "model='fixture'\nbase_url='{url}'\napi='chat'\nchat_token_limit_field='max_tokens'\n"
        ),
    )
    .unwrap();
    let out = command(root.path())
        .args(["doctor", "--probe"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("requires --max-output-tokens"));
    assert!(
        l.accept().is_err(),
        "invalid saved preferences triggered a probe request"
    );
}

#[test]
fn clear_budget_does_not_require_a_model_or_supported_protocol() {
    for root_api in [None, Some("chat"), Some("obsolete")] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let api = root_api
            .map(|api| format!("api='{api}'\n"))
            .unwrap_or_default();
        let original = format!(
            "{api}api_key='ROOT_CLEANUP_SECRET_993'\nmax_output_tokens=0\nchat_token_limit_field='unsupported'\nactive_profile='ready'\n[profiles.unconfigured]\napi='obsolete'\napi_key_env='MISSING_CLEANUP_REF_993'\nmax_output_tokens=0\nchat_token_limit_field='unsupported'\n[profiles.ready]\nmodel='ready'\nbase_url='http://localhost:1/v1'\napi_key='READY_CLEANUP_SECRET_993'\nmax_output_tokens=64\n"
        );
        std::fs::write(&path, &original).unwrap();
        let mut expected: toml::Value = toml::from_str(&original).unwrap();
        // Valid named selection proves that the full typed document parses and
        // that unrelated root preferences do not affect this active profile.
        let report = command(root.path())
            .args(["doctor", "--json"])
            .output()
            .unwrap();
        assert!(report.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&report.stdout).unwrap()["configuration"]["details"]["profile"],
            "ready"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        for name in ["default", "unconfigured"] {
            let before = std::fs::read(&path).unwrap();
            // Setting new preferences still needs a model; clearing does not.
            let rejected = command(root.path())
                .args(["config", "--name", name, "--max-output-tokens", "128"])
                .output()
                .unwrap();
            assert!(!rejected.status.success());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let out = command(root.path())
                .args(["config", "--name", name, "--clear-output-budget"])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stdout.is_empty());
            for secret in ["ROOT_CLEANUP_SECRET_993", "READY_CLEANUP_SECRET_993"] {
                assert!(!String::from_utf8_lossy(&out.stderr).contains(secret));
            }
            let profile = if name == "default" {
                &mut expected
            } else {
                &mut expected["profiles"][name]
            };
            profile.as_table_mut().unwrap().remove("max_output_tokens");
            profile
                .as_table_mut()
                .unwrap()
                .remove("chat_token_limit_field");
            let actual: toml::Value =
                toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(actual, expected);
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
