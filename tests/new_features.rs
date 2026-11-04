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
    let (stream, body, _) = accept_with_headers(listener);
    (stream, body)
}

fn accept_with_headers(listener: &TcpListener) -> (TcpStream, Value, Vec<(String, String)>) {
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
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap();
            }
        }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    (stream, serde_json::from_slice(&body).unwrap(), headers)
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
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
    // A large queued response must finish before the fixture closes both halves.
    // Send FIN after the body, then wait for the client to close (bounded by the
    // read timeout set in accept). Some protocol-complete clients reset a socket
    // with unread trailing events; output/error assertions remain on the client.
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut ignored = Vec::new();
    let _ = stream.read_to_end(&mut ignored);
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
fn doctor_credential_source_matches_actual_authentication() {
    const CUSTOM: &str = "ZISTRAL_DIAG_TEST_KEY";
    let cases = [
        (
            Some(CUSTOM),
            Some("saved-secret"),
            Some("custom-secret"),
            Some("unused-openai-secret"),
            None,
            Some("custom-secret"),
            "environment variable \"ZISTRAL_DIAG_TEST_KEY\" (value hidden)",
        ),
        (
            Some(CUSTOM),
            Some("saved-secret"),
            Some("  "),
            Some("unused-openai-secret"),
            None,
            Some("saved-secret"),
            "saved profile (value hidden)",
        ),
        (
            Some(CUSTOM),
            Some("saved-secret"),
            Some("custom-secret"),
            Some("unused-openai-secret"),
            Some("zi-secret"),
            Some("zi-secret"),
            "environment variable \"ZI_API_KEY\" (value hidden)",
        ),
        (
            Some(CUSTOM),
            Some("saved-secret"),
            Some("custom-secret"),
            None,
            Some(" \t"),
            Some("custom-secret"),
            "environment variable \"ZISTRAL_DIAG_TEST_KEY\" (value hidden)",
        ),
        (
            None,
            Some("saved-secret"),
            None,
            Some("openai-secret"),
            None,
            Some("openai-secret"),
            "environment variable \"OPENAI_API_KEY\" (value hidden)",
        ),
        (
            Some(CUSTOM),
            None,
            None,
            Some("unused-openai-secret"),
            None,
            None,
            "not configured",
        ),
        (
            None,
            Some("  "),
            None,
            Some(" \t"),
            Some("  "),
            None,
            "not configured",
        ),
        (
            Some("ZI_API_KEY"),
            Some("saved-secret"),
            None,
            Some("unused-openai-secret"),
            Some("zi-secret"),
            Some("zi-secret"),
            "environment variable \"ZI_API_KEY\" (value hidden)",
        ),
    ];
    for api in ["chat", "responses", "anthropic"] {
        for (key_env, saved, custom, openai, zi, expected_key, source) in cases {
            let (l, url) = listener();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            let mut config = format!("model='fixture'\nbase_url='{url}'\napi='{api}'\n");
            if let Some(name) = key_env {
                config += &format!("api_key_env='{name}'\n");
            }
            if let Some(key) = saved {
                config += &format!("api_key='{key}'\n");
            }
            std::fs::write(&path, &config).unwrap();
            let protocol = api.to_owned();
            let server = std::thread::spawn(move || {
                let (mut stream, _, headers) = accept_with_headers(&l);
                let auth = headers.iter().find(|(name, _)| {
                    name == if protocol == "anthropic" {
                        "x-api-key"
                    } else {
                        "authorization"
                    }
                });
                match expected_key {
                    Some(key) => {
                        let expected = if protocol == "anthropic" {
                            key.into()
                        } else {
                            format!("Bearer {key}")
                        };
                        assert!(
                            auth.is_some_and(|(_, value)| value == &expected),
                            "actual authentication precedence changed"
                        );
                    }
                    None => assert!(auth.is_none(), "unexpected authentication header"),
                }
                let body = match protocol.as_str() {
                    "anthropic" => json!({"stop_reason":"end_turn", "content":[{"type":"text", "text":"OK"}]}),
                    "responses" => json!({"status":"completed", "output":[{"type":"message", "content":[{"type":"output_text", "text":"OK"}]}]}),
                    _ => json!({"choices":[{"message":{"content":"OK"}, "finish_reason":"stop"}]}),
                }.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let configured = || {
                let mut c = command();
                c.env_remove(CUSTOM);
                for (name, value) in [
                    (CUSTOM, custom),
                    ("OPENAI_API_KEY", openai),
                    ("ZI_API_KEY", zi),
                ] {
                    if let Some(value) = value {
                        c.env(name, value);
                    }
                }
                c.args(["--config", path.to_str().unwrap()]);
                c
            };
            let actual = configured()
                .args(["--no-tools", "--no-stream", "Reply with OK."])
                .output()
                .unwrap();
            server.join().unwrap();
            assert!(actual.status.success());
            assert_eq!(actual.stdout, b"OK\n");
            let doctor = configured().arg("doctor").output().unwrap();
            assert!(doctor.status.success());
            let report = String::from_utf8(doctor.stdout).unwrap();
            assert!(
                report.contains(&format!("API Key source: {source}")),
                "missing effective credential source: {report}"
            );
            assert!(!report.contains("secret"));
            if !source.contains("OPENAI_API_KEY") {
                assert!(!report.contains("OPENAI_API_KEY"));
            }
            if !source.contains(CUSTOM) {
                assert!(!report.contains(CUSTOM));
            }
            assert!(!report.contains("Environment overrides:"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), config);
        }
    }
}

#[test]
fn doctor_reports_profile_and_environment_presence_without_claiming_cli_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let config = "model='saved-model'\nbase_url='http://localhost:1/v1'\napi_key='saved-secret'\nactive_profile='work'\n[profiles.work]\nmodel='work-model'\nbase_url='http://localhost:2/v1'\napi_key_env='ZISTRAL_DIAG_TEST_KEY'\n";
    std::fs::write(&path, config).unwrap();
    for (args, env_profile, expected_profile, expected_model, key_source, custom_present) in [
        (
            vec![],
            None,
            "work",
            "work-model",
            "environment variable \"ZISTRAL_DIAG_TEST_KEY\" (value hidden)",
            true,
        ),
        (
            vec![],
            Some("default"),
            "default",
            "saved-model",
            "saved profile (value hidden)",
            true,
        ),
        (
            vec![
                "--profile",
                "default",
                "--model",
                "cli-model",
                "--api",
                "chat",
                "--base-url",
                "http://localhost:3/v1",
            ],
            Some("work"),
            "default",
            "cli-model",
            "saved profile (value hidden)",
            true,
        ),
        (vec![], None, "work", "work-model", "not configured", false),
    ] {
        let mut c = command();
        c.env_remove("ZISTRAL_DIAG_TEST_KEY");
        if custom_present {
            c.env("ZISTRAL_DIAG_TEST_KEY", "custom-secret");
        } else {
            c.env("OPENAI_API_KEY", "unused-openai-secret");
        }
        if let Some(profile) = env_profile {
            c.env("ZI_PROFILE", profile);
        }
        if args.contains(&"--model") {
            c.env("ZI_MODEL", "unused-env-model")
                .env("ZI_API", "responses")
                .env("ZI_BASE_URL", "https://unused.example/v1");
        }
        let result = c
            .env("ZI_CONFIG", dir.path().join("must-not-read.toml"))
            .args(["--config", path.to_str().unwrap()])
            .args(&args)
            .arg("doctor")
            .output()
            .unwrap();
        assert!(result.status.success());
        let report = String::from_utf8(result.stdout).unwrap();
        assert!(report.contains(&format!("Profile: {expected_profile}\n")));
        assert!(report.contains(&format!("Model: {expected_model}\n")));
        assert!(report.contains(&format!("API Key source: {key_source}")));
        assert!(report.contains("Environment settings present (CLI flags may override):"));
        assert!(report.contains("ZI_CONFIG"));
        assert!(!report.contains("Environment overrides:"));
        assert!(!report.contains("unused-env-model"));
        assert!(!report.contains("secret"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), config);
    }
}

#[test]
fn doctor_escapes_control_characters_in_credential_environment_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for (name, encoded) in [
        ("ZISTRAL_DIAG\nKEY", r#""ZISTRAL_DIAG\nKEY""#),
        ("ZISTRAL_DIAG\x1b[2JKEY", r#""ZISTRAL_DIAG\u001b[2JKEY""#),
        ("ZISTRAL_DIAG\u{009b}2JKEY", r#""ZISTRAL_DIAG\u009b2JKEY""#),
        ("ZISTRAL_DIAG\u{007f}KEY", r#""ZISTRAL_DIAG\u007fKEY""#),
        ("ZISTRAL_中\"KEY", r#""ZISTRAL_中\"KEY""#),
    ] {
        std::fs::write(
            &path,
            format!("model='fixture'\nbase_url='http://localhost:1/v1'\napi_key_env={encoded}\n"),
        )
        .unwrap();
        let result = command()
            .env(name, "control-name-secret")
            .args(["--config", path.to_str().unwrap(), "doctor"])
            .output()
            .unwrap();
        assert!(result.status.success());
        let report = String::from_utf8(result.stdout).unwrap();
        assert!(report.contains(&format!(
            "API Key source: environment variable {encoded} (value hidden)"
        )));
        if name.chars().any(char::is_control) {
            assert!(!report.contains(name));
        }
        assert!(!report.contains("control-name-secret"));
    }
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
fn doctor_probe_is_minimal_and_bounded_in_each_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let (l, url) = listener();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config =
            format!("model='fixture'\nbase_url='{url}'\napi='{api}'\napi_key='probe-key-secret'\n");
        std::fs::write(&path, &config).unwrap();
        let protocol = api.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, request) = accept(&l);
            assert_eq!(request["model"], "fixture");
            assert_eq!(request["stream"], false);
            assert!(request.get("tools").is_none());
            assert!(
                request.to_string().len() < 1024,
                "probe contains task instructions"
            );
            let messages = &request[if protocol == "responses" {
                "input"
            } else {
                "messages"
            }];
            assert_eq!(
                messages.as_array().unwrap().len(),
                if protocol == "anthropic" { 1 } else { 2 }
            );
            assert_eq!(
                messages.as_array().unwrap().last().unwrap()["content"],
                "Reply with OK."
            );
            assert_eq!(
                request[if protocol == "responses" {
                    "max_output_tokens"
                } else {
                    "max_tokens"
                }],
                16
            );
            if protocol == "responses" {
                assert_eq!(request["store"], false);
            }
            let body = match protocol.as_str() {
                "anthropic" => json!({"stop_reason":"end_turn", "content":[{"type":"text", "text":"probe-answer-secret"}]}),
                "responses" => json!({"status":"completed", "output":[{"type":"message", "status":"completed", "role":"assistant", "content":[{"type":"output_text", "text":"probe-answer-secret"}]}]}),
                _ => json!({"choices":[{"message":{"role":"assistant", "content":"probe-answer-secret"}, "finish_reason":"stop"}]}),
            }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let result = command()
            .args(["doctor", "--probe", "--config", path.to_str().unwrap()])
            .output()
            .unwrap();
        server.join().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(output.contains("Model probe: OK"));
        assert!(!output.contains("probe-key-secret"));
        assert!(!output.contains("probe-answer-secret"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), config);
    }
}

#[test]
fn doctor_probe_does_not_follow_redirects_or_retry_errors() {
    for status in [302, 403, 429] {
        let (l, url) = listener();
        let first = l.try_clone().unwrap();
        let (redirected, redirect_url) = listener();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!("model='fixture'\nbase_url='{url}'\napi_key='probe-key-secret'\n"),
        )
        .unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = accept(&first);
            let body = "probe-key-secret response-body-secret";
            write!(stream, "HTTP/1.1 {status} Fixture\r\nLocation: {redirect_url}/redirected\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let result = command()
            .args(["doctor", "--probe", "--config", path.to_str().unwrap()])
            .output()
            .unwrap();
        server.join().unwrap();
        assert!(!result.status.success());
        assert!(l.accept().is_err(), "probe retried a denied request");
        assert!(redirected.accept().is_err(), "probe followed a redirect");
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(output.contains(&format!("HTTP {status}")));
        assert!(!output.contains("probe-key-secret"));
        assert!(!output.contains("response-body-secret"));
        assert!(!output.contains("Model probe: OK"));
        if status == 403 {
            assert!(output.contains("model/account permissions"));
            assert!(output.contains("proxy"));
        }
    }
}

#[test]
fn doctor_probe_does_not_report_truncated_answers_as_healthy() {
    for api in ["chat", "responses", "anthropic"] {
        let (l, url) = listener();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            format!("model='fixture'\nbase_url='{url}'\napi='{api}'\n"),
        )
        .unwrap();
        let protocol = api.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = accept(&l);
            let body = match protocol.as_str() {
                "anthropic" => json!({"stop_reason":"max_tokens", "content":[{"type":"text", "text":"partial-answer-secret"}]}),
                "responses" => json!({"status":"incomplete", "incomplete_details":{"reason":"max_output_tokens"}, "output":[{"type":"message", "content":[{"type":"output_text", "text":"partial-answer-secret"}]}]}),
                _ => json!({"choices":[{"message":{"role":"assistant", "content":"partial-answer-secret"}, "finish_reason":"length"}]}),
            }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let result = command()
            .args(["doctor", "--probe", "--config", path.to_str().unwrap()])
            .output()
            .unwrap();
        server.join().unwrap();
        assert!(!result.status.success());
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!output.contains("partial-answer-secret"));
        assert!(!output.contains("Model probe: OK"));
    }
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
