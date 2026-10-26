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

#[test]
fn jsonl_conversions_work_offline_with_recipes_files_and_real_pipes() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("broken-config.toml"), "not model configuration").unwrap();
    let original = "\u{feff}{\"n\":123456789012345678901234567890,\"n\":-0}\r\n[1,2]\n\"a,b\"";
    std::fs::write(root.join("records.jsonl"), original).unwrap();
    let out = invoke(
        root,
        &[
            "--config",
            "broken-config.toml",
            "data",
            "jsonl-to-array",
            "-i",
            "records.jsonl",
            "-o",
            "array.json",
        ],
        b"",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        std::fs::read(root.join("records.jsonl")).unwrap(),
        original.as_bytes()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("array.json")).unwrap(),
        "[{\"n\":123456789012345678901234567890,\"n\":-0},[1,2],\"a,b\"]\n"
    );
    std::fs::write(
        root.join("lines.toml"),
        "version=1\n[[steps]]\noperation='json-minify'\n[[steps]]\noperation='array-to-jsonl'\n",
    )
    .unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--recipe",
            "lines.toml",
            "-i",
            "array.json",
            "-o",
            "records.jsonl",
            "--force",
        ],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("records.jsonl")).unwrap(),
        "{\"n\":123456789012345678901234567890,\"n\":-0}\n[1,2]\n\"a,b\"\n"
    );
    let mut first = command(root)
        .args(["data", "array-to-jsonl", "-i", "array.json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .args(["data", "jsonl-to-array"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = second.wait_with_output().unwrap();
    assert!(first.wait().unwrap().success() && out.status.success());
    assert_eq!(out.stdout, std::fs::read(root.join("array.json")).unwrap());
    for (op, input, expected) in [
        ("jsonl-to-array", b"".as_slice(), b"[]\n".as_slice()),
        ("array-to-jsonl", b"[]", b""),
    ] {
        let out = invoke(root, &["data", op], input);
        assert!(out.status.success());
        assert_eq!(out.stdout, expected);
    }
    assert!(!root.join("config").exists());
}

#[test]
fn later_invalid_jsonl_record_never_publishes_partial_records_or_changes_target() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("result.json"), b"old result\r\n").unwrap();
    for input in [
        b"{}\nsecret invalid record".as_slice(),
        b"{}\n\n",
        b"{\n\"x\":1\n}",
        b"{}\n\xef\xbb\xbf{}",
    ] {
        for save in [false, true] {
            let mut args = vec!["data", "jsonl-to-array"];
            if save {
                args.extend(["-o", "result.json", "--force"]);
            }
            let out = invoke(root, &args, input);
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("secret invalid"));
            assert_eq!(
                std::fs::read(root.join("result.json")).unwrap(),
                b"old result\r\n"
            );
            assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
        }
    }
    let out = invoke(root, &["data", "jsonl-to-array"], b"{}\nsecret");
    assert!(String::from_utf8_lossy(&out.stderr).contains("record 2"));
    for op in ["jsonl-to-array", "array-to-jsonl"] {
        let out = invoke(root, &["data", op, "--reverse"], b"[]");
        assert!(!out.status.success() && out.stdout.is_empty());
    }
    let out = invoke(
        root,
        &[
            "data",
            "--step",
            "array-to-jsonl",
            "--step",
            "jsonl-to-array",
        ],
        b"[{},{}]",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"[{},{}]\n");
}

fn model(api: &str, args: Value, expected: Value) -> (String, std::thread::JoinHandle<()>) {
    model_turns(api, vec![(args, expected)])
}

fn model_turns(api: &str, calls: Vec<(Value, Value)>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let api = api.to_owned();
    let task = std::thread::spawn(move || {
        for turn in 0..=calls.len() {
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
                let schema = match api.as_str() {
                    "chat" => &request["tools"][2]["function"]["parameters"],
                    "responses" => &request["tools"][2]["parameters"],
                    _ => &request["tools"][2]["input_schema"],
                };
                let ops = schema["properties"]["operation"]["enum"]
                    .as_array()
                    .unwrap();
                assert!(ops.contains(&json!("jsonl-to-array")));
                assert!(ops.contains(&json!("array-to-jsonl")));
                assert!(ops.contains(&json!("json-select")));
                assert_eq!(schema["properties"]["pointer"]["type"], "string");
                let steps = &schema["properties"]["steps"];
                assert_eq!(steps["minItems"], 1);
                assert_eq!(steps["maxItems"], 16);
                assert_eq!(
                    steps["items"]["properties"]["operation"]["enum"]
                        .as_array()
                        .unwrap(),
                    ops
                );
                assert_eq!(steps["items"]["additionalProperties"], false);
                assert_eq!(steps["items"]["properties"]["pointer"]["type"], "string");
            }
            if turn > 0 {
                let expected = &calls[turn - 1].1;
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
                    assert!(result.get("output").is_none());
                    assert!(!result["error"].as_str().unwrap().contains("PRIVATE"));
                } else {
                    assert_eq!(&result["output"], expected)
                }
            }
            let payload=if let Some((args,_))=calls.get(turn) { let id=format!("t{turn}"); match api.as_str(){
                "chat"=>json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":id,"type":"function","function":{"name":"transform_input","arguments":args.to_string()}}]}}]}),
                "responses"=>json!({"status":"completed","output":[{"type":"function_call","call_id":id,"name":"transform_input","arguments":args.to_string()}]}),
                _=>json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":id,"name":"transform_input","input":args}]}),
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
fn tool_pipelines_compose_in_order_and_keep_lexical_json_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        for (args, input, expected) in [
            (
                json!({"steps":[{"operation":"trim"},{"operation":"dedupe"},{"operation":"sort","reverse":true}]}),
                " 乙 \r\n甲\n 乙\n\n",
                json!("甲\n乙\n\n"),
            ),
            (
                json!({"steps":[{"operation":"dedupe"},{"operation":"trim"}]}),
                "a\na \n",
                json!("a\na\n"),
            ),
            (
                json!({"steps":[{"operation":"jsonl-to-array"},{"operation":"json-format"},{"operation":"json-minify"},{"operation":"array-to-jsonl"}]}),
                "{\"n\":1.2300E+400,\"n\":-0}\n123456789012345678901234567890\n\"\\u4e2d\"\n",
                json!(
                    "{\"n\":1.2300E+400,\"n\":-0}\n123456789012345678901234567890\n\"\\u4e2d\"\n"
                ),
            ),
        ] {
            let (url, server) = model(api, args, expected);
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "transform",
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
            server.join().unwrap();
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn json_selection_steps_compose_and_reject_invalid_options_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        for (args, input, expected) in [
            (
                json!({"steps":[{"operation":"jsonl-to-array"},{"operation":"json-select","pointer":"/1/data"},{"operation":"array-to-jsonl"}]}),
                "null\n{\"data\":[{\"n\":1.2300E+400,\"n\":-0},123456789012345678901234567890,\"\\u4e2d\"]}\n",
                json!(
                    "{\"n\":1.2300E+400,\"n\":-0}\n123456789012345678901234567890\n\"\\u4e2d\"\n"
                ),
            ),
            (
                json!({"operation":"json-select","pointer":"/x"}),
                r#"{"x":"\u4e2d"}"#,
                json!("\"\\u4e2d\""),
            ),
            (
                json!({"steps":[{"operation":"json-select","pointer":"/x"},{"operation":"array-to-jsonl"}]}),
                r#"{"x":"PRIVATE_VALUE"}"#,
                Value::Null,
            ),
            (
                json!({"operation":"json-select","pointer":"/x"}),
                r#"{"x":1,"x":2}"#,
                Value::Null,
            ),
            (
                json!({"operation":"json-select","pointer":null}),
                "{}",
                Value::Null,
            ),
            (
                json!({"operation":"trim","pointer":""}),
                "PRIVATE_INPUT",
                Value::Null,
            ),
        ] {
            let (url, server) = model(api, args, expected);
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "transform nested data",
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
            server.join().unwrap();
        }
        assert!(!root.path().join("config").exists());
    }
}
#[test]
fn tool_pipeline_invalid_shapes_and_midstep_failures_return_no_intermediate() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        for args in [
            json!({"steps":[]}),
            json!({"steps":vec![json!({"operation":"trim"});17]}),
            json!({"operation":"trim","steps":[{"operation":"dedupe"}]}),
            json!({"steps":[{"operation":"trim"}],"reverse":false}),
            json!({"steps":[{"operation":"trim"},{"operation":"dedupe","reverse":true}]}),
            json!({"steps":[{"operation":"trim","path":"PRIVATE_PATH"}]}),
            json!({"steps":[{"operation":"trim"},{"operation":"json-minify"}]}),
        ] {
            let (url, server) = model(api, args, Value::Null);
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "transform",
                ],
                b" PRIVATE_INPUT \n",
            );
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"verified\n");
            assert!(out.stderr.is_empty());
            server.join().unwrap();
        }
    }
}
#[test]
fn tool_pipeline_can_reduce_large_intermediates_and_errors_stay_bounded() {
    let root = tempfile::tempdir().unwrap();
    for api in ["chat", "responses", "anthropic"] {
        for (args, input, expected) in [
            (
                json!({"steps":[{"operation":"sort"},{"operation":"dedupe"}]}),
                "line\n".repeat(20000),
                json!("line\n"),
            ),
            (
                json!({"steps":[{"operation":"trim"}]}),
                "\"".repeat(40000),
                Value::Null,
            ),
        ] {
            let (url, server) = model(api, args, expected);
            let out = invoke(
                root.path(),
                &[
                    "--api",
                    api,
                    "--base-url",
                    &url,
                    "--model",
                    "fixture",
                    "transform",
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
            server.join().unwrap();
        }
    }
}
#[test]
fn failed_tool_pipeline_can_recover_and_later_calls_restart_from_original_input() {
    let root = tempfile::tempdir().unwrap();
    for api in ["chat", "responses", "anthropic"] {
        let calls = vec![
            (
                json!({"steps":[{"operation":"trim"},{"operation":"json-minify"}]}),
                Value::Null,
            ),
            (
                json!({"steps":[{"operation":"trim"},{"operation":"dedupe"}]}),
                json!("b\na\n"),
            ),
            (json!({"operation":"dedupe"}), json!(" b \na\nb\n")),
        ];
        let (url, server) = model_turns(api, calls);
        let out = invoke(
            root.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "-o",
                "result.txt",
                "transform",
            ],
            b" b \na\nb\n",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("result.txt")).unwrap(),
            b"verified\n"
        );
        std::fs::remove_file(root.path().join("result.txt")).unwrap();
        server.join().unwrap();
    }
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
            (
                json!({"operation":"jsonl-to-array"}),
                "{\"n\":1.2300E+400}\n-0\n",
                json!("[{\"n\":1.2300E+400},-0]"),
            ),
            (
                json!({"operation":"array-to-jsonl"}),
                "[ {\"n\":123456789012345678901234567890}, \"a,b\" ]",
                json!("{\"n\":123456789012345678901234567890}\n\"a,b\"\n"),
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
        (json!({"operation":"jsonl-to-array"}), "{}\ninvalid".into()),
        (json!({"operation":"array-to-jsonl"}), "{}".into()),
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
