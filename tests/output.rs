use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
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
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}
fn no_temporary_files(root: &Path) {
    assert!(!std::fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

fn model(
    api: &str,
    text: &str,
    status: u16,
    change: Option<PathBuf>,
) -> (String, std::thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let api = api.to_owned();
    let text = text.to_owned();
    let task = std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        let mut s = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < end, "request missing");
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        };
        s.set_nonblocking(false).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    length = v.trim().parse().unwrap()
                }
            }
        }
        let mut bytes = vec![0; length];
        r.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            request["stream"], false,
            "file output must request a complete answer"
        );
        if let Some(path) = change {
            std::fs::write(path, b"external writer").unwrap();
        }
        let body=if status!=200{json!({"error":{"message":text}})}else{match api.as_str(){
            "responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]}),
            "anthropic"=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":text}]}),
            _=>json!({"choices":[{"finish_reason":"stop","message":{"content":text}}]}),
        }}.to_string();
        write!(s,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        request
    });
    (url, task)
}

#[test]
fn offline_results_are_saved_without_stdout_and_in_place_input_is_read_first() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let out = invoke(
        root,
        &["data", "dedupe", "-o", "out.txt"],
        "a\na\n你好".as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        std::fs::read(root.join("out.txt")).unwrap(),
        "a\n你好\n".as_bytes()
    );
    let compact = r#"{"n":123456789012345678901234567890}"#;
    std::fs::write(root.join("data.json"), compact).unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "json-format",
            "-i",
            "data.json",
            "-o",
            "data.json",
            "--force",
        ],
        b"ignored",
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    let out = invoke(root, &["data", "json-minify", "-i", "data.json"], b"");
    assert_eq!(out.stdout, format!("{compact}\n").as_bytes());
    let out = invoke(root, &["data", "trim", "-o", "empty"], b"");
    assert!(out.status.success());
    assert_eq!(std::fs::read(root.join("empty")).unwrap(), b"");
    let out = invoke(root, &["--count", "-o", "count.json"], b"hello");
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(root.join("count.json")).unwrap()).unwrap()
            ["words"],
        1
    );
    let out = invoke(root, &["--config-example", "-o", "example.toml"], b"");
    assert!(out.status.success());
    let _: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("example.toml")).unwrap()).unwrap();
    let out = invoke(root, &["data", "trim", "-o", "-"], b" x ");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"x\n");
    assert!(!root.join("-").exists());
    no_temporary_files(root);
}
#[test]
fn preflight_and_option_errors_do_not_contact_a_model_or_touch_the_target() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("keep.txt"), b"keep").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let out = invoke(
        root,
        &[
            "--base-url",
            &url,
            "--model",
            "fixture",
            "-o",
            "keep.txt",
            "task",
        ],
        b"data",
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    for args in [
        vec!["--stream", "-o", "new.txt", "task"],
        vec!["-o", "new.txt"],
        vec!["models", "-o", "new.txt"],
        vec!["--force", "--count"],
        vec!["--force", "-o", "-", "--count"],
        vec!["data", "trim", "-o", "missing/out.txt"],
    ] {
        let out = invoke(root, &args, b"x");
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(!root.join("new.txt").exists());
    }
    assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"keep");
    no_temporary_files(root);
}
#[test]
fn all_protocols_save_raw_results_without_disclosing_destination_or_streaming() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let text = "# 标题\n- raw result\n";
        std::fs::write(
            root.join("destination-private-7917.md"),
            b"old-result-private-5ad1",
        )
        .unwrap();
        let (url, server) = model(api, text, 200, None);
        let out = invoke(
            root,
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "-o",
                "destination-private-7917.md",
                "--force",
                "summarize",
            ],
            b"source",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
        assert_eq!(
            std::fs::read(root.join("destination-private-7917.md")).unwrap(),
            text.as_bytes()
        );
        let request = server.join().unwrap();
        assert!(!request.to_string().contains("destination-private-7917"));
        assert!(!request.to_string().contains("old-result-private-5ad1"));
        no_temporary_files(root);
    }
    let root = tempfile::tempdir().unwrap();
    let (url, server) = model("chat", "{\n  \"name\":\"小明\"\n}", 200, None);
    let out = invoke(
        root.path(),
        &[
            "--base-url",
            &url,
            "--model",
            "fixture",
            "--json",
            "-o",
            "result.json",
            "extract",
        ],
        b"source",
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read(root.path().join("result.json")).unwrap(),
        "{\"name\":\"小明\"}\n".as_bytes()
    );
    server.join().unwrap();
    no_temporary_files(root.path());
}
#[test]
fn request_and_json_failures_preserve_original_including_same_input_output() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for (text, status, json_mode) in [
        ("do-not-echo-provider-secret", 401, false),
        ("partial {", 200, true),
    ] {
        std::fs::write(root.join("same.txt"), b"original input").unwrap();
        let (url, server) = model("chat", text, status, None);
        let mut args = vec![
            "--base-url",
            &url,
            "--model",
            "fixture",
            "-i",
            "same.txt",
            "-o",
            "same.txt",
            "--force",
        ];
        if json_mode {
            args.push("--json");
        }
        args.push("summarize");
        let out = invoke(root, &args, b"");
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("provider-secret"));
        assert_eq!(
            std::fs::read(root.join("same.txt")).unwrap(),
            b"original input"
        );
        server.join().unwrap();
        no_temporary_files(root);
    }
    std::fs::write(root.join("same.txt"), b"{invalid").unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "json-format",
            "-i",
            "same.txt",
            "-o",
            "same.txt",
            "--force",
        ],
        b"",
    );
    assert!(!out.status.success());
    assert_eq!(std::fs::read(root.join("same.txt")).unwrap(), b"{invalid");
    no_temporary_files(root);
}
#[test]
fn changes_during_requests_and_new_file_races_are_never_silently_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for existing in [false, true] {
        let path = root.join("out");
        if existing {
            std::fs::write(&path, b"old").unwrap();
        }
        let (url, server) = model("chat", "new", 200, Some(path.clone()));
        let out = invoke(
            root,
            &[
                "--base-url",
                &url,
                "--model",
                "fixture",
                "--force",
                "-o",
                "out",
                "task",
            ],
            b"data",
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"external writer");
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        no_temporary_files(root);
    }
}
#[test]
fn real_terminal_file_output_disables_auto_stream_and_shows_saved_status_only() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let (url, server) = model("chat", "unique-provider-answer-9ad7", 200, None);
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
        "-o",
        "saved.md",
        "task",
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
            panic!("terminal timeout: {transcript}");
        }
    }
    server.join().unwrap();
    assert!(transcript.contains("Response ready"), "{transcript}");
    assert!(
        transcript.contains("Saved") && transcript.contains("saved.md"),
        "{transcript}"
    );
    assert!(
        !transcript.contains("unique-provider-answer-9ad7"),
        "{transcript}"
    );
    assert_eq!(
        std::fs::read(root.path().join("saved.md")).unwrap(),
        b"unique-provider-answer-9ad7\n"
    );
    no_temporary_files(root.path());
}

#[test]
fn terminal_control_cleanup_never_rewrites_redirected_or_explicit_file_results() {
    let payload = "BEFORE\x1b[31mCOLOR\x1b[0m\x1b]0;RAW_TITLE_SENTINEL\x07AFTER\r\nsecond line\n";
    for api in ["chat", "responses", "anthropic"] {
        for plain in [false, true] {
            for file in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path();
                let (url, server) = model(api, payload, 200, None);
                let mut args = vec![
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "--no-stream",
                ];
                if plain {
                    args.push("--plain");
                }
                if file {
                    args.extend(["-o", "result.txt"]);
                }
                args.push("return raw text");
                let out = invoke(root, &args, b"");
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                if file {
                    assert!(out.stdout.is_empty());
                    assert_eq!(
                        std::fs::read(root.join("result.txt")).unwrap(),
                        payload.as_bytes()
                    );
                } else {
                    assert_eq!(out.stdout, payload.as_bytes());
                }
                assert!(out.stderr.is_empty());
                server.join().unwrap();
                no_temporary_files(root);
            }
        }
    }
}
