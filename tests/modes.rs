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
        if name.to_uppercase().starts_with("ZI_")
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
        .env("ZI_CONFIG", root.join("invalid.toml"))
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
fn template_mode_conflicts_fail_before_input_or_output_publication() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let original = b"{\"kept\":true}\r\n";
    std::fs::write(root.join("keep.json"), original).unwrap();
    for args in [
        vec!["--config-example", "--json"],
        vec!["--json", "--config-example"],
        vec!["--config-example", "--count"],
        vec!["--count", "--config-example"],
        vec!["--config-example", "--select", "/n"],
        vec!["--select", "/n", "--config-example"],
        vec!["--config-example", "-i", "missing-data"],
        vec!["-i", "missing-data", "--config-example"],
        vec!["--config-example", "an instruction"],
    ] {
        let mut args = args;
        args.extend(["-o", "keep.json", "--force"]);
        let out = invoke(root, &args, b"PRIVATE_INPUT\xff");
        assert!(!out.status.success(), "ignored conflicting modes: {args:?}");
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("config-example") && !error.contains("PRIVATE_INPUT"),
            "{error}"
        );
        assert_eq!(std::fs::read(root.join("keep.json")).unwrap(), original);
        assert_eq!(
            std::fs::read_dir(root).unwrap().count(),
            1,
            "output was prepared before rejection"
        );
    }
}

#[test]
fn project_selectors_are_rejected_for_every_nonmodel_mode_even_with_no_tools() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("invalid.toml"), "PRIVATE_INVALID_CONFIG").unwrap();
    std::fs::write(root.join("invalid-presets.toml"), "PRIVATE_INVALID_PRESETS").unwrap();
    let modes = [
        vec!["--count"],
        vec!["--select", "/n"],
        vec!["--config-example"],
        vec!["data", "trim"],
        vec!["data", "--show-plan", "--step", "trim"],
        vec!["models"],
        vec!["use", "other"],
        vec!["doctor"],
        vec!["doctor", "--probe"],
        vec!["config", "--list"],
        vec!["config", "--manual"],
        vec!["presets", "list"],
        vec!["presets", "render", "translate"],
        vec!["presets", "export", "translate", "--name", "team-translate"],
        vec!["presets", "init"],
    ];
    for project in [
        vec!["--agent"],
        vec!["-C", "missing-project"],
        vec!["--read-only"],
        vec!["--no-exec"],
    ] {
        for mode in &modes {
            for no_tools in [false, true] {
                let mut args = project.clone();
                if no_tools {
                    args.push("--no-tools");
                }
                args.extend(mode);
                let out = invoke(root, &args, b"PRIVATE_DATA");
                assert!(!out.status.success(), "{args:?}");
                assert!(out.stdout.is_empty());
                let error = String::from_utf8_lossy(&out.stderr);
                assert!(
                    error.contains("project options") && error.contains("model tasks"),
                    "{args:?}: {error}"
                );
                assert!(!error.contains("PRIVATE"));
                assert_eq!(
                    std::fs::read_dir(root).unwrap().count(),
                    2,
                    "unexpected file changes: {args:?}"
                );
            }
        }
    }
}

#[test]
fn invalid_mode_combinations_finish_with_open_stdin_and_never_probe_a_model() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    for args in [
        vec!["--config-example", "--json"],
        vec!["--agent", "--count"],
        vec!["-C", "missing-project", "doctor", "--probe"],
        vec!["--no-tools", "--agent", "presets", "init"],
    ] {
        let mut child = command(root)
            .args(&args)
            .env("ZI_BASE_URL", &url)
            .env("ZI_MODEL", "fixture")
            .env("ZI_API_KEY", "PRIVATE_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() > end {
                child.kill().ok();
                panic!("invalid modes waited for stdin: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_KEY"));
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn valid_offline_modes_keep_json_and_export_behavior_with_inherited_model_settings() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("invalid.toml"), "PRIVATE_INVALID_CONFIG").unwrap();
    std::fs::write(root.join("presets.toml"), "PRIVATE_INVALID_PRESETS").unwrap();
    let run = |args: &[&str], input: &[u8]| {
        let mut child = command(root)
            .args(args)
            .env("ZI_MODEL", "PRIVATE_ENV_MODEL")
            .env("ZI_BASE_URL", "http://127.0.0.1:9/v1")
            .env("ZI_API", "anthropic")
            .env("ZI_PROFILE", "missing-profile")
            .env("ZI_PRESETS", root.join("presets.toml"))
            .env("ZI_API_KEY", "PRIVATE_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).ok();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE"));
        out
    };
    let out = run(
        &["--count", "--json", "--no-tools", "--no-stream"],
        "你好\n".as_bytes(),
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["characters"],
        3
    );
    let out = run(
        &["--select", "/n", "--json"],
        b"{\"n\":123456789012345678901234567890}",
    );
    assert_eq!(out.stdout, b"123456789012345678901234567890\n");
    let out = run(&["--config-example"], b"\xff");
    let text = std::str::from_utf8(&out.stdout).unwrap();
    assert!(text.starts_with("# Save as"));
    let _: toml::Value = toml::from_str(text).unwrap();
    let out = run(&["--config-example", "-o", "example.toml"], b"\xff");
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read(root.join("example.toml")).unwrap(),
        text.as_bytes()
    );
    let out = run(&["data", "trim"], b" a \n");
    assert_eq!(out.stdout, b"a\n");
    let out = run(&["data", "--show-plan", "--step", "trim"], b"\xff");
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
    assert_eq!(parsed["version"].as_integer(), Some(1));
    assert!(!root.join("config").exists());
}

#[test]
fn model_tasks_still_allow_project_selectors_with_no_tools_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let protocol = api.to_owned();
        let server = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "missing model request");
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
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        length = v.trim().parse().unwrap();
                    }
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(request.get("tools").is_none());
            let payload=match protocol.as_str(){"chat"=>json!({"choices":[{"finish_reason":"stop","message":{"content":"verified"}}]}),"responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"verified"}]}]}),_=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":"verified"}]}),}.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
        });
        let cfg = root.join("model.toml");
        std::fs::write(
            &cfg,
            format!("model='fixture'\nbase_url='{url}'\napi='{api}'\n"),
        )
        .unwrap();
        let mut child = command(root)
            .env("ZI_CONFIG", &cfg)
            .args([
                "--agent",
                "-C",
                "missing-project",
                "--read-only",
                "--no-exec",
                "--no-tools",
                "task",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"input").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"verified\n");
        assert!(out.stderr.is_empty());
        server.join().unwrap();
        assert!(!root.join("missing-project").exists());
    }
}

#[test]
fn help_hides_environment_values_on_root_and_inherited_help_paths() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["help"],
        vec!["config", "--help"],
        vec!["models", "--help"],
        vec!["doctor", "--help"],
        vec!["use", "--help"],
        vec!["data", "--help"],
        vec!["presets", "--help"],
        vec!["presets", "import", "--help"],
        vec!["presets", "render", "--help"],
        vec!["help", "config"],
        vec!["help", "data"],
        vec!["help", "presets", "import"],
    ] {
        let output = command(root.path())
            .env(
                "ZI_BASE_URL",
                "https://user:HELP_URL_SECRET_993@service.example/v1?token=HELP_QUERY_SECRET_993",
            )
            .env("ZI_MODEL", "HELP_MODEL_VALUE_993-中\x1b[31mcolor\x1b[0m")
            .env("ZI_API", "chat")
            .env("ZI_CONFIG", root.path().join("HELP_CONFIG_VALUE_993.toml"))
            .env("ZI_PROFILE", "HELP_PROFILE_VALUE_993")
            .env(
                "ZI_PRESETS",
                root.path().join("HELP_PRESETS_VALUE_993.toml"),
            )
            .env("NO_COLOR", "1")
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "help failed: {args:?}");
        assert!(
            output.stderr.is_empty(),
            "help wrote unexpected stderr: {args:?}"
        );
        let text = String::from_utf8(output.stdout).unwrap();
        for marker in [
            "HELP_URL_SECRET_993",
            "HELP_QUERY_SECRET_993",
            "HELP_MODEL_VALUE_993",
            "HELP_CONFIG_VALUE_993",
            "HELP_PROFILE_VALUE_993",
            "HELP_PRESETS_VALUE_993",
        ] {
            assert!(
                !text.contains(marker),
                "help echoed an environment value: {args:?}"
            );
        }
        let root_help = args.len() == 1;
        for name in if root_help {
            &[
                "ZI_BASE_URL",
                "ZI_MODEL",
                "ZI_API",
                "ZI_CONFIG",
                "ZI_PROFILE",
                "ZI_PRESETS",
            ][..]
        } else {
            &["ZI_CONFIG", "ZI_PROFILE", "ZI_PRESETS"][..]
        } {
            assert!(
                text.contains(&format!("[env: {name}]")),
                "environment name missing from help: {name}, {args:?}"
            );
        }
        assert!(!text.contains("ZI_API=chat"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn hiding_help_values_keeps_effective_environment_settings_for_model_preview() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let original = b"model='saved-root'\nbase_url='http://localhost:2/v1'\napi='responses'\n[profiles.work]\nmodel='saved-work'\nbase_url='http://localhost:3/v1'\napi='responses'\n";
    std::fs::write(&path, original).unwrap();
    let output = command(root.path())
        .env("ZI_CONFIG", &path)
        .env("ZI_PROFILE", "work")
        .env("ZI_MODEL", "selected-env-model")
        .env("ZI_API", "chat")
        .env("ZI_BASE_URL", "http://127.0.0.1:1/v1")
        .env("ZI_API_KEY", "HELP_EFFECTIVE_KEY_993")
        .args(["--preview", "--no-stream", "--no-tools", "Explain pipes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "preview failed after help-only settings change"
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["request"]["body"]["model"], "selected-env-model");
    assert_eq!(
        value["request"]["url"],
        "http://127.0.0.1:1/v1/chat/completions"
    );
    assert_eq!(value["network_requests"], 0);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("HELP_EFFECTIVE_KEY_993"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
