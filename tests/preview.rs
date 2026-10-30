use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    c.current_dir(root);
    for (name, _) in std::env::vars() {
        if name.starts_with("ZI_")
            || matches!(
                name.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(name);
        }
    }
    c.env("HOME", root)
        .env("USERPROFILE", root)
        .env("APPDATA", root.join("config"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("NO_PROXY", "*");
    c
}

fn invoke(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

#[test]
fn preview_matches_actual_first_request_in_all_protocols_and_task_modes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("AGENTS.md"),
        "PROJECT_PREVIEW_INSTRUCTIONS: preserve unrelated files.",
    )
    .unwrap();
    std::fs::write(root.path().join("keep.txt"), "unchanged").unwrap();
    let input = "合成输入\r\n\u{1b}[31mDO_NOT_EXECUTE\u{009b}31m\u{007f}\n".as_bytes();
    let modes: &[&[&str]] = &[
        &[],
        &["--no-tools"],
        &["--json"],
        &["--stream"],
        &["--exact"],
        &["--exact", "--export-plan"],
        &["--read-only"],
        &["--no-exec"],
        &["--agent"],
        &["--agent", "--no-tools"],
        &["-s", "translate", "--var", "language=French"],
        &["-s", "extract"],
    ];
    for api in ["chat", "responses", "anthropic"] {
        for mode in modes {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}/v1", listener.local_addr().unwrap());
            let mut args = vec![
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "--timeout",
                "2",
                "--max-steps",
                "1",
            ];
            args.extend_from_slice(mode);
            let mut preview_args = args.clone();
            preview_args.extend(["--preview", "task"]);
            let mut child = command(root.path())
                .args(preview_args)
                .env("ZI_API_KEY", "PREVIEW_AUTH_SECRET_991")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "{api}/{mode:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stderr.is_empty());
            let preview: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(preview["kind"], "first-model-request-preview");
            assert_eq!(preview["network_requests"], 0);
            assert_eq!(preview["max_model_turns"], 1);
            assert_eq!(preview["authentication"]["configured"], true);
            assert_eq!(
                preview["authentication"]["credential_headers_omitted"],
                true
            );
            assert!(!String::from_utf8_lossy(&out.stdout).contains("PREVIEW_AUTH_SECRET_991"));
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock,
                "preview made a network request"
            );
            let worker = std::thread::spawn(move || {
                let end = Instant::now() + Duration::from_secs(10);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(value) => break value,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < end =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("actual request missing: {e}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut length = 0;
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                    headers.push_str(&line);
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                stream
                    .write_all(
                        b"HTTP/1.1 418 Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
                (body, headers, request_line)
            });
            args.push("task");
            let mut child = command(root.path())
                .args(args)
                .env("ZI_API_KEY", "PREVIEW_AUTH_SECRET_991")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let actual = child.wait_with_output().unwrap();
            assert!(!actual.status.success()); // Fixture rejects after capturing the actual request.
            let (body, headers, request_line) = worker.join().unwrap();
            assert_eq!(preview["request"]["body"], body, "{api}/{mode:?}");
            assert!(headers.contains("PREVIEW_AUTH_SECRET_991"));
            let path = match api {
                "chat" => "/v1/chat/completions",
                "responses" => "/v1/responses",
                _ => "/v1/messages",
            };
            assert_eq!(
                preview["request"]["url"],
                format!(
                    "http://{}{path}",
                    url.trim_start_matches("http://").trim_end_matches("/v1")
                )
            );
            assert!(request_line.starts_with(&format!("POST {path} ")));
            assert_eq!(
                std::fs::read_to_string(root.path().join("keep.txt")).unwrap(),
                "unchanged"
            );
        }
    }
    assert!(!root.path().join("config").exists());
}

#[test]
fn preview_resolves_profile_exact_endpoint_and_literal_preset_without_exposing_keys() {
    let root = tempfile::tempdir().unwrap();
    let config = "active_profile='work'\n[profiles.work]\nmodel='selected-model'\nbase_url='https://service.example/v1'\nendpoint='https://service.example/private/messages'\napi='anthropic'\napi_key='CONFIG_AUTH_SECRET_881'\n";
    std::fs::write(root.path().join("model.toml"), config).unwrap();
    std::fs::write(root.path().join("presets.toml"), "version=1\n[presets.team]\ndescription='fixture'\nprompt='For {{who}}, summarize.'\njson=true\n[presets.team.defaults]\nwho='default'\n").unwrap();
    let out = invoke(
        root.path(),
        &[
            "--preview",
            "--config",
            "model.toml",
            "--presets",
            "presets.toml",
            "-s",
            "team",
            "--var",
            "who=literal{{name}}",
            "extra task",
        ],
        b"data",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let p: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        p["request"]["url"],
        "https://service.example/private/messages"
    );
    assert_eq!(p["request"]["body"]["model"], "selected-model");
    let body = p["request"]["body"].to_string();
    assert!(body.contains("For literal{{name}}, summarize.") && body.contains("extra task"));
    assert!(body.contains("exactly one valid JSON"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("CONFIG_AUTH_SECRET_881"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("model.toml")).unwrap(),
        config
    );
    let overridden = invoke(
        root.path(),
        &[
            "--preview",
            "--config",
            "model.toml",
            "--api",
            "responses",
            "task",
        ],
        b"",
    );
    assert!(overridden.status.success());
    let p: Value = serde_json::from_slice(&overridden.stdout).unwrap();
    assert_eq!(p["request"]["url"], "https://service.example/v1/responses");
    assert_eq!(p["request"]["body"]["store"], false);
}

#[test]
fn preview_can_inspect_without_authentication_but_actual_public_request_still_requires_key() {
    let root = tempfile::tempdir().unwrap();
    for (api, url) in [
        ("chat", "https://api.openai.com/v1"),
        ("anthropic", "https://api.anthropic.com"),
    ] {
        let out = invoke(
            root.path(),
            &[
                "--preview",
                "--api",
                api,
                "--base-url",
                url,
                "--model",
                "fixture",
                "task",
            ],
            b"data",
        );
        assert!(out.status.success());
        let p: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(p["authentication"]["configured"], false);
        let actual = invoke(
            root.path(),
            &[
                "--api",
                api,
                "--base-url",
                url,
                "--model",
                "fixture",
                "task",
            ],
            b"data",
        );
        assert!(!actual.status.success() && actual.stdout.is_empty());
        assert!(String::from_utf8_lossy(&actual.stderr).contains("set ZI_API_KEY"));
    }
    let missing = invoke(root.path(), &["--preview", "task"], b"data");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--model"));
    assert!(!root.path().join("config").exists());
}

#[test]
fn preview_mode_conflicts_and_empty_task_fail_before_io_or_configuration() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["--preview", "data", "trim"],
        vec!["--preview", "doctor"],
        vec!["--preview", "config"],
        vec!["--preview", "presets"],
        vec!["--preview", "--count"],
        vec!["--preview", "--select", "/x"],
        vec!["--preview", "--config-example"],
        vec!["--preview"],
    ] {
        let mut args = args;
        args.extend(["-i", "missing-input"]);
        let out = invoke(root.path(), &args, b"PRIVATE_INPUT");
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            !error.contains("cannot open") && !error.contains("PRIVATE_INPUT"),
            "{error}"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn preview_file_output_is_json_and_failures_preserve_existing_result() {
    let root = tempfile::tempdir().unwrap();
    let common = ["--preview", "--model", "fixture", "-o", "request.json"];
    let out = invoke(
        root.path(),
        &[common.to_vec(), vec!["task"]].concat(),
        "original input\u{009b}\u{007f}".as_bytes(),
    );
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    let saved = std::fs::read(root.path().join("request.json")).unwrap();
    let p: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(p["request"]["body"]["stream"], false);
    assert_eq!(p["version"], 1);
    let wire = p["request"]["body"]["messages"][1]["content"]
        .as_str()
        .unwrap();
    assert!(wire.contains('\u{009b}') && wire.contains('\u{007f}'));
    assert!(String::from_utf8_lossy(&saved).contains('\u{009b}'));
    let again = invoke(
        root.path(),
        &[common.to_vec(), vec!["task"]].concat(),
        b"changed",
    );
    assert!(!again.status.success() && again.stdout.is_empty());
    for input in [vec![0xff], vec![0; 3 * 1024 * 1024]] {
        let failed = invoke(
            root.path(),
            &[common.to_vec(), vec!["--force", "task"]].concat(),
            &input,
        );
        assert!(!failed.status.success() && failed.stdout.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("request.json")).unwrap(),
            saved
        );
    }
    let replaced = invoke(
        root.path(),
        &[common.to_vec(), vec!["--force", "--no-tools", "task"]].concat(),
        b"changed",
    );
    assert!(replaced.status.success());
    let p: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("request.json")).unwrap()).unwrap();
    assert!(p["request"]["body"].get("tools").is_none());
}

#[test]
fn preview_project_context_is_explicit_and_does_not_expand_permissions_or_execute_tools() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "SCOPED_PREVIEW_ONLY").unwrap();
    std::fs::write(root.path().join("private.txt"), "DO_NOT_AUTO_INCLUDE_FILE").unwrap();
    for flags in [
        vec![],
        vec!["--read-only"],
        vec!["--no-exec"],
        vec!["--agent", "--no-tools"],
    ] {
        let out = invoke(
            root.path(),
            &[
                vec!["--preview", "--model", "fixture"],
                flags.clone(),
                vec!["task"],
            ]
            .concat(),
            b"",
        );
        assert!(out.status.success());
        let p: Value = serde_json::from_slice(&out.stdout).unwrap();
        let enabled = flags.contains(&"--read-only") || flags.contains(&"--no-exec");
        assert_eq!(p["project_tools_enabled"], enabled);
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.contains("SCOPED_PREVIEW_ONLY"), enabled);
        assert!(!text.contains("DO_NOT_AUTO_INCLUDE_FILE"));
        let tools = p["request"]["body"]["tools"].as_array();
        let names: Vec<_> = tools
            .into_iter()
            .flatten()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(!names.contains(&"run_command"));
        if flags.contains(&"--read-only") {
            assert!(!names.contains(&"write_file"));
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("private.txt")).unwrap(),
            "DO_NOT_AUTO_INCLUDE_FILE"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    // In exact export mode the inspected tool is restricted to proposing transforms.
    let out = invoke(
        root.path(),
        &[
            "--preview",
            "--model",
            "fixture",
            "--exact",
            "--export-plan",
            "task",
        ],
        b"data",
    );
    let p: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(p["request"]["body"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        p["request"]["body"]["tools"][0]["function"]["name"],
        json!("transform_input")
    );
}
