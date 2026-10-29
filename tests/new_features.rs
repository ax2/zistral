use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn command() -> Command {
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
    c.env("APPDATA", std::env::temp_dir().join("zi-empty-config-v05"));
    c.env(
        "XDG_CONFIG_HOME",
        std::env::temp_dir().join("zi-empty-config-v05"),
    );
    c.env("NO_PROXY", "*");
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn accept(listener: &TcpListener) -> (TcpStream, Value) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "server timed out");
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
    let mut len = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap();
            }
        }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    (stream, serde_json::from_slice(&body).unwrap())
}

fn listener() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    (l, url)
}
fn event(value: Value) -> String {
    format!("data: {value}\n\n")
}
fn chat(text: &str, reason: Value) -> String {
    event(json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":reason}]}))
}
fn send(mut stream: TcpStream, body: &str) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nConnection: close\r\n\r\n{body}").unwrap();
    stream.flush().unwrap();
}

#[test]
fn stream_flushes_before_completion_and_no_json_stream_combination() {
    let (l, url) = listener();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, request) = accept(&l);
        assert_eq!(request["stream"], true);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}",
            chat("first", Value::Null)
        )
        .unwrap();
        stream.flush().unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        write!(stream, "{}data: [DONE]\n\n", chat("second", json!("stop"))).unwrap();
    });
    let mut child = command()
        .args([
            "--output",
            "-",
            "--stream",
            "--base-url",
            &url,
            "--model",
            "fixture",
            "say hello",
        ])
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0; 5];
    stdout.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"first");
    tx.send(()).unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "second\n");
    assert!(child.wait().unwrap().success());
    server.join().unwrap();
    let output = command()
        .args(["--stream", "--json", "hello"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn chat_stream_reassembles_fragmented_tool_arguments() {
    let (l, url) = listener();
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        let a = event(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call1","function":{"name":"count_input","arguments":"{"}}]},"finish_reason":null}]}),
        );
        let b = event(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}]},"finish_reason":"tool_calls"}]}),
        );
        send(s, &format!("{a}{b}data: [DONE]\n\n"));
        let (s, request) = accept(&l);
        assert_eq!(
            request["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
        assert_eq!(request["messages"][3]["role"], "tool");
        send(
            s,
            &format!("{}data: [DONE]\n\n", chat("done", json!("stop"))),
        );
    });
    let o = command()
        .args([
            "--stream",
            "--base-url",
            &url,
            "--model",
            "fixture",
            "count",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, b"done\n");
    server.join().unwrap();
}

#[test]
fn responses_stream_preserves_reasoning_and_tools_for_next_turn() {
    let (l, url) = listener();
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        let output = json!([{"type":"reasoning","id":"r","encrypted_content":"opaque"},{"type":"function_call","call_id":"c","name":"count_input","arguments":"{}"}]);
        send(
            s,
            &event(
                json!({"type":"response.completed","response":{"status":"completed","output":output}}),
            ),
        );
        let (s, r) = accept(&l);
        assert_eq!(r["input"][2]["encrypted_content"], "opaque");
        assert_eq!(r["input"][4]["type"], "function_call_output");
        send(
            s,
            &format!(
                "{}{}",
                event(json!({"type":"response.output_text.delta","delta":"你好"})),
                event(
                    json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"你好"}]}]}})
                )
            ),
        );
    });
    let o = command()
        .args([
            "--stream",
            "--api",
            "responses",
            "--base-url",
            &url,
            "--model",
            "fixture",
            "count",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, "你好\n".as_bytes());
    server.join().unwrap();
}

#[test]
fn anthropic_stream_reassembles_tools_and_text() {
    let (l, url) = listener();
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        let parts = [
            json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":2}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"c","name":"count_input","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ];
        send(s, &parts.into_iter().map(event).collect::<String>());
        let (s, r) = accept(&l);
        assert_eq!(r["messages"][1]["content"][0]["input"], json!({}));
        assert_eq!(r["messages"][2]["content"][0]["type"], "tool_result");
        let parts = [
            json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
            json!({"type":"message_stop"}),
        ];
        send(s, &parts.into_iter().map(event).collect::<String>());
    });
    let o = command()
        .args([
            "--stream",
            "--api",
            "anthropic",
            "--base-url",
            &url,
            "--model",
            "fixture",
            "count",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, b"done\n");
    server.join().unwrap();
}

#[test]
fn interrupted_stream_is_failure_and_default_pipeline_remains_atomic() {
    for streaming in [false, true] {
        let (l, url) = listener();
        let server = std::thread::spawn(move || {
            let (s, r) = accept(&l);
            assert_eq!(r["stream"], streaming);
            send(s, &chat("partial", Value::Null));
        });
        let mut c = command();
        c.args(["--base-url", &url, "--model", "fixture"]);
        if streaming {
            c.arg("--stream");
        }
        c.arg("hello");
        let o = c.output().unwrap();
        assert!(!o.status.success());
        if streaming {
            assert_eq!(o.stdout, b"partial\n");
        } else {
            assert!(o.stdout.is_empty());
        }
        server.join().unwrap();
    }
}

#[test]
fn profiles_switch_preserves_legacy_and_redacts_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let contents = "model='legacy'\nbase_url='http://localhost:1/v1'\napi_key='legacy-secret'\n[profiles.work]\nmodel='work-model'\nbase_url='http://localhost:2/v1'\napi_key='work-secret'\n[profiles.local]\nmodel='local-model'\nbase_url='http://localhost:3/v1'\n";
    std::fs::write(&path, contents).unwrap();
    let p = path.to_str().unwrap();
    let output = command().args(["models", "--config", p]).output().unwrap();
    assert!(output.status.success());
    let list = String::from_utf8(output.stdout).unwrap();
    assert!(list.contains("* default"));
    assert!(list.contains("work-model"));
    assert!(!list.contains("secret"));
    let o = command()
        .args(["use", "work", "--config", p])
        .output()
        .unwrap();
    assert!(o.status.success());
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("active_profile = \"work\""));
    assert!(saved.contains("legacy-secret"));
    let o = command().args(["doctor", "--config", p]).output().unwrap();
    let text = String::from_utf8(o.stdout).unwrap();
    assert!(o.status.success());
    assert!(text.contains("work-model"));
    assert!(!text.contains("work-secret"));
    let o = command()
        .args(["doctor", "--config", p, "--profile", "local"])
        .output()
        .unwrap();
    assert!(String::from_utf8(o.stdout).unwrap().contains("local-model"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let o = command()
        .args(["use", "missing", "--config", p])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let o = command()
        .args(["use", "default", "--config", p])
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("active_profile")
    );
}

#[test]
fn doctor_probe_is_explicit_and_never_prints_provider_errors_or_answers() {
    let (l, url) = listener();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        format!("model='fixture'\nbase_url='{url}'\napi_key='hidden-secret'\n"),
    )
    .unwrap();
    let p = path.to_str().unwrap();
    let offline = command().args(["doctor", "--config", p]).output().unwrap();
    assert!(offline.status.success());
    assert!(l.accept().is_err());
    let server = std::thread::spawn(move || {
        let (mut s, r) = accept(&l);
        assert_eq!(r["stream"], false);
        assert!(r.get("tools").is_none());
        write!(s,"HTTP/1.1 401 Unauthorized\r\nContent-Length: 13\r\nConnection: close\r\n\r\nhidden-secret").unwrap();
    });
    let o = command()
        .args(["doctor", "--probe", "--config", p])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(output.contains("401"));
    assert!(!output.contains("hidden-secret"));
    server.join().unwrap();
}

#[test]
fn many_stream_fragments_keep_complete_unicode_pipe_output_in_all_protocols() {
    let part = "x".repeat(121) + "中🙂";
    let expected = part.repeat(2048);
    for api in ["chat", "responses", "anthropic"] {
        let (l, url) = listener();
        let full = expected.clone();
        let fragment = part.clone();
        let name = api.to_owned();
        let server = std::thread::spawn(move || {
            let (stream, request) = accept(&l);
            assert_eq!(request["stream"], true);
            assert!(request.get("tools").is_none());
            let mut body = String::new();
            if name == "anthropic" {
                body += &event(
                    json!({"type":"message_start","message":{"role":"assistant","stop_reason":null,"content":[]}}),
                );
                body += &event(
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                );
            }
            for _ in 0..2048 {
                body += &event(match name.as_str() {
                    "chat" => {
                        json!({"choices":[{"delta":{"content":fragment},"finish_reason":null}]})
                    }
                    "responses" => json!({"type":"response.output_text.delta","delta":fragment}),
                    _ => {
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":fragment}})
                    }
                });
            }
            match name.as_str() {
                "chat" => {
                    body += &chat("", json!("stop"));
                    body += "data: [DONE]\n\n";
                }
                "responses" => {
                    body += &event(
                        json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":full}]}]}}),
                    )
                }
                _ => {
                    body += &event(json!({"type":"content_block_stop","index":0}));
                    body +=
                        &event(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}));
                    body += &event(json!({"type":"message_stop"}));
                }
            }
            send(stream, &body);
        });
        let output = command()
            .args([
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "--no-tools",
                "--stream",
                "--plain",
                "repeat supplied response",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, format!("{expected}\n").as_bytes());
        assert!(output.stderr.is_empty());
        server.join().unwrap();
    }
}
