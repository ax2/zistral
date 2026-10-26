use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Reply {
    code: u16,
    headers: String,
    body: String,
    sse: bool,
    wait: Duration,
}
fn reply(code: u16, body: impl Into<String>) -> Reply {
    Reply {
        code,
        headers: String::new(),
        body: body.into(),
        sse: false,
        wait: Duration::ZERO,
    }
}
fn transient(code: u16) -> Reply {
    Reply {
        headers: "Retry-After: 0\r\n".into(),
        ..reply(code, "PRIVATE_PROVIDER_BODY")
    }
}
fn final_reply(api: &str, text: &str) -> Reply {
    reply(200,match api {
        "responses"=>json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]}),
        "anthropic"=>json!({"stop_reason":"end_turn","content":[{"type":"text","text":text}]}),
        _=>json!({"choices":[{"finish_reason":"stop","message":{"content":text}}]}),
    }.to_string())
}
type Requests = Vec<(Value, Instant)>;
fn server(replies: Vec<Reply>) -> (String, mpsc::Sender<()>, std::thread::JoinHandle<Requests>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let task = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if rx.try_recv().is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "fixture timed out");
            let mut stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
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
            let i = requests.len();
            assert!(i < 10, "unbounded retry");
            requests.push((serde_json::from_slice(&bytes).unwrap(), Instant::now()));
            let r = replies.get(i).unwrap_or_else(|| replies.last().unwrap());
            std::thread::sleep(r.wait);
            if r.code == 0 {
                continue;
            }
            let mime = if r.sse {
                "text/event-stream"
            } else {
                "application/json"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {} Fixture\r\nContent-Type: {mime}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                r.code,
                r.headers,
                r.body.len(),
                r.body
            );
        }
        requests
    });
    (url, tx, task)
}
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
fn invoke(root: &Path, url: &str, args: &[&str]) -> Output {
    let mut p = command(root)
        .args(["--base-url", url, "--model", "fixture", "--timeout", "2"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    p.stdin.take().unwrap().write_all(b"PRIVATE_INPUT").ok();
    p.wait_with_output().unwrap()
}
fn done(stop: mpsc::Sender<()>, task: std::thread::JoinHandle<Requests>) -> Requests {
    stop.send(()).unwrap();
    task.join().unwrap()
}

#[test]
fn opt_in_retries_same_turn_body_in_all_protocols_and_saves_only_final_json() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let (url, stop, task) = server(vec![
            transient(429),
            transient(if api == "anthropic" { 529 } else { 503 }),
            final_reply(api, "{\"ok\":true}"),
        ]);
        let out = invoke(
            root.path(),
            &url,
            &[
                "--api",
                api,
                "--retries",
                "2",
                "--max-steps",
                "1",
                "--json",
                "-o",
                "result.json",
                "extract",
            ],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("result.json")).unwrap(),
            b"{\"ok\":true}\n"
        );
        let requests = done(stop, task);
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].0, requests[1].0);
        assert_eq!(requests[1].0, requests[2].0);
    }
}
#[test]
fn defaults_permanent_errors_and_exhausted_budgets_stop_without_disclosure() {
    let root = tempfile::tempdir().unwrap();
    for (status, budget, expected) in [
        (429, 0, 1),
        (503, 0, 1),
        (401, 3, 1),
        (403, 3, 1),
        (404, 3, 1),
        (408, 3, 1),
        (409, 3, 1),
        (501, 3, 1),
        (500, 2, 3),
        (502, 1, 2),
        (504, 3, 4),
    ] {
        let (url, stop, task) = server(vec![transient(status)]);
        let n = budget.to_string();
        let out = invoke(root.path(), &url, &["--retries", &n, "request"]);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains(&format!("HTTP {status}")), "{error}");
        assert!(!error.contains("PRIVATE_"));
        assert_eq!(done(stop, task).len(), expected);
    }
    for args in [
        vec!["--retries", "4", "task"],
        vec!["--retries", "1", "--count"],
        vec!["--retries", "1", "doctor", "--probe"],
        vec!["--retries", "1", "data", "trim"],
    ] {
        let out = invoke(root.path(), "http://127.0.0.1:1/v1", &args);
        assert!(!out.status.success() && out.stdout.is_empty());
    }
}
#[test]
fn retry_after_is_honored_and_unacceptable_waits_preserve_old_files() {
    let root = tempfile::tempdir().unwrap();
    for (header, min) in [
        ("Retry-After: 1\r\n", Duration::from_millis(900)),
        ("", Duration::from_millis(200)),
        (
            "Retry-After: Sun, 06 Nov 1994 08:49:37 GMT\r\n",
            Duration::ZERO,
        ),
    ] {
        let mut r = transient(503);
        r.headers = header.into();
        let (url, stop, task) = server(vec![r, final_reply("chat", "ok")]);
        let out = invoke(root.path(), &url, &["--retries", "1", "--verbose", "task"]);
        assert!(out.status.success());
        assert_eq!(out.stdout, b"ok\n");
        let requests = done(stop, task);
        assert_eq!(requests.len(), 2);
        assert!(requests[1].1.duration_since(requests[0].1) >= min);
        assert!(String::from_utf8_lossy(&out.stderr).contains("Retry 1/1"));
    }
    std::fs::write(root.path().join("old.txt"), b"old bytes\r\n").unwrap();
    for header in [
        "Retry-After: 31\r\n",
        "Retry-After: secret-value\r\n",
        "Retry-After: 0\r\nRetry-After: 1\r\n",
    ] {
        let mut r = transient(503);
        r.headers = header.into();
        let (url, stop, task) = server(vec![r]);
        let out = invoke(
            root.path(),
            &url,
            &["--retries", "3", "-o", "old.txt", "--force", "task"],
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("secret-value"));
        assert_eq!(done(stop, task).len(), 1);
        assert_eq!(
            std::fs::read(root.path().join("old.txt")).unwrap(),
            b"old bytes\r\n"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
#[test]
fn successful_headers_disconnects_invalid_json_and_partial_streams_are_never_replayed() {
    let root = tempfile::tempdir().unwrap();
    for r in [
        reply(0, ""),
        Reply {
            wait: Duration::from_millis(2200),
            ..final_reply("chat", "late")
        },
        reply(200, "invalid json"),
        Reply {
            sse: true,
            ..reply(
                200,
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"first\"},\"finish_reason\":null}]}\n\n",
            )
        },
    ] {
        let streamed = r.sse;
        let (url, stop, task) = server(vec![r]);
        let out = invoke(root.path(), &url, &["--retries", "3", "--stream", "task"]);
        assert!(!out.status.success());
        assert_eq!(done(stop, task).len(), 1);
        if streamed {
            assert_eq!(out.stdout, b"first\n");
        } else {
            assert!(out.stdout.is_empty());
        }
    }
}
#[test]
fn retrying_followup_turn_does_not_execute_a_project_command_twice() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let cmd = "[IO.File]::AppendAllText('executed.txt','x')";
    #[cfg(unix)]
    let cmd = "printf x >> executed.txt";
    let tool=reply(200,json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"t","type":"function","function":{"name":"run_command","arguments":json!({"command":cmd}).to_string()}}]}}]}).to_string());
    let (url, stop, task) = server(vec![tool, transient(503), final_reply("chat", "done")]);
    let out = invoke(
        root.path(),
        &url,
        &["--agent", "--retries", "1", "--max-steps", "2", "task"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"done\n");
    assert_eq!(
        std::fs::read(root.path().join("executed.txt")).unwrap(),
        b"x"
    );
    let requests = done(stop, task);
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].0, requests[2].0);
}
#[test]
fn real_terminal_reports_retry_and_counts_actual_http_attempts() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    let root = tempfile::tempdir().unwrap();
    let (url, stop, task) = server(vec![transient(503), final_reply("chat", "recovered")]);
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
    cmd.args([
        "--base-url",
        &url,
        "--model",
        "fixture",
        "--no-stream",
        "--retries",
        "1",
        "--max-steps",
        "1",
        "task",
    ]);
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
    let mut text = String::new();
    let mut queries = 0;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(40)) {
            text.push_str(&String::from_utf8_lossy(&b));
            let count = text.matches("\x1b[6n").count();
            for _ in queries..count {
                writer.write_all(b"\x1b[1;1R").unwrap();
            }
            writer.flush().unwrap();
            queries = count;
        }
        if let Some(s) = child.try_wait().unwrap() {
            assert!(s.success(), "{text}");
            while let Ok(b) = rx.recv_timeout(Duration::from_millis(80)) {
                text.push_str(&String::from_utf8_lossy(&b));
            }
            break;
        }
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("terminal timed out: {text}");
        }
    }
    assert_eq!(done(stop, task).len(), 2);
    for value in ["Retry 1/1", "HTTP 503", "2 requests", "recovered"] {
        assert!(text.contains(value), "{value}: {text}");
    }
    assert!(!text.contains("PRIVATE_PROVIDER_BODY"));
}
