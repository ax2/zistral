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
    c.env("APPDATA", root.join("config"))
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
    // Invalid arguments may close stdin before all test input has been written.
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}
#[test]
fn offline_line_commands_need_no_configuration_and_chain_via_pipes() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    // An invalid config would fail any attempted model setup or configuration load.
    std::fs::write(
        root.join("invalid.toml"),
        "broken configuration, do not load",
    )
    .unwrap();
    let out = invoke(
        root,
        &["--config", "invalid.toml", "data", "trim"],
        " 乙 \r\n甲\r\n 乙 \n\n".as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, "乙\n甲\n乙\n\n".as_bytes());
    assert!(out.stderr.is_empty());
    let mut first = command(root)
        .args(["data", "dedupe"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .args(["data", "sort", "--reverse"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    first.stdin.take().unwrap().write_all(&out.stdout).unwrap();
    assert!(first.wait().unwrap().success());
    let out = second.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, "甲\n乙\n\n".as_bytes());
    assert!(out.stderr.is_empty());
    for op in ["trim", "dedupe", "sort"] {
        assert!(invoke(root, &["data", op], b"").stdout.is_empty());
    }
    assert!(!root.join("config").exists());
}
#[test]
fn json_tokens_and_input_files_survive_roundtrip_without_numeric_conversion() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let compact =
        r#"{"z":123456789012345678901234567890,"a":1.23400E+999,"z":-0,"s":"[] , : \" \\ \u4e2d"}"#;
    std::fs::write(root.join("input.json"), format!("\u{feff}{compact}")).unwrap();
    let pretty = invoke(
        root,
        &["data", "json-format", "-i", "input.json"],
        b"ignored",
    );
    assert!(
        pretty.status.success(),
        "{}",
        String::from_utf8_lossy(&pretty.stderr)
    );
    let min = invoke(root, &["data", "json-minify"], &pretty.stdout);
    assert!(min.status.success());
    assert_eq!(min.stdout, format!("{compact}\n").as_bytes());
    let keys = invoke(root, &["data", "json-keys", "-i", "-"], compact.as_bytes());
    assert!(keys.status.success());
    assert_eq!(keys.stdout, b"[\"z\",\"a\",\"z\",\"s\"]\n");
    assert!(pretty.stderr.is_empty() && min.stderr.is_empty() && keys.stderr.is_empty());
}
#[test]
fn local_failures_keep_stdout_empty_and_do_not_mutate_input() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for (args, input) in [
        (vec!["data", "json-minify"], b"{ bad".as_slice()),
        (vec!["data", "json-keys"], b"[1,2]".as_slice()),
        (vec!["data", "trim", "--reverse"], b"a".as_slice()),
        (vec!["--json", "data", "sort"], b"a".as_slice()),
        (vec!["--agent", "data", "dedupe"], b"a".as_slice()),
        (vec!["--stream", "data", "sort"], b"a".as_slice()),
        (vec!["--var", "x=y", "data", "sort"], b"a".as_slice()),
        (vec!["--max-input", "2", "data", "trim"], b"abc".as_slice()),
        (vec!["data", "trim"], b"\xff".as_slice()),
    ] {
        let out = invoke(root, &args, input);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty());
    }
    let too_many = "a\n".repeat(100001);
    let out = invoke(root, &["data", "dedupe"], too_many.as_bytes());
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    std::fs::write(root.join("source.json"), b"{broken").unwrap();
    let out = invoke(root, &["data", "json-format", "-i", "source.json"], b"");
    assert!(!out.status.success());
    assert_eq!(std::fs::read(root.join("source.json")).unwrap(), b"{broken");
}

fn model(api: &str, args: Value, expected: Value) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let api = api.to_owned();
    let task = std::thread::spawn(move || {
        for turn in 0..2 {
            let end = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "model request missing");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        len = v.trim().parse().unwrap()
                    }
                }
            }
            let mut data = vec![0; len];
            r.read_exact(&mut data).unwrap();
            let request: Value = serde_json::from_slice(&data).unwrap();
            if turn == 0 {
                let names: Vec<_> = request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| {
                        if api == "chat" {
                            t["function"]["name"].as_str().unwrap()
                        } else {
                            t["name"].as_str().unwrap()
                        }
                    })
                    .collect();
                assert_eq!(names, ["count_input", "select_input", "transform_input"]);
            } else {
                let result =
                    match api.as_str() {
                        "chat" => {
                            request["messages"].as_array().unwrap().last().unwrap()["content"]
                                .as_str()
                                .unwrap()
                        }
                        "responses" => {
                            request["input"].as_array().unwrap().last().unwrap()["output"]
                                .as_str()
                                .unwrap()
                        }
                        _ => request["messages"].as_array().unwrap().last().unwrap()["content"][0]
                            ["content"]
                            .as_str()
                            .unwrap(),
                    };
                let result: Value = serde_json::from_str(result).unwrap();
                if expected.is_null() {
                    assert!(result["error"].is_string());
                    assert!(result.get("output").is_none())
                } else {
                    assert_eq!(result["output"], expected)
                }
            }
            let payload=if turn==0 { match api.as_str(){
                "chat"=>json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"t","type":"function","function":{"name":"transform_input","arguments":args.to_string()}}]}}]}),
                "responses"=>json!({"status":"completed","output":[{"type":"function_call","call_id":"t","name":"transform_input","arguments":args.to_string()}]}),
                _=>json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":"t","name":"transform_input","input":args}]}),
            }}else {match api.as_str(){
                "chat"=>json!({"choices":[{"finish_reason":"stop","message":{"content":"verified"}}]}),
                "responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"verified"}]}]}),
                _=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":"verified"}]}),
            }}.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
        }
    });
    (url, task)
}
#[test]
fn transform_tool_works_on_original_input_in_every_protocol_without_project_permissions() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        for (args, input, expected) in [
            (
                json!({"operation":"dedupe"}),
                "b\r\na\nb\n",
                json!("b\na\n"),
            ),
            (
                json!({"operation":"json-minify"}),
                "{ \"n\":123456789012345678901234567890 }",
                json!("{\"n\":123456789012345678901234567890}"),
            ),
        ] {
            let (url, task) = model(api, args, expected);
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "transform the input",
                ],
                input.as_bytes(),
            );
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"verified\n");
            assert!(out.stderr.is_empty());
            task.join().unwrap();
        }
    }
}
#[test]
fn tool_argument_and_output_limits_return_errors_to_model_without_partial_output() {
    let root = tempfile::tempdir().unwrap();
    for (args, input) in [
        (json!({"operation":"trim"}), "x".repeat(65536)),
        (
            json!({"operation":"sort","path":"must-not-read"}),
            "a".into(),
        ),
    ] {
        let (url, task) = model("chat", args, Value::Null);
        let out = invoke(
            root.path(),
            &["--base-url", &url, "--model", "fixture", "transform"],
            input.as_bytes(),
        );
        assert!(out.status.success());
        assert_eq!(out.stdout, b"verified\n");
        assert!(out.stderr.is_empty());
        task.join().unwrap();
    }
}

#[test]
fn rich_terminal_names_native_transform_correctly() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("lines.txt");
    std::fs::write(&source, "b\na\nb\n").unwrap();
    let (url, server) = model("chat", json!({"operation":"dedupe"}), json!("b\na\n"));
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut cmd = CommandBuilder::new(binary);
    cmd.args([
        "--base-url",
        &url,
        "--model",
        "fixture",
        "-i",
        source.to_str().unwrap(),
        "--no-stream",
        "dedupe",
    ]);
    cmd.cwd(root.path());
    for (name, _) in std::env::vars() {
        if name.starts_with("ZI_")
            || matches!(
                name.to_uppercase().as_str(),
                "OPENAI_API_KEY"
                    | "ANTHROPIC_API_KEY"
                    | "NO_COLOR"
                    | "HTTP_PROXY"
                    | "HTTPS_PROXY"
                    | "ALL_PROXY"
            )
        {
            cmd.env_remove(name);
        }
    }
    cmd.env("APPDATA", root.path().join("config"));
    cmd.env("XDG_CONFIG_HOME", root.path().join("config"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("NO_PROXY", "*");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = [0; 4096];
        while let Ok(n) = reader.read(&mut b) {
            if n == 0 || tx.send(b[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut queries = 0;
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(50)) {
            transcript.push_str(&String::from_utf8_lossy(&bytes));
            let count = transcript.matches("\x1b[6n").count();
            for _ in queries..count {
                writer.write_all(b"\x1b[1;1R").unwrap();
            }
            writer.flush().unwrap();
            queries = count;
        }
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{transcript}");
            while let Ok(bytes) = rx.recv_timeout(Duration::from_millis(100)) {
                transcript.push_str(&String::from_utf8_lossy(&bytes));
            }
            break;
        }
        if Instant::now() > end {
            child.kill().ok();
            panic!("terminal timed out: {transcript}");
        }
    }
    server.join().unwrap();
    assert!(transcript.contains("Transform input"), "{transcript}");
    assert!(!transcript.contains("Unknown tool"), "{transcript}");
    assert!(transcript.contains("verified"), "{transcript}");
}
