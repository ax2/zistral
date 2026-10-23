use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn command() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zi"));
    for name in [
        "ZI_MODEL",
        "ZI_API_KEY",
        "OPENAI_API_KEY",
        "ZI_BASE_URL",
        "ZI_API",
        "ZI_CONFIG",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env_remove(name);
    }
    // Isolate tests from the user's configuration.
    cmd.env("APPDATA", std::env::temp_dir().join("zistral-no-config"));
    cmd.env(
        "XDG_CONFIG_HOME",
        std::env::temp_dir().join("zistral-no-config"),
    );
    cmd.env("NO_PROXY", "*");
    cmd
}

fn invoke(args: &[&str], input: &[u8]) -> Output {
    let mut child = command()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn mock(responses: Vec<(u16, Value)>) -> (String, thread::JoinHandle<Vec<Value>>) {
    mock_auth(responses, None)
}

fn mock_auth(
    responses: Vec<(u16, Value)>,
    expected_key: Option<&str>,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    mock_header(
        responses,
        expected_key.map(|key| ("authorization".into(), format!("Bearer {key}"))),
        None,
    )
}

fn mock_header(
    responses: Vec<(u16, Value)>,
    expected_header: Option<(String, String)>,
    expected_path: Option<String>,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "mock server timed out");
                        thread::sleep(Duration::from_millis(10));
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
            let mut auth = String::new();
            let mut version = String::new();
            let mut first_line = true;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if first_line {
                    if let Some(path) = &expected_path {
                        assert_eq!(line.split_whitespace().nth(1), Some(path.as_str()));
                    }
                    first_line = false;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
                if expected_header
                    .as_ref()
                    .is_some_and(|(name, _)| line.to_lowercase().starts_with(&format!("{name}:")))
                {
                    auth = line.split_once(':').unwrap().1.trim().to_owned();
                }
                if line.to_lowercase().starts_with("anthropic-version:") {
                    version = line.split_once(':').unwrap().1.trim().to_owned();
                }
            }
            if let Some((name, expected)) = &expected_header {
                assert_eq!(&auth, expected);
                if name == "x-api-key" {
                    assert_eq!(version, "2023-06-01");
                }
            }
            let mut data = vec![0; length];
            reader.read_exact(&mut data).unwrap();
            requests.push(serde_json::from_slice(&data).unwrap());
            let text = body.to_string();
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
        }
        requests
    });
    (url, handle)
}

#[test]
fn anthropic_tool_roundtrip_with_saved_configuration() {
    let (url, server) = mock_header(
        vec![
            (
                200,
                json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":"c1","name":"count_input","input":{}}]}),
            ),
            (
                200,
                json!({"stop_reason":"end_turn","content":[{"type":"text","text":"2"}]}),
            ),
        ],
        Some(("x-api-key".into(), "anthropic-test-key".into())),
        Some("/plan/anthropic/v1/messages".into()),
    );
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let base = url.strip_suffix("/v1").unwrap();
    std::fs::write(&config, format!("base_url='{base}/plan/anthropic'\napi='anthropic'\nmodel='my-model'\napi_key='anthropic-test-key'\n")).unwrap();
    let out = invoke(
        &["--config", config.to_str().unwrap(), "count"],
        "你好".as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"2\n");
    let requests = server.join().unwrap();
    assert!(requests[0]["system"].is_string());
    assert_eq!(requests[0]["max_tokens"], 4096);
    assert!(requests[0]["tools"][0]["input_schema"].is_object());
    let tool: Value = serde_json::from_str(
        requests[1]["messages"][3]["content"][0]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tool["characters"], 2);
}

#[test]
fn custom_endpoint_is_exact_and_cli_base_override_clears_it() {
    let (url, server) = mock_header(
        vec![(200, answer("exact"))],
        None,
        Some("/v1/custom/infer".into()),
    );
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!("base_url='{url}'\nendpoint='{url}/custom/infer'\napi='chat'\nmodel='test'\n"),
    )
    .unwrap();
    let out = invoke(&["--config", config.to_str().unwrap(), "hello"], b"");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"exact\n");
    server.join().unwrap();
    let (other, server) = mock_header(
        vec![(200, answer("override"))],
        None,
        Some("/v1/chat/completions".into()),
    );
    let out = invoke(
        &[
            "--config",
            config.to_str().unwrap(),
            "--base-url",
            &other,
            "hello",
        ],
        b"",
    );
    assert!(out.status.success());
    server.join().unwrap();
}

#[test]
fn anthropic_truncation_never_emits_partial_output() {
    let (url, server) = mock(vec![(
        200,
        json!({"stop_reason":"max_tokens","content":[{"type":"text","text":"partial"}]}),
    )]);
    let out = invoke(
        &[
            "--base-url",
            &url,
            "--api",
            "anthropic",
            "--model",
            "test",
            "hello",
        ],
        b"",
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    server.join().unwrap();
}

#[test]
fn saved_configuration_works_without_environment_variables() {
    let (url, server) = mock_auth(vec![(200, answer("configured"))], Some("saved-test-key"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zistral/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "base_url = '{url}'\nmodel = 'saved-model'\napi = 'chat'\napi_key = 'saved-test-key'\n"
        ),
    )
    .unwrap();
    let out = command()
        .env("APPDATA", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .arg("hello")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"configured\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("saved-test-key"));
    assert_eq!(server.join().unwrap()[0]["model"], "saved-model");
}

#[test]
fn environment_key_overrides_saved_key() {
    let (url, server) = mock_auth(
        vec![(200, answer("configured"))],
        Some("environment-test-key"),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        format!("base_url = '{url}'\nmodel = 'saved-model'\napi_key = 'saved-test-key'\n"),
    )
    .unwrap();
    let out = command()
        .env("ZI_API_KEY", "environment-test-key")
        .args(["--config", path.to_str().unwrap(), "hello"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    server.join().unwrap();
}

#[test]
fn config_command_is_explicit_and_does_not_consume_pipeline_data() {
    let out = invoke(&["config"], b"not-a-secret\n");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("interactive terminal"));
    let out = invoke(&["config", "--help"], b"");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("zi config"));
}

fn answer(text: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":text}}]})
}
fn call(name: &str, args: &str) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":name,"arguments":args}}]}}]})
}

#[test]
fn offline_count_and_bom() {
    let out = invoke(&["--count"], "\u{feff}你好 e\u{301}\n".as_bytes());
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let count: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(count["characters"], 6);
    assert_eq!(count["cjk_characters"], 2);
}

#[test]
fn offline_select_join() {
    let out = invoke(
        &["--select", "/abc/0", "--join", ","],
        br#"{"abc":[["a","b"]]}"#,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a,b\n");
}

#[test]
fn rejects_bad_input_without_stdout() {
    for (args, data) in [
        (vec!["--count"], vec![0xff]),
        (vec!["--count", "--max-input", "2"], b"abc".to_vec()),
        (vec!["--select", "/missing"], b"{}".to_vec()),
        (vec!["--count", "extra prompt"], b"text".to_vec()),
    ] {
        let out = invoke(&args, &data);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}

#[test]
fn chat_tool_roundtrip_keeps_pipe_clean() {
    let (url, server) = mock(vec![
        (
            200,
            call("select_input", r#"{"pointer":"/abc/0","separator":"-"}"#),
        ),
        (200, answer("a-b")),
    ]);
    let out = invoke(
        &[
            "--base-url",
            &url,
            "--model",
            "test",
            "--verbose",
            "join",
            "first",
            "array",
        ],
        br#"{"abc":[["a","b"]]}"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"a-b\n");
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["messages"][2]["content"], "join first array");
    assert_eq!(requests[1]["messages"][4]["content"], "a-b");
    assert!(String::from_utf8_lossy(&out.stderr).contains("tool select_input"));
}

#[test]
fn responses_tool_roundtrip() {
    let (url, server) = mock(vec![
        (
            200,
            json!({"status":"completed","output":[{"type":"function_call","id":"fc_1","call_id":"c1","name":"count_input","arguments":"{}"}]}),
        ),
        (
            200,
            json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"2"}]}]}),
        ),
    ]);
    let out = invoke(
        &[
            "--base-url",
            &url,
            "--model",
            "test",
            "--api",
            "responses",
            "count characters",
        ],
        "你好".as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"2\n");
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["store"], false);
    assert_eq!(requests[0]["tools"][0]["name"], "count_input");
    let result: Value =
        serde_json::from_str(requests[1]["input"][4]["output"].as_str().unwrap()).unwrap();
    assert_eq!(result["characters"], 2);
}

#[test]
fn max_steps_stops_tool_loop() {
    let (url, server) = mock(vec![(200, call("count_input", "{}"))]);
    let out = invoke(
        &[
            "--base-url",
            &url,
            "--model",
            "test",
            "--max-steps",
            "1",
            "count",
        ],
        b"abc",
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("max-steps"));
    server.join().unwrap();
}

#[test]
fn api_error_does_not_echo_sensitive_body() {
    let (url, server) = mock(vec![(401, json!({"error":"secret-input-and-token"}))]);
    let out = invoke(&["--base-url", &url, "--model", "test", "count"], b"abc");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("401"));
    assert!(!stderr.contains("secret"));
    server.join().unwrap();
}

#[test]
fn invalid_json_output_is_atomic() {
    let (url, server) = mock(vec![(200, answer("```json\n{}\n```"))]);
    let out = invoke(
        &["--base-url", &url, "--model", "test", "--json", "transform"],
        b"abc",
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    server.join().unwrap();
}

#[test]
fn text_only_mode_and_json_output() {
    let (url, server) = mock(vec![(200, answer("{ \"ok\": true }"))]);
    let out = invoke(
        &[
            "--base-url",
            &url,
            "--model",
            "test",
            "--no-tools",
            "--json",
            "transform",
        ],
        b"abc",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"{\"ok\":true}\n");
    assert!(server.join().unwrap()[0].get("tools").is_none());
}

#[test]
fn incomplete_output_is_not_success() {
    let (url, server) = mock(vec![(
        200,
        json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
    )]);
    let out = invoke(
        &["--base-url", &url, "--model", "test", "transform"],
        b"abc",
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    server.join().unwrap();
}
