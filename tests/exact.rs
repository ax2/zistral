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

            let system = if api == "anthropic" {
                req["system"].as_str().unwrap()
            } else if api == "responses" {
                req["input"][0]["content"].as_str().unwrap()
            } else {
                req["messages"][0]["content"].as_str().unwrap()
            };
            assert!(system.contains("Exact mode:"));
            let plan_export = system.contains("Plan-export mode:");
            assert_eq!(
                req["tools"].as_array().unwrap().len(),
                if plan_export { 1 } else { 3 }
            );
            if plan_export {
                let tool = &req["tools"][0];
                let function = if api == "chat" {
                    &tool["function"]
                } else {
                    tool
                };
                assert_eq!(function["name"], "transform_input");
                assert!(
                    function["description"]
                        .as_str()
                        .unwrap()
                        .contains("does not execute")
                );
            }

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

#[test]
fn model_plan_export_roundtrips_offline_without_executing_invalid_input() {
    for api in ["chat", "responses", "anthropic"] {
        for pipeline in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let args = if pipeline {
                json!({"steps":[{"operation":"json-minify"},{"operation":"json-select","pointer":"/items"},{"operation":"array-to-jsonl"}]})
            } else {
                json!({"operation":"json-minify"})
            };
            let (url, task) = server(
                api,
                vec![calls(
                    api,
                    vec![call(api, "proposal", "transform_input", args)],
                )],
            );
            let out = invoke(
                root.path(),
                api,
                &url,
                &["--export-plan", "--max-steps", "1", "-o", "plan.toml"],
                b"NOT_JSON_PRIVATE_CONTEXT",
            );
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stdout.is_empty());
            let plan = std::fs::read_to_string(root.path().join("plan.toml")).unwrap();
            let parsed: toml::Value = toml::from_str(&plan).unwrap();
            assert_eq!(parsed["version"].as_integer(), Some(1));
            assert_eq!(
                parsed["steps"].as_array().unwrap().len(),
                if pipeline { 3 } else { 1 }
            );
            assert!(
                !plan.contains("NOT_JSON_PRIVATE_CONTEXT")
                    && !plan.contains("MODEL_TEXT_IS_NOT_NATIVE_OUTPUT")
            );
            let mut child = command(root.path())
                .args(["data", "--recipe", "plan.toml"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(br#"{"items":[1.2300E+400]}"#)
                .unwrap();
            let executed = child.wait_with_output().unwrap();
            assert!(executed.status.success());
            assert_eq!(
                executed.stdout,
                if pipeline {
                    b"1.2300E+400\n".to_vec()
                } else {
                    b"{\"items\":[1.2300E+400]}\n".to_vec()
                }
            );
            let preview = command(root.path())
                .args(["data", "--show-plan", "--recipe", "plan.toml"])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(preview.status.success());
            assert_eq!(preview.stdout, plan.as_bytes());
            let requests = task.join().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].to_string().contains("NOT_JSON_PRIVATE_CONTEXT"));
        }
    }
}

#[test]
fn model_plan_definition_errors_recover_without_data_execution() {
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
                        json!({"operation":"json-minify"}),
                    )],
                ),
            ],
        );
        let out = invoke(
            root.path(),
            api,
            &url,
            &["--export-plan", "--max-steps", "2"],
            b"INVALID_JSON_STILL_NOT_EXECUTED",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let plan: toml::Value = toml::from_str(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
        assert_eq!(plan["steps"][0]["operation"].as_str(), Some("json-minify"));
        let requests = task.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1]
                .to_string()
                .contains("lines must be an integer from 1 to 100000")
        );
    }
}

#[test]
fn model_plan_export_rejects_other_tools_free_text_and_batches_without_publication() {
    for api in ["chat", "responses", "anthropic"] {
        let text = match api {
            "chat" => {
                json!({"choices":[{"finish_reason":"stop","message":{"content":"FAKE_PLAN"}}]})
            }
            "responses" => {
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"FAKE_PLAN"}]}]})
            }
            _ => json!({"stop_reason":"end_turn","content":[{"type":"text","text":"FAKE_PLAN"}]}),
        };
        for reply in [
            calls(api, vec![call(api, "wrong", "count_input", json!({}))]),
            calls(
                api,
                vec![
                    call(api, "one", "transform_input", json!({"operation":"trim"})),
                    call(api, "two", "transform_input", json!({"operation":"dedupe"})),
                ],
            ),
            (text.to_string(), "application/json"),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("keep.toml"), b"keep").unwrap();
            let (url, task) = server(api, vec![reply]);
            let out = invoke(
                root.path(),
                api,
                &url,
                &[
                    "--export-plan",
                    "--max-steps",
                    "1",
                    "-o",
                    "keep.toml",
                    "--force",
                ],
                b"context",
            );
            assert!(!out.status.success() && out.stdout.is_empty());
            assert_eq!(
                std::fs::read(root.path().join("keep.toml")).unwrap(),
                b"keep"
            );
            assert!(!String::from_utf8_lossy(&out.stderr).contains("zi: tool "));
            task.join().unwrap();
        }
    }
}

#[test]
fn model_plan_invalid_and_oversized_definitions_cannot_replace_files_or_leak_literals() {
    for api in ["chat", "responses", "anthropic"] {
        for args in [
            json!({"operation":"trim","path":"PRIVATE_PATH"}),
            json!({"steps":vec![json!({"operation":"trim"});17]}),
            json!({"operation":"lines-include","contains":"x".repeat(65536)}),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("keep.toml"), b"keep").unwrap();
            let (url, task) = server(
                api,
                vec![calls(api, vec![call(api, "bad", "transform_input", args)])],
            );
            let out = invoke(
                root.path(),
                api,
                &url,
                &[
                    "--export-plan",
                    "--max-steps",
                    "1",
                    "-o",
                    "keep.toml",
                    "--force",
                ],
                b"context",
            );
            assert!(!out.status.success() && out.stdout.is_empty());
            assert_eq!(
                std::fs::read(root.path().join("keep.toml")).unwrap(),
                b"keep"
            );
            assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_PATH"));
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|p| {
                p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".zi-result-")
            }));
            task.join().unwrap();
        }
    }
}

#[test]
fn model_plan_export_requires_exact_and_rejects_json_presets_before_data_output_or_network() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("invalid.toml"), b"not model configuration").unwrap();
    std::fs::write(root.path().join("keep.toml"), b"keep").unwrap();
    for options in [
        vec!["--export-plan", "task"],
        vec!["--exact", "--export-plan", "--json", "task"],
        vec![
            "--exact",
            "--export-plan",
            "--preset",
            "builtin:extract",
            "task",
        ],
        vec!["--exact", "--export-plan", "data", "trim"],
    ] {
        let out = command(root.path())
            .args([
                "--config",
                "invalid.toml",
                "-i",
                "missing-input",
                "-o",
                "keep.toml",
                "--force",
            ])
            .args(&options)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            !error.contains("cannot open missing-input")
                && !error.contains("cannot read configuration")
        );
        if options.contains(&"builtin:extract") {
            assert!(error.contains("--export-plan produces TOML"), "{error}");
        }
        assert_eq!(
            std::fs::read(root.path().join("keep.toml")).unwrap(),
            b"keep"
        );
    }
}

#[test]
fn jsonl_where_exact_execution_and_model_plan_export_roundtrip_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let input=b"{\"level\":\"INFO\",\"message\":\"ERROR\"}\n{\"level\":\"\\u0045RROR\",\"id\":1.2300E+400}\n";
        let args = json!({"operation":"jsonl-where","pointer":"/level","equals":"ERROR"});
        for export in [false, true] {
            let (url, task) = server(
                api,
                vec![calls(
                    api,
                    vec![call(api, "where", "transform_input", args.clone())],
                )],
            );
            let options = if export {
                vec!["--export-plan", "--max-steps", "1", "-o", "plan.toml"]
            } else {
                vec!["--max-steps", "1"]
            };
            let out = invoke(root.path(), api, &url, &options, input);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            if export {
                assert!(out.stdout.is_empty());
                let plan = std::fs::read_to_string(root.path().join("plan.toml")).unwrap();
                let value: toml::Value = toml::from_str(&plan).unwrap();
                assert_eq!(value["steps"][0]["equals"].as_str(), Some("ERROR"));
                let mut child = command(root.path())
                    .args(["data", "--recipe", "plan.toml"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child.stdin.take().unwrap().write_all(input).unwrap();
                let run = child.wait_with_output().unwrap();
                assert!(run.status.success());
                assert_eq!(
                    run.stdout,
                    b"{\"level\":\"\\u0045RROR\",\"id\":1.2300E+400}\n"
                );
            } else {
                assert_eq!(
                    out.stdout,
                    b"{\"level\":\"\\u0045RROR\",\"id\":1.2300E+400}\n"
                );
            }
            assert_eq!(task.join().unwrap().len(), 1);
        }
    }
}

#[test]
fn keyed_jsonl_exact_results_and_plan_exports_roundtrip_in_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let input =
            b"{\"id\":\"b\",\"t\":\"2\"}\n{\"id\":\"a\",\"t\":\"1\"}\n{\"id\":\"b\",\"t\":\"2\"}\n";
        for (op, pointer, expected) in [
            (
                "jsonl-dedupe",
                "/id",
                "{\"id\":\"b\",\"t\":\"2\"}\n{\"id\":\"a\",\"t\":\"1\"}\n",
            ),
            (
                "jsonl-sort",
                "/t",
                "{\"id\":\"a\",\"t\":\"1\"}\n{\"id\":\"b\",\"t\":\"2\"}\n{\"id\":\"b\",\"t\":\"2\"}\n",
            ),
            (
                "jsonl-count-by",
                "/id",
                "{\"value\":\"b\",\"count\":2}\n{\"value\":\"a\",\"count\":1}\n",
            ),
        ] {
            for export in [false, true] {
                let args = json!({"operation":op,"pointer":pointer});
                let (url, task) = server(
                    api,
                    vec![calls(
                        api,
                        vec![call(api, "keyed", "transform_input", args)],
                    )],
                );
                let options = if export {
                    vec!["--export-plan", "--max-steps", "1"]
                } else {
                    vec!["--max-steps", "1"]
                };
                let out = invoke(root.path(), api, &url, &options, input);
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert_eq!(task.join().unwrap().len(), 1);
                if export {
                    let value: toml::Value =
                        toml::from_str(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
                    assert_eq!(value["steps"][0]["operation"].as_str(), Some(op));
                    assert_eq!(value["steps"][0]["pointer"].as_str(), Some(pointer));
                    std::fs::write(root.path().join("keyed.toml"), out.stdout).unwrap();
                    let mut child = command(root.path())
                        .args(["data", "--recipe", "keyed.toml"])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .unwrap();
                    child.stdin.take().unwrap().write_all(input).unwrap();
                    let run = child.wait_with_output().unwrap();
                    assert!(run.status.success());
                    assert_eq!(run.stdout, expected.as_bytes());
                    assert!(run.stderr.is_empty());
                } else {
                    assert_eq!(out.stdout, expected.as_bytes());
                }
            }
        }
    }
}

#[test]
fn integer_ranking_exact_result_and_plan_export_roundtrip_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let input = b"{\"n\":2,\"id\":\"a\"}\n{\"n\":10,\"id\":\"b\"}\n{\"n\":10,\"id\":\"c\"}\n";
        let expected =
            b"{\"n\":10,\"id\":\"b\"}\n{\"n\":10,\"id\":\"c\"}\n{\"n\":2,\"id\":\"a\"}\n";
        for export in [false, true] {
            let args = json!({"operation":"jsonl-sort-int","pointer":"/n","reverse":true});
            let (url, task) = server(
                api,
                vec![calls(api, vec![call(api, "ints", "transform_input", args)])],
            );
            let options = if export {
                vec!["--export-plan", "--max-steps", "1"]
            } else {
                vec!["--max-steps", "1"]
            };
            let out = invoke(root.path(), api, &url, &options, input);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(task.join().unwrap().len(), 1);
            if export {
                let plan: toml::Value =
                    toml::from_str(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
                assert_eq!(
                    plan["steps"][0]["operation"].as_str(),
                    Some("jsonl-sort-int")
                );
                assert_eq!(plan["steps"][0]["reverse"].as_bool(), Some(true));
                std::fs::write(root.path().join("ints.toml"), out.stdout).unwrap();
                let mut child = command(root.path())
                    .args(["data", "--recipe", "ints.toml"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child.stdin.take().unwrap().write_all(input).unwrap();
                let run = child.wait_with_output().unwrap();
                assert!(run.status.success() && run.stderr.is_empty());
                assert_eq!(run.stdout, expected);
            } else {
                assert_eq!(out.stdout, expected);
            }
        }
    }
}
