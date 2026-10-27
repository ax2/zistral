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
fn invoke(root: &Path, api: &str, url: &str, options: &[&str], input: &[u8]) -> Output {
    let mut child = command(root)
        .args([
            "--api",
            api,
            "--base-url",
            url,
            "--model",
            "fixture",
            "--plain",
            "--verbose",
            "--exact",
            "--timeout",
            "3",
        ])
        .args(options)
        .arg("task")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}
fn call(api: &str, id: &str, name: &str, args: Value) -> Value {
    match api {
        "chat" => {
            json!({"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})
        }
        "responses" => {
            json!({"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()})
        }
        _ => json!({"type":"tool_use","id":id,"name":name,"input":args}),
    }
}
type Reply = (String, &'static str);
fn calls(api: &str, items: Vec<Value>) -> Reply {
    let body = match api {
        "chat" => {
            json!({"choices":[{"finish_reason":"tool_calls","message":{"content":"MODEL_TEXT_IS_NOT_NATIVE_OUTPUT","tool_calls":items}}]})
        }
        "responses" => json!({"status":"completed","output":items}),
        _ => json!({"stop_reason":"tool_use","content":items}),
    };
    (body.to_string(), "application/json")
}
fn server(api: &str, replies: Vec<Reply>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let api = api.to_owned();
    let task = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (body, kind) in replies {
            let end = Instant::now() + Duration::from_secs(15);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "missing fixture request");
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
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        size = v.trim().parse().unwrap();
                    }
                }
            }
            let mut bytes = vec![0; size];
            reader.read_exact(&mut bytes).unwrap();
            let req: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(req["stream"], false);
            assert_eq!(req["tools"].as_array().unwrap().len(), 3);
            let system = if api == "anthropic" {
                req["system"].as_str().unwrap()
            } else if api == "responses" {
                req["input"][0]["content"].as_str().unwrap()
            } else {
                req["messages"][0]["content"].as_str().unwrap()
            };
            assert!(system.contains("Exact mode:"));
            requests.push(req);
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (url, task)
}

#[test]
fn exact_native_results_preserve_spaces_tokens_error_shaped_values_and_empty_output() {
    for api in ["chat", "responses", "anthropic"] {
        for (name, args, input, expected, json_output) in [
            (
                "transform_input",
                json!({"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-tail","lines":2}]}),
                "INFO boot\n ERROR old \nERROR end\n ERROR repeat \n",
                "ERROR end\n ERROR repeat \n",
                false,
            ),
            (
                "select_input",
                json!({"pointer":"/x"}),
                r#"{"x":1.2300E+400}"#,
                "1.2300E+400\n",
                true,
            ),
            (
                "select_input",
                json!({"pointer":"/x"}),
                r#"{"x":{"error":"data","n":1.2300E+400}}"#,
                "{\"error\":\"data\",\"n\":1.2300E+400}\n",
                false,
            ),
            (
                "select_input",
                json!({"pointer":"/x","separator":"-"}),
                r#"{"x":["甲","乙","丙"]}"#,
                "甲-乙-丙\n",
                false,
            ),
            (
                "transform_input",
                json!({"operation":"lines-head","lines":1}),
                "",
                "",
                false,
            ),
        ] {
            for file in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (url, task) = server(
                    api,
                    vec![calls(api, vec![call(api, "one", name, args.clone())])],
                );
                let mut options = vec!["--max-steps", "1"];
                if json_output {
                    options.push("--json");
                }
                if file {
                    options.extend(["-o", "result.txt"]);
                }
                let out = invoke(root.path(), api, &url, &options, input.as_bytes());
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                if file {
                    assert!(out.stdout.is_empty());
                    assert_eq!(
                        std::fs::read(root.path().join("result.txt")).unwrap(),
                        expected.as_bytes()
                    );
                } else {
                    assert_eq!(out.stdout, expected.as_bytes());
                }
                assert!(
                    !String::from_utf8_lossy(&out.stdout)
                        .contains("MODEL_TEXT_IS_NOT_NATIVE_OUTPUT")
                );
                assert_eq!(task.join().unwrap().len(), 1);
            }
        }
        let root = tempfile::tempdir().unwrap();
        let (url, task) = server(
            api,
            vec![calls(
                api,
                vec![call(api, "count", "count_input", json!({}))],
            )],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["--json", "--max-steps", "1"],
            "你好 e\u{301}\n".as_bytes(),
        );
        assert!(out.status.success());
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["characters"], 6);
        assert_eq!(value["graphemes"], 5);
        assert_eq!(value["cjk_characters"], 2);
        assert_eq!(task.join().unwrap().len(), 1);
    }
}

#[test]
fn exact_native_argument_errors_recover_on_original_input_without_final_rewrite() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let (url, task) = server(
            api,
            vec![
                calls(
                    api,
                    vec![call(
                        api,
                        "bad",
                        "transform_input",
                        json!({"operation":"lines-tail","lines":0}),
                    )],
                ),
                calls(
                    api,
                    vec![call(
                        api,
                        "fixed",
                        "transform_input",
                        json!({"operation":"lines-tail","lines":1}),
                    )],
                ),
            ],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["--max-steps", "2"],
            b"old\n last \n",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b" last \n");
        let requests = task.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].to_string().contains("lines must be an integer"));
    }
}

#[test]
fn exact_json_result_validation_can_recover_with_a_json_preserving_tool() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let (url, task) = server(
            api,
            vec![
                calls(
                    api,
                    vec![call(
                        api,
                        "decoded",
                        "select_input",
                        json!({"pointer":"/s"}),
                    )],
                ),
                calls(
                    api,
                    vec![call(
                        api,
                        "quoted",
                        "transform_input",
                        json!({"operation":"json-select","pointer":"/s"}),
                    )],
                ),
            ],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["--json", "--max-steps", "2"],
            br#"{"s":"\u4e2d","n":1.2300E+400}"#,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"\"\\u4e2d\"\n");
        let requests = task.join().unwrap();
        assert!(
            requests[1]
                .to_string()
                .contains("native result is not JSON")
        );
    }
}

#[test]
fn exact_rejects_text_batches_nondata_and_incomplete_replies_before_publication() {
    for api in ["chat", "responses", "anthropic"] {
        let text = match api {
            "chat" => {
                json!({"choices":[{"finish_reason":"stop","message":{"content":"NOT_NATIVE"}}]})
            }
            "responses" => {
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"NOT_NATIVE"}]}]})
            }
            _ => json!({"stop_reason":"end_turn","content":[{"type":"text","text":"NOT_NATIVE"}]}),
        };
        let incomplete = match api {
            "chat" => {
                json!({"choices":[{"finish_reason":"length","message":{"tool_calls":[call(api,"one","count_input",json!({}))]}}]})
            }
            "responses" => {
                json!({"status":"incomplete","output":[call(api,"one","count_input",json!({}))]})
            }
            _ => {
                json!({"stop_reason":"max_tokens","content":[call(api,"one","count_input",json!({}))]})
            }
        };
        for reply in [
            (text.to_string(), "application/json"),
            calls(
                api,
                vec![
                    call(api, "one", "count_input", json!({})),
                    call(api, "two", "count_input", json!({})),
                ],
            ),
            calls(
                api,
                vec![call(
                    api,
                    "write",
                    "write_file",
                    json!({"path":"unexpected.txt","content":"bad"}),
                )],
            ),
            (incomplete.to_string(), "application/json"),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
            let (url, task) = server(api, vec![reply]);
            let out = invoke(
                root.path(),
                api,
                &url,
                &["-o", "keep.txt", "--force"],
                b"PRIVATE_INPUT",
            );
            assert!(!out.status.success() && out.stdout.is_empty());
            assert_eq!(
                std::fs::read(root.path().join("keep.txt")).unwrap(),
                b"keep"
            );
            assert!(!root.path().join("unexpected.txt").exists());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("zi: tool "));
            task.join().unwrap();
        }
    }
}

#[test]
fn exact_exhausted_native_failures_emit_no_result_or_file_prefix() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
        let (url, task) = server(
            api,
            vec![
                calls(
                    api,
                    vec![call(
                        api,
                        "one",
                        "transform_input",
                        json!({"operation":"lines-tail","lines":0}),
                    )],
                ),
                calls(
                    api,
                    vec![call(
                        api,
                        "two",
                        "transform_input",
                        json!({"operation":"lines-tail","lines":0}),
                    )],
                ),
            ],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["--max-steps", "2", "-o", "keep.txt", "--force"],
            b"secret",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("reached --max-steps"));
        assert_eq!(
            std::fs::read(root.path().join("keep.txt")).unwrap(),
            b"keep"
        );
        assert_eq!(task.join().unwrap().len(), 2);
    }
}

#[test]
fn exact_conflicting_modes_fail_before_input_configuration_and_output_access() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("invalid.toml"), b"not configuration").unwrap();
    std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
    for options in [
        vec!["--no-tools"],
        vec!["--stream"],
        vec!["--agent"],
        vec!["--cwd", "missing-project"],
        vec!["--read-only"],
        vec!["--no-exec"],
        vec!["--count"],
        vec!["--select", ""],
        vec!["doctor"],
        vec!["data", "trim"],
        vec!["models"],
    ] {
        let out = command(root.path())
            .args([
                "--exact",
                "--config",
                "invalid.toml",
                "-i",
                "missing-input",
                "-o",
                "keep.txt",
                "--force",
            ])
            .args(options)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            !error.contains("cannot open missing-input")
                && !error.contains("cannot read configuration")
        );
        assert_eq!(
            std::fs::read(root.path().join("keep.txt")).unwrap(),
            b"keep"
        );
    }
}

#[test]
fn exact_unrequested_streams_never_emit_partial_output_or_replace_files() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
        let (url, task) = server(
            api,
            vec![(
                "data: PARTIAL_NATIVE_SENTINEL\n\n".into(),
                "text/event-stream",
            )],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["-o", "keep.txt", "--force"],
            b"input",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PARTIAL_NATIVE_SENTINEL"));
        assert_eq!(
            std::fs::read(root.path().join("keep.txt")).unwrap(),
            b"keep"
        );
        task.join().unwrap();
    }
}
