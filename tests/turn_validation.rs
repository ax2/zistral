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
    for (key, _) in std::env::vars() {
        if key.starts_with("ZI_")
            || matches!(
                key.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(key);
        }
    }
    c.env("APPDATA", root.join("config"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("NO_PROXY", "*");
    c
}

fn call(api: &str, id: &str, name: &str, arguments: Value) -> Value {
    match api {
        "chat" => {
            json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}})
        }
        "responses" => {
            json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments.to_string()})
        }
        _ => json!({"type":"tool_use","id":id,"name":name,"input":arguments}),
    }
}
fn turn(api: &str, calls: Vec<Value>) -> Value {
    match api {
        "chat" => {
            json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":calls}}]})
        }
        "responses" => json!({"status":"completed","output":calls}),
        _ => json!({"stop_reason":"tool_use","content":calls}),
    }
}
fn answer(api: &str, text: &str) -> Value {
    match api {
        "chat" => json!({"choices":[{"finish_reason":"stop","message":{"content":text}}]}),
        "responses" => {
            json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]})
        }
        _ => json!({"stop_reason":"end_turn","content":[{"type":"text","text":text}]}),
    }
}
fn event(value: Value) -> String {
    format!("data: {value}\n\n")
}
fn encode(api: &str, result: Value, streaming: bool, prefix: bool) -> (String, &'static str) {
    if !streaming {
        return (result.to_string(), "application/json");
    }
    let body = match api {
        "chat" => {
            let message = &result["choices"][0]["message"];
            let mut delta = message.clone();
            if let Some(calls) = delta["tool_calls"].as_array_mut() {
                for (index, call) in calls.iter_mut().enumerate() {
                    call["index"] = json!(index);
                }
            }
            if prefix {
                delta["content"] = json!("partial");
            }
            event(
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":result["choices"][0]["finish_reason"]}]}),
            ) + "data: [DONE]\n\n"
        }
        "responses" => {
            let text = if prefix {
                event(json!({"type":"response.output_text.delta","delta":"partial"}))
            } else {
                String::new()
            };
            text + &event(json!({"type":"response.completed","response":result}))
        }
        _ => {
            let mut text =
                event(json!({"type":"message_start","message":{"content":[],"usage":{}}}));
            let mut blocks = result["content"].as_array().unwrap().clone();
            if prefix {
                blocks.insert(0, json!({"type":"text","text":"partial"}));
            }
            for (index, block) in blocks.into_iter().enumerate() {
                text += &event(
                    json!({"type":"content_block_start","index":index,"content_block":block}),
                );
                text += &event(json!({"type":"content_block_stop","index":index}));
            }
            text += &event(
                json!({"type":"message_delta","delta":{"stop_reason":result["stop_reason"]}}),
            );
            text + &event(json!({"type":"message_stop"}))
        }
    };
    (body, "text/event-stream")
}
fn serve(replies: Vec<(String, &'static str)>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (body, mime) in replies {
            let end = Instant::now() + Duration::from_secs(15);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "missing model request");
                        std::thread::sleep(Duration::from_millis(5));
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
            requests.push(serde_json::from_slice(&bytes).unwrap());
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (url, server)
}
fn invoke(root: &Path, api: &str, url: &str, streaming: bool, project: bool, file: bool) -> Output {
    let mut c = command(root);
    c.args([
        "--api",
        api,
        "--base-url",
        url,
        "--model",
        "fixture",
        "--max-steps",
        "4",
    ]);
    if streaming {
        c.arg("--stream");
    }
    if project {
        c.arg("--agent");
    }
    if file {
        c.args(["-o", "keep.txt", "--force"]);
    }
    c.arg("inspect input")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    if let Err(e) = child.stdin.take().unwrap().write_all(b"abc") {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}
fn assert_rejected(root: &Path, api: &str, result: Value, streaming: bool, prefix: bool) {
    std::fs::write(root.join("keep.txt"), b"original\r\n").unwrap();
    let (url, server) = serve(vec![encode(api, result, streaming, prefix)]);
    let out = invoke(root, api, &url, streaming, true, !streaming);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!out.status.success(), "unexpected success: {api}");
    assert_eq!(
        out.stdout,
        if prefix {
            b"partial\n".as_slice()
        } else {
            b"".as_slice()
        }
    );
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(!error.contains("PRIVATE"), "{error}");
    assert_eq!(
        std::fs::read(root.join("keep.txt")).unwrap(),
        b"original\r\n"
    );
    assert!(
        !root.join("created.txt").exists(),
        "file tool ran before validation: {api}, streaming={streaming}"
    );
    assert!(
        !root.join("command.txt").exists(),
        "command tool ran before validation: {api}"
    );
    assert!(!std::fs::read_dir(root).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn incomplete_or_inconsistent_final_turns_never_replace_existing_results() {
    for reason in [
        Value::Null,
        json!(false),
        json!("PRIVATE_REASON"),
        json!("length"),
        json!("content_filter"),
        json!("function_call"),
        json!("tool_calls"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut result = answer("chat", "PRIVATE_PARTIAL_TEXT");
        result["choices"][0]["finish_reason"] = reason;
        assert_rejected(root.path(), "chat", result, false, false);
    }
    for calls in [json!({}), json!("PRIVATE"), json!(1)] {
        let root = tempfile::tempdir().unwrap();
        let mut result = answer("chat", "PRIVATE_PARTIAL");
        result["choices"][0]["message"]["tool_calls"] = calls;
        assert_rejected(root.path(), "chat", result, false, false);
    }
    let root = tempfile::tempdir().unwrap();
    let mut result = answer("chat", "PRIVATE_PARTIAL");
    result["choices"][0]
        .as_object_mut()
        .unwrap()
        .remove("finish_reason");
    assert_rejected(root.path(), "chat", result, false, false);
    let root = tempfile::tempdir().unwrap();
    let mut result = answer("anthropic", "PRIVATE_PARTIAL");
    result["stop_reason"] = json!("tool_use");
    assert_rejected(root.path(), "anthropic", result, false, false);
}

#[test]
fn malformed_batches_are_rejected_before_any_tools_in_every_protocol_and_stream_mode() {
    for api in ["chat", "responses", "anthropic"] {
        for streaming in [false, true] {
            for kind in 0..9 {
                let root = tempfile::tempdir().unwrap();
                let write = call(
                    api,
                    "PRIVATE_ID",
                    "write_file",
                    json!({"path":"created.txt","content":"PRIVATE_BODY"}),
                );
                let command = call(
                    api,
                    "command",
                    "run_command",
                    json!({"command":"echo executed>command.txt"}),
                );
                let mut bad = call(api, "bad", "count_input", json!({}));
                let id = if api == "responses" { "call_id" } else { "id" };
                if kind == 8 {
                    if api == "chat" {
                        bad["function"]["arguments"] = Value::Null;
                    } else if api == "responses" {
                        bad["arguments"] = Value::Null;
                    } else {
                        bad["input"] = Value::Null;
                    }
                } else {
                    match kind {
                        0 => {
                            bad.as_object_mut().unwrap().remove(id);
                        }
                        1 => bad[id] = json!("   "),
                        2 => bad[id] = json!(7),
                        3 => bad[id] = json!("PRIVATE_ID"),
                        _ => {
                            let fields = if api == "chat" {
                                bad["function"].as_object_mut().unwrap()
                            } else {
                                bad.as_object_mut().unwrap()
                            };
                            if kind == 4 {
                                fields.remove("name");
                            } else if kind == 5 {
                                fields.insert("name".into(), json!(" "));
                            } else {
                                let key = if api == "anthropic" {
                                    "input"
                                } else {
                                    "arguments"
                                };
                                if kind == 6 {
                                    fields.remove(key);
                                } else if api == "anthropic" {
                                    fields.insert("name".into(), json!(7));
                                } else {
                                    fields.insert(key.into(), json!({"PRIVATE":"not a string"}));
                                }
                            }
                        }
                    }
                }
                assert_rejected(
                    root.path(),
                    api,
                    turn(api, vec![write, command, bad]),
                    streaming,
                    false,
                );
            }
        }
    }
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let mut calls = vec![call(
            api,
            "write",
            "write_file",
            json!({"path":"created.txt","content":"PRIVATE_BODY"}),
        )];
        for index in 1..33 {
            calls.push(call(api, &format!("c{index}"), "count_input", json!({})));
        }
        assert_rejected(root.path(), api, turn(api, calls), false, false);
    }
}

#[test]
fn refusals_and_invalid_text_are_checked_before_tools() {
    for streaming in [false, true] {
        for part in [
            json!({"type":"refusal","refusal":"PRIVATE_REFUSAL"}),
            json!({"type":"output_text","text":7}),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut result = turn(
                "responses",
                vec![call(
                    "responses",
                    "write",
                    "write_file",
                    json!({"path":"created.txt","content":"PRIVATE_BODY"}),
                )],
            );
            result["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"message","content":[part]}));
            assert_rejected(root.path(), "responses", result, streaming, false);
        }
    }
    for api in ["chat", "responses", "anthropic"] {
        for streaming in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut result = turn(
                api,
                vec![call(
                    api,
                    "write",
                    "write_file",
                    json!({"path":"created.txt","content":"PRIVATE_BODY"}),
                )],
            );
            match api {
                "chat" => result["choices"][0]["message"]["content"] = json!({"PRIVATE":true}),
                "responses" => result["output"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type":"message","content":{"PRIVATE":true}})),
                _ => result["content"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type":"text","text":7})),
            }
            assert_rejected(root.path(), api, result, streaming, false);
        }
    }
}

#[test]
fn valid_32_call_batches_keep_correlated_results_recoverable_errors_and_per_turn_ids() {
    for api in ["chat", "responses", "anthropic"] {
        for streaming in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let calls = (0..32)
                .map(|index| {
                    let mut value = call(
                        api,
                        &format!("c{index}"),
                        if index == 0 {
                            "select_input"
                        } else {
                            "count_input"
                        },
                        if index == 0 {
                            json!({"pointer":7})
                        } else {
                            json!({})
                        },
                    );
                    if index == 1 {
                        if api == "anthropic" {
                            value["input"] = json!({"unexpected":true});
                        } else if api == "chat" {
                            value["function"]["arguments"] = json!("");
                        } else {
                            value["arguments"] = json!("");
                        }
                    }
                    value
                })
                .collect();
            let (url, server) = serve(vec![
                encode(api, turn(api, calls), streaming, false),
                encode(
                    api,
                    turn(api, vec![call(api, "c0", "count_input", json!({}))]),
                    streaming,
                    false,
                ),
                encode(api, answer(api, "verified"), streaming, false),
            ]);
            let out = invoke(root.path(), api, &url, streaming, false, false);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"verified\n");
            assert!(out.stderr.is_empty());
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 3);
            let results: Vec<(&str, &str)> = match api {
                "chat" => requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "tool")
                    .map(|m| {
                        (
                            m["tool_call_id"].as_str().unwrap(),
                            m["content"].as_str().unwrap(),
                        )
                    })
                    .collect(),
                "responses" => requests[1]["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["type"] == "function_call_output")
                    .map(|m| {
                        (
                            m["call_id"].as_str().unwrap(),
                            m["output"].as_str().unwrap(),
                        )
                    })
                    .collect(),
                _ => requests[1]["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| {
                        (
                            m["tool_use_id"].as_str().unwrap(),
                            m["content"].as_str().unwrap(),
                        )
                    })
                    .collect(),
            };
            assert_eq!(results.len(), 32);
            for (index, (id, text)) in results.into_iter().enumerate() {
                assert_eq!(id, format!("c{index}"));
                let value: Value = serde_json::from_str(text).unwrap();
                if index < 2 {
                    assert!(value["error"].is_string());
                } else {
                    assert_eq!(value["bytes"], 3);
                }
            }
        }
    }
}

#[test]
fn streamed_text_may_be_visible_but_invalid_batches_still_execute_no_tools() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let mut bad = call(api, "bad", "count_input", json!({}));
        bad.as_object_mut()
            .unwrap()
            .remove(if api == "responses" { "call_id" } else { "id" });
        assert_rejected(
            root.path(),
            api,
            turn(
                api,
                vec![
                    call(
                        api,
                        "write",
                        "write_file",
                        json!({"path":"created.txt","content":"PRIVATE_BODY"}),
                    ),
                    bad,
                ],
            ),
            true,
            true,
        );
    }
}

#[test]
fn stop_reasons_cannot_hide_a_tool_batch_in_chat_or_anthropic() {
    for api in ["chat", "anthropic"] {
        for streaming in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut result = turn(
                api,
                vec![call(
                    api,
                    "write",
                    "write_file",
                    json!({"path":"created.txt","content":"PRIVATE_BODY"}),
                )],
            );
            if api == "chat" {
                result["choices"][0]["finish_reason"] = json!("stop");
            } else {
                result["stop_reason"] = json!("end_turn");
            }
            assert_rejected(root.path(), api, result, streaming, false);
        }
    }
}
