use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn command(root: &Path) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    c.current_dir(root);
    for (k, _) in std::env::vars() {
        if k.starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(k);
        }
    }
    c.env("APPDATA", root.join("config"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("NO_PROXY", "*");
    c
}
fn run(root: &Path, args: &[&str], data: &[u8]) -> Output {
    let mut child = command(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(data) {
        // Invalid definitions may be rejected before the child consumes any stdin.
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}
fn model(api: &str, text: &str) -> (String, std::thread::JoinHandle<Value>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let api = api.to_owned();
    let answer = text.to_owned();
    let task = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut s = loop {
            match l.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "request missing");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        s.set_nonblocking(false).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap();
                }
            }
        }
        let mut bytes = vec![0; len];
        r.read_exact(&mut bytes).unwrap();
        let req = serde_json::from_slice(&bytes).unwrap();
        let response=match api.as_str(){"responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":answer}]}]}),"anthropic"=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":answer}]}),_=>json!({"choices":[{"finish_reason":"stop","message":{"content":answer}}]})}.to_string();
        write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        req
    });
    (url, task)
}

#[test]
fn offline_catalog_init_render_and_no_clobber() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let out = run(root, &["presets"], b"ignored pipe input");
    assert!(out.status.success());
    let list = String::from_utf8(out.stdout).unwrap();
    for name in [
        "summarize",
        "translate",
        "extract",
        "explain",
        "review",
        "commit",
        "rewrite",
    ] {
        assert!(list.contains(name));
    }
    assert!(out.stderr.is_empty());
    let out = run(root, &["--presets", "custom.toml", "presets", "init"], b"");
    assert!(out.status.success());
    let before = std::fs::read(root.join("custom.toml")).unwrap();
    let out = run(root, &["--presets", "custom.toml", "presets", "init"], b"");
    assert!(!out.status.success());
    assert_eq!(std::fs::read(root.join("custom.toml")).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.join("custom.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let out = run(
        root,
        &[
            "--presets",
            "custom.toml",
            "presets",
            "render",
            "team-summary",
            "--var",
            "audience=研发团队",
        ],
        b"",
    );
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("研发团队"));
    assert!(!root.join("config/zistral/config.toml").exists());
}

#[test]
fn exported_builtin_roundtrips_as_editable_toml_with_literal_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("invalid-model.toml"), "PRIVATE_INVALID_MODEL").unwrap();
    let value = "研发 {{not_expanded}}\nline=two";
    let variable = format!("language={value}");
    let args = [
        "--config",
        "invalid-model.toml",
        "presets",
        "export",
        "translate",
        "--name",
        "team-translate",
        "--var",
        &variable,
    ];
    let out = run(root, &args, b"PRIVATE_INPUT\xff");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let definition: toml::Value =
        toml::from_str(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
    assert_eq!(definition["version"].as_integer(), Some(1));
    let presets = definition["presets"].as_table().unwrap();
    assert_eq!(presets.len(), 1);
    assert!(
        presets["team-translate"]["prompt"]
            .as_str()
            .unwrap()
            .contains("{{language}}")
    );
    assert_eq!(
        presets["team-translate"]["defaults"]["language"].as_str(),
        Some(value)
    );
    assert_eq!(presets["team-translate"]["json"].as_bool(), Some(false));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE"));
    std::fs::write(root.join("copy.toml"), &out.stdout).unwrap();
    let rendered = run(
        root,
        &[
            "--presets",
            "copy.toml",
            "presets",
            "render",
            "team-translate",
        ],
        b"ignored",
    );
    assert!(rendered.status.success());
    assert!(String::from_utf8_lossy(&rendered.stdout).contains(value));
    let override_out = run(
        root,
        &[
            "--presets",
            "copy.toml",
            "presets",
            "render",
            "team-translate",
            "--var",
            "language=French",
        ],
        b"",
    );
    assert!(override_out.status.success());
    assert!(String::from_utf8_lossy(&override_out.stdout).contains("French"));
    let stdout_out = run(
        root,
        &[
            "presets",
            "export",
            "translate",
            "--name",
            "other",
            "-o",
            "-",
        ],
        b"",
    );
    assert!(stdout_out.status.success());
    assert!(
        toml::from_str::<toml::Value>(std::str::from_utf8(&stdout_out.stdout).unwrap()).is_ok()
    );
    assert!(!root.join("config").exists());
}

#[test]
fn exported_presets_use_success_only_file_saving_without_merging_or_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = b"version=1\n[presets.custom]\ndescription='For a team'\nprompt='For {{team}}: {{field}}'\njson=true\n[presets.custom.defaults]\nteam='support'\n[presets.unrelated]\ndescription='Other'\nprompt='Other prompt'\n";
    std::fs::write(root.join("source.toml"), source).unwrap();
    let out = run(
        root,
        &[
            "--presets",
            "source.toml",
            "presets",
            "export",
            "custom",
            "--name",
            "copy",
            "-o",
            "copy.toml",
        ],
        b"not read",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let before = std::fs::read(root.join("copy.toml")).unwrap();
    let exported: toml::Value = toml::from_str(std::str::from_utf8(&before).unwrap()).unwrap();
    assert_eq!(exported["presets"].as_table().unwrap().len(), 1);
    assert!(exported["presets"]["copy"]["json"].as_bool().unwrap());
    let missing = run(
        root,
        &["--presets", "copy.toml", "presets", "render", "copy"],
        b"",
    );
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing variable field"));
    let args = [
        "presets",
        "export",
        "translate",
        "--name",
        "localized",
        "--var",
        "language=Japanese",
        "-o",
        "copy.toml",
    ];
    let out = run(root, &args, b"");
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("copy.toml")).unwrap(), before);
    let mut force = args.to_vec();
    force.push("--force");
    let out = run(root, &force, b"");
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    let after = std::fs::read(root.join("copy.toml")).unwrap();
    assert_ne!(after, before);
    assert_eq!(std::fs::read(root.join("source.toml")).unwrap(), source);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.join("copy.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn exported_json_presets_reuse_in_all_protocols_without_permission_or_input_leaks() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let out = run(
            root,
            &[
                "presets",
                "export",
                "extract",
                "--name",
                "ticket",
                "-o",
                "ticket.toml",
            ],
            b"PRIVATE_INPUT",
        );
        assert!(out.status.success());
        let definition = std::fs::read(root.join("ticket.toml")).unwrap();
        assert!(!String::from_utf8_lossy(&definition).contains("PRIVATE_INPUT"));
        let (url, server) = model(api, r#"{"ok":true}"#);
        let out = run(
            root,
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "--presets",
                "ticket.toml",
                "-s",
                "ticket",
                "requested fields",
            ],
            b"PRIVATE_INPUT {{not_expanded}}",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"{\"ok\":true}\n");
        assert!(out.stderr.is_empty());
        let request = server.join().unwrap();
        assert_eq!(request["stream"], false);
        assert_eq!(request["tools"].as_array().unwrap().len(), 3);
        let key = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        let messages = request[key].as_array().unwrap();
        let prompt = messages.last().unwrap()["content"].as_str().unwrap();
        assert!(prompt.contains("requested fields") && !prompt.contains("PRIVATE_INPUT"));
        assert!(messages.iter().any(|m| {
            m["content"]
                .as_str()
                .is_some_and(|s| s.contains("PRIVATE_INPUT {{not_expanded}}"))
        }));
        assert_eq!(std::fs::read(root.join("ticket.toml")).unwrap(), definition);
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
}

#[test]
fn export_validation_finishes_with_open_stdin_and_never_probes_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.toml"), b"original").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    for mut args in [
        vec!["presets", "export", "translate", "--name", "copy"],
        vec!["presets", "export", "translate", "--name", "translate"],
        vec!["presets", "export", "missing", "--name", "copy"],
        vec!["presets", "export", "translate", "--name", "bad/name"],
        vec![
            "presets",
            "export",
            "translate",
            "--name",
            "copy",
            "--var",
            "unused=PRIVATE_VALUE",
        ],
        vec![
            "presets",
            "export",
            "translate",
            "--name",
            "copy",
            "--var",
            "language=French",
            "--var",
            "language=Japanese",
        ],
        vec![
            "presets",
            "export",
            "translate",
            "--name",
            "copy",
            "-i",
            "missing.txt",
        ],
    ] {
        let valid = args == ["presets", "export", "translate", "--name", "copy"];
        args.extend(["-o", "keep.toml", "--force"]);
        let mut child = command(root)
            .args(["--base-url", &url, "--model", "fixture"])
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                child.kill().ok();
                panic!("export waited for stdin");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert_eq!(
            out.status.success(),
            valid,
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_VALUE"));
        if valid {
            std::fs::write(root.join("keep.toml"), b"original").unwrap();
        }
        assert_eq!(std::fs::read(root.join("keep.toml")).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
        assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    }
    for action in [
        vec!["presets", "list"],
        vec!["presets", "show", "translate"],
        vec!["presets", "render", "translate"],
        vec!["presets", "init"],
    ] {
        let mut args = action;
        args.extend(["-o", "keep.toml", "--force"]);
        let out = run(root, &args, b"");
        assert!(!out.status.success());
        assert_eq!(std::fs::read(root.join("keep.toml")).unwrap(), b"original");
    }
}

#[test]
fn builtin_translation_preserves_input_and_permissions_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let (url, task) = model(api, "Bonjour");
        let out = run(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "-s",
                "translate",
                "--var",
                "language=French",
                "Keep identifiers",
            ],
            "Hello 世界\n{{not_a_variable_in_data}}".as_bytes(),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"Bonjour\n");
        assert!(out.stderr.is_empty());
        let req = task.join().unwrap();
        assert_eq!(req["tools"].as_array().unwrap().len(), 3);
        let key = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        let msgs = req[key].as_array().unwrap();
        let prompt = msgs.last().unwrap()["content"].as_str().unwrap();
        assert!(prompt.contains("French"));
        assert!(prompt.contains("Keep identifiers"));
        assert!(!prompt.contains("Hello 世界"));
        assert!(msgs.iter().any(|m| {
            m["content"]
                .as_str()
                .is_some_and(|s| s.contains("Hello 世界\n{{not_a_variable_in_data}}"))
        }));
    }
}

#[test]
fn custom_preset_and_extraction_keep_json_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("presets.toml"),"version=1\n[presets.ticket]\ndescription='Ticket summary'\nprompt='Summarize for {{team}}'\njson=true\n[presets.ticket.defaults]\nteam='support'\n").unwrap();
    for (name, answer, ok) in [
        ("ticket", r#"{"issue":"示例"}"#, true),
        ("extract", "partial {", false),
    ] {
        let (url, task) = model("chat", answer);
        let out = run(
            root,
            &[
                "--presets",
                "presets.toml",
                "--base-url",
                &url,
                "--model",
                "fixture",
                "-s",
                name,
            ],
            b"input ticket",
        );
        assert_eq!(out.status.success(), ok);
        if ok {
            assert_eq!(
                serde_json::from_slice::<Value>(&out.stdout).unwrap()["issue"],
                "示例"
            );
        } else {
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains("not valid JSON"));
        }
        let req = task.join().unwrap();
        assert_eq!(req["stream"], false);
        if name == "ticket" {
            assert!(
                req["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap()
                    .contains("Summarize for support")
            );
        }
    }
    let out = run(root, &["-s", "extract", "--stream"], b"data");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn malformed_templates_fail_before_network_and_do_not_leak_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for body in [
        "version=1\n[presets.summarize]\ndescription='conflict'\nprompt='test'\n",
        "version=1\n[presets.custom]\ndescription='test'\nprompt='{{input}}'\n",
        "version=1\n[presets.custom]\ndescription='test'\nprompt='test'\napi_key='never-print-this-example'\n",
        "version=99\n[presets]\n",
    ] {
        std::fs::write(root.join("custom.toml"), body).unwrap();
        let out = run(
            root,
            &["--presets", "custom.toml", "-s", "summarize"],
            b"input",
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("never-print-this-example"));
    }
    let out = run(
        root,
        &[
            "-s",
            "translate",
            "--var",
            "unused=never-print-this-example",
        ],
        b"data",
    );
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("never-print-this-example"));
}

#[test]
fn interactive_presets_and_json_task_work_in_a_real_terminal() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::sync::mpsc;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (url, server) = model("chat", r#"{"ok":true}"#);
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!("model='fixture'\napi='chat'\nbase_url='{url}'\n"),
    )
    .unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 35,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut cmd = CommandBuilder::new(binary);
    cmd.args(["--config", config.to_str().unwrap(), "--no-stream"]);
    cmd.cwd(root);
    for (k, _) in std::env::vars() {
        if k.starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY"
                    | "ANTHROPIC_API_KEY"
                    | "NO_COLOR"
                    | "HTTP_PROXY"
                    | "HTTPS_PROXY"
                    | "ALL_PROXY"
            )
        {
            cmd.env_remove(k);
        }
    }
    cmd.env("APPDATA", root.join("config"));
    cmd.env("XDG_CONFIG_HOME", root.join("config"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("NO_PROXY", "*");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut r = pair.master.try_clone_reader().unwrap();
    let mut w = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = [0; 4096];
        while let Ok(n) = r.read(&mut b) {
            if n == 0 || tx.send(b[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut offset = 0;
    let mut queries = 0;
    for (prompt, answer) in [
        ("›", "/presets"),
        ("summarize [builtin]", "/run extract name"),
        (r#"{"ok":true}"#, "/exit"),
    ] {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(i) = transcript[offset..].find(prompt) {
                offset += i + prompt.len();
                break;
            }
            match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(bytes) => {
                    transcript.push_str(&String::from_utf8_lossy(&bytes));
                    let count = transcript.matches("\x1b[6n").count();
                    for _ in queries..count {
                        w.write_all(b"\x1b[1;1R").unwrap();
                    }
                    w.flush().unwrap();
                    queries = count;
                }
                Err(_) => {
                    child.kill().ok();
                    panic!("missing {prompt}: {transcript}")
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        write!(w, "{answer}\r").unwrap();
        w.flush().unwrap();
    }
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            assert!(s.success(), "{transcript}");
            break;
        }
        assert!(Instant::now() < end, "terminal did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    let request = server.join().unwrap();
    assert!(
        request["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("Return exactly one valid JSON value")
    );
    assert!(!root.join("presets.toml").exists());
}
