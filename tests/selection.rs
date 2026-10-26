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
fn invoke(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut p = command(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = p.stdin.take().unwrap().write_all(input);
    p.wait_with_output().unwrap()
}
fn response(api: &str, text: &str) -> Value {
    match api {
        "responses" => {
            json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]})
        }
        "anthropic" => json!({"stop_reason":"end_turn","content":[{"type":"text","text":text}]}),
        _ => json!({"choices":[{"finish_reason":"stop","message":{"content":text}}]}),
    }
}
fn model(
    api: &str,
    text: &str,
    tool: Option<(Value, Option<String>)>,
) -> (String, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let api = api.to_owned();
    let text = text.to_owned();
    let task = std::thread::spawn(move || {
        for turn in 0..if tool.is_some() { 2 } else { 1 } {
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut s = loop {
                match l.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "model request missing");
                        std::thread::sleep(Duration::from_millis(5));
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
                assert!(r.read_line(&mut line).unwrap() > 0);
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
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["stream"], false);
            if turn == 1 {
                let last=match api.as_str(){"responses"=>request["input"].as_array().unwrap().last().unwrap()["output"].as_str().unwrap(),"anthropic"=>request["messages"].as_array().unwrap().last().unwrap()["content"][0]["content"].as_str().unwrap(),_=>request["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap()};
                if let Some(expected) = &tool.as_ref().unwrap().1 {
                    assert_eq!(last, expected);
                } else {
                    let error: Value = serde_json::from_str(last).unwrap();
                    assert!(error["error"].is_string());
                    assert!(!last.contains("PRIVATE_VALUE"));
                }
            }
            let body=if let Some((args,_))=tool.as_ref().filter(|_|turn==0){match api.as_str(){"responses"=>json!({"status":"completed","output":[{"type":"function_call","call_id":"t","name":"select_input","arguments":args.to_string()}]}),"anthropic"=>json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":"t","name":"select_input","input":args}]}),_=>json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"t","type":"function","function":{"name":"select_input","arguments":args.to_string()}}]}}]})}}else{response(&api,&text)}.to_string();
            write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, task)
}
#[test]
fn offline_selection_and_join_keep_raw_numbers_key_order_and_decoded_strings() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("invalid.toml"), "invalid model config").unwrap();
    let source = "\u{feff}{\"abc\":[[123456789012345678901234567890,1.2300E+400,-0,\"\\u4e2d\",{\"z\":1,\"a\":2,\"z\":3}]],\"unused\":1e9999}";
    std::fs::write(root.join("input.json"), source).unwrap();
    let out = invoke(
        root,
        &[
            "--config",
            "invalid.toml",
            "--select",
            "/abc/0",
            "--join",
            "|",
            "-i",
            "input.json",
        ],
        b"",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "123456789012345678901234567890|1.2300E+400|-0|中|{\"z\":1,\"a\":2,\"z\":3}\n"
    );
    let out = invoke(
        root,
        &[
            "--select",
            "/abc/0",
            "--json",
            "-i",
            "input.json",
            "-o",
            "result.json",
        ],
        b"",
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("result.json")).unwrap(),
        "[123456789012345678901234567890,1.2300E+400,-0,\"\\u4e2d\",{\"z\":1,\"a\":2,\"z\":3}]\n"
    );
    let out = invoke(
        root,
        &["--select", "/abc/0/0", "--json", "-i", "input.json"],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"123456789012345678901234567890\n");
    assert_eq!(
        std::fs::read_to_string(root.join("input.json")).unwrap(),
        source
    );
    assert!(!root.join("config").exists());
    let out = invoke(
        root,
        &[
            "--select",
            "/abc/0",
            "--json",
            "-i",
            "input.json",
            "-o",
            "input.json",
            "--force",
        ],
        b"",
    );
    assert!(out.status.success() && out.stdout.is_empty());
    assert_eq!(
        std::fs::read(root.join("input.json")).unwrap(),
        std::fs::read(root.join("result.json")).unwrap()
    );
}
#[test]
fn ambiguous_invalid_and_oversized_references_preserve_targets_without_partial_output() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("result.txt"), b"old result\r\n").unwrap();
    for (input, pointer) in [
        (r#"{"a":1,"a":2}"#, "/a"),
        (r#"{"a":1,"\u0061":2}"#, "/a"),
        ("[1,2]", "/01"),
        ("[1,2]", "/-"),
        (r#"{"PRIVATE_VALUE":1}"#, "/PRIVATE_VALUE~2"),
        ("{}", "/missing"),
        ("{\"a\":1} trailing", "/a"),
    ] {
        let out = invoke(
            root,
            &["--select", pointer, "-o", "result.txt", "--force"],
            input.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_VALUE"));
        assert_eq!(
            std::fs::read(root.join("result.txt")).unwrap(),
            b"old result\r\n"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
    let pointer = format!("/{}", "a".repeat(4096));
    assert!(
        !invoke(root, &["--select", &pointer], b"{}")
            .status
            .success()
    );
    let out = invoke(root, &["--select", "/a~1b/~01"], br#"{"a/b":{"~1":"yes"}}"#);
    assert_eq!(out.stdout, b"yes\n");
}
#[test]
fn selection_tool_preserves_precision_and_returns_bounded_errors_in_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        for (input, args, expected) in [
            (
                r#"{"abc":[123456789012345678901234567890,1.2300E+400,-0,{"z":1,"a":2,"z":3}]}"#
                    .into(),
                json!({"pointer":"/abc","separator":"|"}),
                Some(
                    "123456789012345678901234567890|1.2300E+400|-0|{\"z\":1,\"a\":2,\"z\":3}"
                        .into(),
                ),
            ),
            (r#"{"x":1,"x":2}"#.into(), json!({"pointer":"/x"}), None),
            (
                format!("{{\"x\":\"{}\"}}", "a".repeat(65537)),
                json!({"pointer":"/x"}),
                None,
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (url, task) = model(api, "verified", Some((args, expected)));
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "--no-stream",
                    "select and join",
                ],
                input.as_bytes(),
            );
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"verified\n");
            task.join().unwrap();
        }
    }
}
#[test]
fn strict_json_answers_keep_lexical_values_in_stdout_and_files_for_every_protocol() {
    let text = r#" { "z":123456789012345678901234567890,"a":1.2300E+400,"z":-0,"s":"\u4e2d" } "#;
    let expected = br#"{"z":123456789012345678901234567890,"a":1.2300E+400,"z":-0,"s":"\u4e2d"}"#;
    for api in ["chat", "responses", "anthropic"] {
        for file in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (url, task) = model(api, text, None);
            let mut args = vec![
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "--json",
            ];
            if file {
                args.extend(["-o", "result.json"]);
            }
            args.push("extract");
            let out = invoke(root.path(), &args, b"input");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let mut result = expected.to_vec();
            result.push(b'\n');
            if file {
                assert!(out.stdout.is_empty());
                assert_eq!(
                    std::fs::read(root.path().join("result.json")).unwrap(),
                    result
                );
            } else {
                assert_eq!(out.stdout, result);
            }
            task.join().unwrap();
        }
    }
}
#[test]
fn interactive_strict_json_answer_keeps_precision_and_escape_forms() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let text = r#"{"n":123456789012345678901234567890,"n":1.2300E+400,"s":"\u4e2d"}"#;
    let (url, task) = model("chat", text, None);
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut cmd = CommandBuilder::new(binary);
    cmd.args(["--base-url", &url, "--model", "fixture", "--json"]);
    cmd.cwd(root.path());
    for (k, _) in std::env::vars() {
        if k.starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY"
                    | "ANTHROPIC_API_KEY"
                    | "HTTP_PROXY"
                    | "HTTPS_PROXY"
                    | "ALL_PROXY"
                    | "NO_COLOR"
            )
        {
            cmd.env_remove(k);
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
    let mut asked = false;
    let mut exited = false;
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(40)) {
            transcript.push_str(&String::from_utf8_lossy(&b));
            let count = transcript.matches("\x1b[6n").count();
            for _ in queries..count {
                writer.write_all(b"\x1b[1;1R").unwrap();
            }
            writer.flush().unwrap();
            queries = count;
        }
        if !asked && transcript.contains("zi> ") {
            writer.write_all(b"extract\r").unwrap();
            writer.flush().unwrap();
            asked = true;
        }
        if !exited && transcript.contains(text) {
            writer.write_all(b"/exit\r").unwrap();
            writer.flush().unwrap();
            exited = true;
        }
        if let Some(s) = child.try_wait().unwrap() {
            assert!(s.success(), "{transcript}");
            break;
        }
        if Instant::now() > end {
            child.kill().ok();
            panic!("terminal timed out: {transcript}");
        }
    }
    task.join().unwrap();
    assert!(exited && transcript.contains(text), "{transcript}");
    assert!(!transcript.contains("out of range"));
}
