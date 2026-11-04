use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path, api: &str, url: &str, field: Option<&str>, limit: Option<&str>) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    for (name, _) in std::env::vars() {
        if name.starts_with("ZI_")
            || matches!(
                name.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(name);
        }
    }
    c.current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .env("NO_PROXY", "*")
        .args([
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--api",
            api,
            "--base-url",
            url,
            "--model",
            "fixture",
            "--timeout",
            "3",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(field) = field {
        c.args(["--chat-token-limit-field", field]);
    }
    if let Some(limit) = limit {
        c.args(["--max-output-tokens", limit]);
    }
    c
}
fn fixture() -> (tempfile::TempDir, TcpListener, String) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("config.toml"),
        "model='fixture'\nbase_url='http://localhost:1/v1'\napi_key='budget-key-secret'\n",
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    (root, l, url)
}
fn accept(l: &TcpListener) -> (TcpStream, Value) {
    let end = Instant::now() + Duration::from_secs(12);
    let stream = loop {
        match l.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end, "request timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                length = value.trim().parse::<usize>().unwrap();
            }
        }
    }
    assert!(length < 1_048_576);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (stream, serde_json::from_slice(&body).unwrap())
}
fn send(mut stream: TcpStream, status: u16, streaming: bool, body: &str) {
    let kind = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.flush().unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
}
fn budget(request: &Value, field: &str, limit: u32) {
    assert_eq!(request[field], limit);
    for other in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        if other != field {
            assert!(request.get(other).is_none(), "multiple budget fields");
        }
    }
}
fn variants() -> [(&'static str, Option<&'static str>, &'static str); 4] {
    [
        ("chat", None, "max_completion_tokens"),
        ("chat", Some("max_tokens"), "max_tokens"),
        ("responses", None, "max_output_tokens"),
        ("anthropic", None, "max_tokens"),
    ]
}
fn answer(api: &str, text: &str, truncated: bool) -> Value {
    match api {
        "responses" => {
            json!({"status":if truncated {"incomplete"} else {"completed"}, "output":[{"type":"message", "content":[{"type":"output_text", "text":text}]}]})
        }
        "anthropic" => {
            json!({"stop_reason":if truncated {"max_tokens"} else {"end_turn"}, "content":[{"type":"text", "text":text}]})
        }
        _ => {
            json!({"choices":[{"message":{"role":"assistant", "content":text}, "finish_reason":if truncated {"length"} else {"stop"}}]})
        }
    }
}
fn tool(api: &str) -> Value {
    match api {
        "responses" => {
            json!({"status":"completed", "output":[{"type":"function_call", "call_id":"c", "name":"count_input", "arguments":"{}"}]})
        }
        "anthropic" => {
            json!({"stop_reason":"tool_use", "content":[{"type":"tool_use", "id":"c", "name":"count_input", "input":{}}]})
        }
        _ => {
            json!({"choices":[{"message":{"role":"assistant", "content":null, "tool_calls":[{"id":"c", "type":"function", "function":{"name":"count_input", "arguments":"{}"}}]}, "finish_reason":"tool_calls"}]})
        }
    }
}
fn event(v: Value) -> String {
    format!("data: {v}\n\n")
}
fn stream_body(api: &str, tool_turn: bool, truncated: bool) -> String {
    match api {
        "responses" => {
            let result = if tool_turn {
                tool(api)
            } else {
                answer(api, "OK", truncated)
            };
            let prefix = if tool_turn {
                String::new()
            } else {
                event(json!({"type":"response.output_text.delta", "delta":"OK"}))
            };
            prefix
                + &event(
                    json!({"type":if truncated {"response.incomplete"} else {"response.completed"}, "response":result}),
                )
        }
        "anthropic" => {
            let start = if tool_turn {
                json!({"type":"tool_use", "id":"c", "name":"count_input", "input":{}})
            } else {
                json!({"type":"text", "text":""})
            };
            let delta = if tool_turn {
                json!({"type":"input_json_delta", "partial_json":"{}"})
            } else {
                json!({"type":"text_delta", "text":"OK"})
            };
            [json!({"type":"message_start", "message":{"role":"assistant", "content":[], "usage":{}}}),
             json!({"type":"content_block_start", "index":0, "content_block":start}),
             json!({"type":"content_block_delta", "index":0, "delta":delta}),
             json!({"type":"content_block_stop", "index":0}),
             json!({"type":"message_delta", "delta":{"stop_reason":if tool_turn {"tool_use"} else if truncated {"max_tokens"} else {"end_turn"}}}),
             json!({"type":"message_stop"})].into_iter().map(event).collect()
        }
        _ => {
            let delta = if tool_turn {
                json!({"tool_calls":[{"index":0, "id":"c", "type":"function", "function":{"name":"count_input", "arguments":"{}"}}]})
            } else {
                json!({"content":"OK"})
            };
            event(json!({"choices":[{"index":0, "delta":delta, "finish_reason":null}]}))
                + &event(
                    json!({"choices":[{"index":0, "delta":{}, "finish_reason":if tool_turn {"tool_calls"} else if truncated {"length"} else {"stop"}}]}),
                )
                + "data: [DONE]\n\n"
        }
    }
}

#[test]
fn output_limits_preserve_defaults_and_match_preview_with_actual_requests() {
    let cases = [
        ("chat", None, None, None),
        ("responses", None, None, None),
        ("anthropic", None, None, Some(("max_tokens", 4096))),
        ("chat", None, Some("1"), Some(("max_completion_tokens", 1))),
        (
            "chat",
            Some("max_completion_tokens"),
            Some("65536"),
            Some(("max_completion_tokens", 65536)),
        ),
        (
            "chat",
            Some("max_tokens"),
            Some("128"),
            Some(("max_tokens", 128)),
        ),
        (
            "responses",
            None,
            Some("16"),
            Some(("max_output_tokens", 16)),
        ),
        (
            "responses",
            None,
            Some("65536"),
            Some(("max_output_tokens", 65536)),
        ),
        ("anthropic", None, Some("1"), Some(("max_tokens", 1))),
        (
            "anthropic",
            None,
            Some("65536"),
            Some(("max_tokens", 65536)),
        ),
    ];
    for (api, field, limit, expected) in cases {
        let (root, l, url) = fixture();
        let preview = command(root.path(), api, &url, field, limit)
            .args(["--preview", "--no-tools", "--no-stream", "task"])
            .output()
            .unwrap();
        assert!(
            preview.status.success(),
            "{}",
            String::from_utf8_lossy(&preview.stderr)
        );
        let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
        assert_eq!(preview["network_requests"], 0);
        assert!(l.accept().is_err());
        let expected_body = preview["request"]["body"].clone();
        if let Some((field, n)) = expected {
            budget(&expected_body, field, n);
        } else {
            for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                assert!(expected_body.get(field).is_none());
            }
        }
        let protocol = api.to_owned();
        let worker = std::thread::spawn(move || {
            let (s, request) = accept(&l);
            assert_eq!(request, expected_body);
            send(s, 200, false, &answer(&protocol, "OK", false).to_string());
        });
        let result = command(root.path(), api, &url, field, limit)
            .args(["--no-tools", "--no-stream", "task"])
            .output()
            .unwrap();
        worker.join().unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, b"OK\n");
    }
}

#[test]
fn output_limits_survive_streamed_and_buffered_tool_followups() {
    for (api, field, wire_field) in variants() {
        for streaming in [false, true] {
            let (root, l, url) = fixture();
            let protocol = api.to_owned();
            let worker = std::thread::spawn(move || {
                for first in [true, false] {
                    let (s, request) = accept(&l);
                    budget(&request, wire_field, 128);
                    assert_eq!(request["stream"], streaming);
                    if !first {
                        assert!(
                            request.to_string().contains("tool_result")
                                || request.to_string().contains("function_call_output")
                                || request.to_string().contains("\"role\":\"tool\"")
                        );
                    }
                    let body = if streaming {
                        stream_body(&protocol, first, false)
                    } else if first {
                        tool(&protocol).to_string()
                    } else {
                        answer(&protocol, "OK", false).to_string()
                    };
                    send(s, 200, streaming, &body);
                }
            });
            let result = command(root.path(), api, &url, field, Some("128"))
                .args([if streaming { "--stream" } else { "--no-stream" }, "count"])
                .output()
                .unwrap();
            worker.join().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, b"OK\n");
        }
    }
}

#[test]
fn output_limits_retry_identically_and_never_guess_a_field_after_rejection() {
    for (api, field, wire_field) in variants() {
        for status in [429, 400] {
            let (root, l, url) = fixture();
            let extra = l.try_clone().unwrap();
            extra.set_nonblocking(true).unwrap();
            let protocol = api.to_owned();
            let worker = std::thread::spawn(move || {
                let (s, request) = accept(&l);
                budget(&request, wire_field, 128);
                send(s, status, false, "budget-key-secret");
                if status == 429 {
                    let (s, next) = accept(&l);
                    assert_eq!(request, next);
                    send(s, 200, false, &answer(&protocol, "OK", false).to_string());
                }
            });
            let result = command(root.path(), api, &url, field, Some("128"))
                .args(["--retries", "1", "--no-tools", "--no-stream", "task"])
                .output()
                .unwrap();
            worker.join().unwrap();
            assert_eq!(result.status.success(), status == 429);
            if status == 400 {
                assert!(result.stdout.is_empty());
                assert!(String::from_utf8_lossy(&result.stderr).contains("HTTP 400"));
            }
            assert!(!String::from_utf8_lossy(&result.stderr).contains("budget-key-secret"));
            assert!(extra.accept().is_err(), "unexpected fallback/retry request");
        }
    }
}

#[test]
fn output_limits_keep_truncation_failures_and_existing_file_protection() {
    for (api, field, wire_field) in variants() {
        for streaming in [false, true] {
            let (root, l, url) = fixture();
            let path = root.path().join("result.txt");
            std::fs::write(&path, "keep").unwrap();
            let protocol = api.to_owned();
            let worker = std::thread::spawn(move || {
                let (s, request) = accept(&l);
                budget(&request, wire_field, 128);
                let body = if streaming {
                    stream_body(&protocol, false, true)
                } else {
                    answer(&protocol, "OK", true).to_string()
                };
                send(s, 200, streaming, &body);
            });
            let mut c = command(root.path(), api, &url, field, Some("128"));
            c.arg("--no-tools");
            if streaming {
                c.arg("--stream");
            } else {
                c.args(["--no-stream", "-o", path.to_str().unwrap(), "--force"]);
            }
            let result = c.arg("task").output().unwrap();
            worker.join().unwrap();
            assert!(!result.status.success());
            if streaming {
                assert_eq!(result.stdout, b"OK\n");
            } else {
                assert!(result.stdout.is_empty());
            }
            assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        }
    }
}

#[test]
fn output_limit_validation_precedes_input_and_output_io() {
    let (root, l, url) = fixture();
    let path = root.path().join("keep.txt");
    std::fs::write(&path, "keep").unwrap();
    let cases: &[&[&str]] = &[
        &["--max-output-tokens", "0", "task"],
        &["--max-output-tokens", "65537", "task"],
        &[
            "--chat-token-limit-field",
            "guess",
            "--max-output-tokens",
            "128",
            "task",
        ],
        &["--chat-token-limit-field", "max_tokens", "task"],
        &[
            "--api",
            "responses",
            "--chat-token-limit-field",
            "max_tokens",
            "--max-output-tokens",
            "128",
            "task",
        ],
        &["--api", "responses", "--max-output-tokens", "15", "task"],
        &[
            "--api",
            "anthropic",
            "--chat-token-limit-field",
            "max_tokens",
            "--max-output-tokens",
            "128",
            "task",
        ],
        &["--max-output-tokens", "128", "--count"],
        &["--max-output-tokens", "128", "--config-example"],
        &["--max-output-tokens", "128", "data", "trim"],
        &["--max-output-tokens", "128", "doctor", "--probe"],
        &["--chat-token-limit-field", "max_tokens", "doctor"],
    ];
    for args in cases {
        // Keep stdin open: validation must not wait for any input, even with a
        // pre-existing output path. Root/config flags are explicit fixtures.
        let api_index = args.iter().position(|arg| *arg == "--api");
        let api = api_index.map_or("chat", |index| args[index + 1]);
        let filtered: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| api_index.is_none_or(|index| *i != index && *i != index + 1))
            .map(|(_, arg)| *arg)
            .collect();
        let mut child = command(root.path(), api, &url, None, None)
            .args(["-i", "-", "-o", path.to_str().unwrap(), "--force"])
            .args(&filtered)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= end {
                child.kill().unwrap();
                panic!("validation waited for stdin: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("max-output-tokens") || error.contains("chat-token-limit-field"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
    }
    // Also check protocol validation with no CLI --api: resolve the saved profile
    // first, before either reading the missing input or inspecting output files.
    std::fs::write(
        root.path().join("config.toml"),
        "model='fixture'\nbase_url='http://localhost:1/v1'\napi='responses'\n",
    )
    .unwrap();
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut saved = Command::new(binary);
    for (key, _) in std::env::vars() {
        if key.starts_with("ZI_") || key == "OPENAI_API_KEY" {
            saved.env_remove(key);
        }
    }
    let result = saved
        .args([
            "--config",
            root.path().join("config.toml").to_str().unwrap(),
            "--chat-token-limit-field",
            "max_tokens",
            "--max-output-tokens",
            "128",
            "-i",
            "must-not-read",
            "-o",
            path.to_str().unwrap(),
            "task",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("chat-token-limit-field"));
    assert!(l.accept().is_err());
}

#[test]
fn output_limits_allow_doctor_to_select_a_chat_field_with_its_fixed_budget() {
    for selector in [None, Some("max_completion_tokens"), Some("max_tokens")] {
        let (root, l, url) = fixture();
        let expected = selector.unwrap_or("max_tokens");
        let worker = std::thread::spawn(move || {
            let (s, request) = accept(&l);
            budget(&request, expected, 16);
            assert!(request.get("tools").is_none());
            assert!(request.to_string().len() < 1024);
            send(
                s,
                200,
                false,
                &answer("chat", "answer-secret", false).to_string(),
            );
        });
        let result = command(root.path(), "chat", &url, selector, None)
            .args(["doctor", "--probe"])
            .output()
            .unwrap();
        worker.join().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = String::from_utf8_lossy(&result.stdout);
        assert!(report.contains("Model probe: OK"));
        assert!(!report.contains("answer-secret"));
    }
}

#[test]
fn output_limits_do_not_replace_exact_native_results() {
    for (api, field, wire_field) in variants() {
        let (root, l, url) = fixture();
        std::fs::write(root.path().join("input.txt"), "one two").unwrap();
        let protocol = api.to_owned();
        let worker = std::thread::spawn(move || {
            let (s, request) = accept(&l);
            budget(&request, wire_field, 128);
            send(s, 200, false, &tool(&protocol).to_string());
        });
        let result = command(root.path(), api, &url, field, Some("128"))
            .args(["--exact", "-i", "input.txt", "count"])
            .output()
            .unwrap();
        worker.join().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let counts: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(counts["words"], 2);
    }
}

#[test]
fn saved_budgets_survive_streamed_tool_followups_and_transient_retries() {
    for (api, field, wire) in variants() {
        let (root, l, url) = fixture();
        let mut config =
            format!("model='fixture'\nbase_url='{url}'\napi='{api}'\nmax_output_tokens=128\n");
        if let Some(field) = field {
            config += &format!("chat_token_limit_field='{field}'\n");
        }
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        let protocol = api.to_owned();
        let worker = std::thread::spawn(move || {
            let (s, first) = accept(&l);
            budget(&first, wire, 128);
            send(s, 429, false, "hidden-provider-error");
            let (s, retried) = accept(&l);
            assert_eq!(first, retried);
            send(s, 200, true, &stream_body(&protocol, true, false));
            let (s, next) = accept(&l);
            budget(&next, wire, 128);
            assert_eq!(next["stream"], true);
            send(s, 200, true, &stream_body(&protocol, false, false));
        });
        let out = command(root.path(), api, &url, None, None)
            .args(["--stream", "--retries", "1", "count"])
            .output()
            .unwrap();
        worker.join().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"OK\n");
    }
}
