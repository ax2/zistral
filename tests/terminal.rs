use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn terminal(
    args: &[&str],
    home: &std::path::Path,
    plain: bool,
    answers: &[(&str, &str)],
) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut command = CommandBuilder::new(binary);
    command.args(args);
    if plain {
        command.arg("--plain");
    }
    for key in [
        "ZI_MODEL",
        "ZI_BASE_URL",
        "ZI_API",
        "ZI_API_KEY",
        "ZI_CONFIG",
        "OPENAI_API_KEY",
        "NO_COLOR",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(key);
    }
    command.env("HOME", home);
    command.env("USERPROFILE", home);
    command.env("CODEX_HOME", home.join(".codex"));
    command.env("APPDATA", home.join("config"));
    command.env("XDG_CONFIG_HOME", home.join("config"));
    command.env("NO_PROXY", "*");
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut offset = 0;
    let mut queries = 0;
    for (prompt, answer) in answers {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(i) = transcript[offset..].find(prompt) {
                offset += i + prompt.len();
                break;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(bytes) => {
                    transcript.push_str(&String::from_utf8_lossy(&bytes));
                    let found = transcript.matches("\x1b[6n").count();
                    for _ in queries..found {
                        writer.write_all(b"\x1b[1;1R").unwrap();
                    }
                    writer.flush().unwrap();
                    queries = found;
                }
                Err(_) => {
                    child.kill().ok();
                    panic!("did not reach {prompt}: {transcript}");
                }
            }
        }
        if !answer.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            write!(writer, "{answer}\r").unwrap();
            writer.flush().unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{transcript}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!("session did not exit: {transcript}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    while let Ok(bytes) = rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&bytes));
    }
    transcript
}

fn server() -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let replies = [
            json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"one","type":"function","function":{"name":"count_input","arguments":"{}"}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
            json!({"choices":[{"finish_reason":"stop","message":{"content":"# Result\n- answer-one\n```text\nhello\n```\u{001b}]52;c;clipboard-marker\u{0007}"}}],"usage":{"prompt_tokens":20,"completion_tokens":4}}),
            json!({"choices":[{"finish_reason":"stop","message":{"content":"answer-two"}}]}),
        ];
        let mut requests = Vec::new();
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "mock server timed out");
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
            let mut size = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        size = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            let mut body = vec![0; size];
            reader.read_exact(&mut body).unwrap();
            requests.push(serde_json::from_slice(&body).unwrap());
            std::thread::sleep(Duration::from_millis(150));
            let body = reply.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        requests
    });
    (url, task)
}

#[test]
fn first_launch_imports_then_runs_rich_terminal_tasks() {
    let home = tempfile::tempdir().unwrap();
    let (url, task) = server();
    std::fs::create_dir(home.path().join(".workbuddy")).unwrap();
    std::fs::write(
        home.path().join(".workbuddy/models.json"),
        json!([{"id":"fixture-model","url":url,"apiKey":"fixture-secret"}]).to_string(),
    )
    .unwrap();
    let output = terminal(
        &["--base-url", &url],
        home.path(),
        false,
        &[
            ("Configuration source", "1"),
            ("›", "first task"),
            ("answer-one", "second task"),
            ("answer-two", "/exit"),
            ("Goodbye.", ""),
        ],
    );
    assert!(output.contains(concat!("Zistral ", env!("CARGO_PKG_VERSION"))));
    assert!(output.contains("Thinking"));
    assert!(output.contains("Count input"));
    assert!(output.contains("30 in / 6 out tokens"));
    assert!(!output.contains("fixture-secret"));
    assert!(!output.contains("clipboard-marker"));
    let requests = task.join().unwrap();
    assert_eq!(requests[2]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(requests[2]["messages"][1]["content"], "second task");
    assert!(home.path().join("config/zistral/config.toml").exists());
}

#[test]
fn plain_terminal_supports_help_and_exit_without_decorations() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config/zistral");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "model='fixture'\nbase_url='http://localhost:11434/v1'\n",
    )
    .unwrap();
    let output = terminal(
        &[],
        home.path(),
        true,
        &[
            ("zi>", "/help"),
            ("independent task", "/exit"),
            ("Goodbye.", ""),
        ],
    );
    assert!(!output.contains("◆"));
    assert!(!output.contains("Thinking"));
    assert!(!output.contains("Configuration source"));
}
