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
    terminal_with_environment(args, home, plain, answers, &[])
}

fn terminal_with_environment(
    args: &[&str],
    home: &std::path::Path,
    plain: bool,
    answers: &[(&str, &str)],
    environment: &[(&str, &str)],
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
    command.cwd(home);
    if plain {
        command.arg("--plain");
    }
    for key in [
        "ZI_PROFILE",
        "ZI_MODEL",
        "ZI_BASE_URL",
        "ZI_API",
        "ZI_API_KEY",
        "ZI_CONFIG",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
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
    for (key, value) in environment {
        command.env(*key, *value);
    }
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
            if console::strip_ansi_codes(&transcript[offset..]).contains(prompt) {
                // ConPTY may insert cursor controls inside a visible message.
                // Keep raw output for security assertions; match only displayed text.
                offset = transcript.len();
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
        for (number, reply) in replies.into_iter().enumerate() {
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
            let (body, content_type) = if number == 2 {
                let mut events = String::new();
                for piece in ["answer-", "two", "\x1b]52;c;", "clipboard-marker", "\x07"] {
                    events.push_str(&format!(
                        "data: {}\n\n",
                        json!({"choices":[{"delta":{"content":piece},"finish_reason":null}]})
                    ));
                }
                events.push_str(&format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":{},"finish_reason":"stop"}]})
                ));
                (events, "text/event-stream")
            } else {
                (reply.to_string(), "application/json")
            };
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
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

fn export_reply(
    api: &str,
    text: &str,
    streaming: bool,
    complete: bool,
) -> (u16, String, &'static str) {
    let event = |v: Value| format!("data: {v}\n\n");
    if streaming {
        let body=match api {
            "chat"=>format!("{}data: [DONE]\n\n",event(json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":if complete {"stop"}else{"length"}}]}))),
            "responses"=>format!("{}{}",event(json!({"type":"response.output_text.delta","delta":text})),event(json!({"type":"response.completed","response":{"status":if complete {"completed"}else{"incomplete"},"output":[{"type":"message","content":[{"type":"output_text","text":text}]}]}}))),
            _=>[
                json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":if complete {"end_turn"}else{"max_tokens"}}}),
                json!({"type":"message_stop"}),
            ].into_iter().map(event).collect(),
        };
        (200, body, "text/event-stream")
    } else {
        let body = match api {
            "chat" => json!({"choices":[{"finish_reason":"stop","message":{"content":text}}]}),
            "responses" => {
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]})
            }
            _ => json!({"stop_reason":"end_turn","content":[{"type":"text","text":text}]}),
        };
        (200, body.to_string(), "application/json")
    }
}

fn export_server(
    replies: Vec<(u16, String, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body, kind) in replies {
            let end = Instant::now() + Duration::from_secs(20);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "expected model request missing");
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
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        length = v.trim().parse().unwrap();
                    }
                }
            }
            let mut data = vec![0; length];
            reader.read_exact(&mut data).unwrap();
            requests.push(serde_json::from_slice(&data).unwrap());
            write!(stream,"HTTP/1.1 {status} Reply\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (url, task)
}

fn export_config(home: &std::path::Path, url: &str, api: &str) {
    let path = home.join("config/zistral");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("config.toml"),format!("model='fixture'\nbase_url='{url}'\napi='{api}'\n[profiles.alternate]\nmodel='alternate-model'\nbase_url='{url}'\napi='{api}'\n")).unwrap();
}

fn assert_independent_export_requests(api: &str, requests: &[Value], prompts: &[&str]) {
    assert_eq!(requests.len(), prompts.len());
    for (request, prompt) in requests.iter().zip(prompts) {
        let history = if api == "responses" {
            &request["input"]
        } else {
            &request["messages"]
        };
        let history = history.as_array().unwrap();
        assert_eq!(history.len(), if api == "anthropic" { 1 } else { 2 });
        assert_eq!(history.last().unwrap()["role"], "user");
        assert_eq!(history.last().unwrap()["content"], *prompt);
        assert_eq!(request["tools"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn interactive_export_is_explicit_no_clobber_and_cleared_by_new_tasks_models_and_clear() {
    for api in ["chat", "responses", "anthropic"] {
        let home = tempfile::tempdir().unwrap();
        let (url, server) = export_server(
            ["RESULT_ONE", "RESULT_TWO", "RESULT_THREE"]
                .into_iter()
                .map(|s| export_reply(api, s, false, true))
                .collect(),
        );
        export_config(home.path(), &url, api);
        std::fs::write(home.path().join("existing.md"), b"old content").unwrap();
        let output = terminal(
            &[],
            home.path(),
            true,
            &[
                ("zi>", "/save before.txt"),
                ("no completed result", "first"),
                ("RESULT_ONE", "/save result with spaces.md"),
                ("Saved result with spaces.md", "/save existing.md"),
                ("output file already exists", "/save! existing.md"),
                ("Saved existing.md", "/save -"),
                ("requires a file path", "/help"),
                ("independent task", "/save again.md"),
                ("Saved again.md", "/clear"),
                ("zi>", "/save cleared.md"),
                ("no completed result", "second"),
                ("RESULT_TWO", "/use alternate"),
                ("Active profile: alternate.", "/save switched.md"),
                ("no completed result", "third"),
                ("RESULT_THREE", "/run nonexistent-scenario"),
                ("zi:", "/save invalid-preset.md"),
                ("no completed result", "/exit"),
                ("Goodbye.", ""),
            ],
        );
        for name in ["result with spaces.md", "existing.md", "again.md"] {
            assert_eq!(
                std::fs::read(home.path().join(name)).unwrap(),
                b"RESULT_ONE\n"
            );
        }
        for name in [
            "before.txt",
            "cleared.md",
            "switched.md",
            "invalid-preset.md",
            "-",
        ] {
            assert!(!home.path().join(name).exists(), "{name}");
        }
        assert!(output.contains("/save! PATH"));
        let requests = server.join().unwrap();
        assert_eq!(requests[2]["model"], "alternate-model");
        assert_independent_export_requests(api, &requests, &["first", "second", "third"]);
        assert!(!std::fs::read_dir(home.path()).unwrap().any(|p| {
            p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".zi-result-")
        }));
    }
}

#[test]
fn interactive_export_keeps_complete_streamed_text_but_never_partial_or_failed_task_results() {
    for api in ["chat", "responses", "anthropic"] {
        let home = tempfile::tempdir().unwrap();
        let (url, server) = export_server(vec![
            export_reply(api, "STREAM_COMPLETE\n中文", true, true),
            export_reply(api, "VISIBLE_PARTIAL", true, false),
            export_reply(api, "RECOVERED", true, true),
            (401, "PRIVATE_PROVIDER_BODY".into(), "application/json"),
        ]);
        export_config(home.path(), &url, api);
        let output = terminal(
            &["--stream"],
            home.path(),
            true,
            &[
                ("zi>", "stream task"),
                ("中文", "/save complete.txt"),
                ("Saved complete.txt", "partial task"),
                ("zi:", "/save partial.txt"),
                ("no completed result", "recover task"),
                ("RECOVERED", "/save recovered.txt"),
                ("Saved recovered.txt", "http fail"),
                ("HTTP 401", "/save http.txt"),
                ("no completed result", "/exit"),
                ("Goodbye.", ""),
            ],
        );
        assert_eq!(
            std::fs::read(home.path().join("complete.txt")).unwrap(),
            "STREAM_COMPLETE\n中文\n".as_bytes()
        );
        assert_eq!(
            std::fs::read(home.path().join("recovered.txt")).unwrap(),
            b"RECOVERED\n"
        );
        assert!(
            !home.path().join("partial.txt").exists() && !home.path().join("http.txt").exists()
        );
        assert!(output.contains("VISIBLE_PARTIAL") && !output.contains("PRIVATE_PROVIDER_BODY"));
        assert_independent_export_requests(
            api,
            &server.join().unwrap(),
            &["stream task", "partial task", "recover task", "http fail"],
        );
    }
}

#[test]
fn interactive_export_validates_json_before_retention_and_keeps_raw_result_format() {
    for api in ["chat", "responses", "anthropic"] {
        let home = tempfile::tempdir().unwrap();
        let (url, server) = export_server(vec![
            export_reply(api, r#" { "n":1.2300E+400,"s":"\u4e2d" } "#, false, true),
            export_reply(api, "NOT_JSON", false, true),
        ]);
        export_config(home.path(), &url, api);
        terminal(
            &["--json"],
            home.path(),
            true,
            &[
                ("zi>", "valid json"),
                (r#"{"n":1.2300E+400,"s":"\u4e2d"}"#, "/save result.json"),
                ("Saved result.json", "invalid json"),
                ("output is not valid JSON", "/save invalid.json"),
                ("no completed result", "/exit"),
                ("Goodbye.", ""),
            ],
        );
        assert_eq!(
            std::fs::read(home.path().join("result.json")).unwrap(),
            b"{\"n\":1.2300E+400,\"s\":\"\\u4e2d\"}\n"
        );
        assert!(!home.path().join("invalid.json").exists());
        assert_independent_export_requests(
            api,
            &server.join().unwrap(),
            &["valid json", "invalid json"],
        );
    }
}

#[test]
fn interactive_export_does_not_reserve_piped_slash_prompts_or_create_hidden_result_files() {
    let home = tempfile::tempdir().unwrap();
    let (url, server) = export_server(vec![export_reply("chat", "PIPE_RESULT", false, true)]);
    export_config(home.path(), &url, "chat");
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut cmd = std::process::Command::new(binary);
    cmd.current_dir(home.path());
    for (name, _) in std::env::vars() {
        if name.starts_with("ZI_")
            || matches!(
                name.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            cmd.env_remove(name);
        }
    }
    let out = cmd
        .args([
            "--config",
            home.path()
                .join("config/zistral/config.toml")
                .to_str()
                .unwrap(),
            "--no-stream",
            "/save result.txt",
        ])
        .env("NO_PROXY", "*")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"PIPE_RESULT\n");
    assert!(!home.path().join("result.txt").exists());
    assert_independent_export_requests("chat", &server.join().unwrap(), &["/save result.txt"]);
    let files: Vec<_> = std::fs::read_dir(home.path())
        .unwrap()
        .map(|p| p.unwrap().file_name())
        .collect();
    assert_eq!(files, vec![std::ffi::OsString::from("config")]);
}

#[test]
fn terminal_controls_in_complete_model_answers_are_removed_when_rich_display_is_disabled() {
    let payload = "BEFORE\x1b[31mCOLOR_TEXT\x1b[0m\x1b]0;PROVIDER_TITLE_SENTINEL\x07\x1b]8;;urn:zi:provider-link\x1b\\LINK_TEXT\x1b]8;;\x1b\\SAFE_AFTER\n";
    for api in ["chat", "responses", "anthropic"] {
        for plain in [true, false] {
            let home = tempfile::tempdir().unwrap();
            let (url, server) = export_server(vec![export_reply(api, payload, false, true)]);
            export_config(home.path(), &url, api);
            let output = terminal_with_environment(
                &["--no-stream"],
                home.path(),
                plain,
                &[
                    ("zi>", "render controls"),
                    ("SAFE_AFTER", "/save raw.txt"),
                    ("Saved raw.txt", "/exit"),
                    ("Goodbye.", ""),
                ],
                if plain { &[] } else { &[("TERM", "dumb")] },
            );
            assert!(
                !output.contains("PROVIDER_TITLE_SENTINEL"),
                "provider title sequence reached terminal: {api},plain={plain}"
            );
            assert!(!output.contains("urn:zi:provider-link"));
            for word in ["BEFORE", "COLOR_TEXT", "LINK_TEXT", "SAFE_AFTER"] {
                assert!(output.contains(word));
            }
            assert_eq!(
                std::fs::read(home.path().join("raw.txt")).unwrap(),
                payload.as_bytes()
            );
            assert_independent_export_requests(api, &server.join().unwrap(), &["render controls"]);
        }
    }
}

#[test]
fn json_terminal_display_escapes_controls_without_losing_values_or_saved_bytes() {
    let payload = " { \"s\":\"L\u{0080}\u{009b}R\", \"n\":1.2300E+400, \"escaped\":\"\\u0080\", \"text\":\"中文\" } ";
    let compact = "{\"s\":\"L\u{0080}\u{009b}R\",\"n\":1.2300E+400,\"escaped\":\"\\u0080\",\"text\":\"中文\"}\n";
    for api in ["chat", "responses", "anthropic"] {
        for mode in ["default", "plain", "dumb"] {
            let home = tempfile::tempdir().unwrap();
            let (url, server) = export_server(vec![export_reply(api, payload, false, true)]);
            export_config(home.path(), &url, api);
            let output = terminal_with_environment(
                &["--json"],
                home.path(),
                mode == "plain",
                &[
                    ("zi>", "json controls"),
                    (r#""s":"L\u0080\u009bR""#, "/save result.json"),
                    ("Saved result.json", "/exit"),
                    ("Goodbye.", ""),
                ],
                if mode == "dumb" {
                    &[("TERM", "dumb")]
                } else {
                    &[]
                },
            );
            let visible = console::strip_ansi_codes(&output).replace(['\r', '\n'], "");
            assert!(visible.contains(r#""n":1.2300E+400"#), "{output}");
            assert!(visible.contains(r#""escaped":"\u0080""#), "{output}");
            assert!(!output.contains(['\u{0080}', '\u{009b}']), "{output}");
            assert_eq!(
                std::fs::read(home.path().join("result.json")).unwrap(),
                compact.as_bytes()
            );
            assert_independent_export_requests(api, &server.join().unwrap(), &["json controls"]);
        }
    }
}

#[test]
fn exact_terminal_tasks_buffer_native_results_and_export_empty_success_without_stale_data() {
    for api in ["chat", "responses", "anthropic"] {
        let reply = |id: &str, name: &str, args: Value| {
            let body = match api {
                "chat" => {
                    json!({"choices":[{"finish_reason":"tool_calls","message":{"content":"MODEL_REWRITE_SENTINEL","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}]})
                }
                "responses" => {
                    json!({"status":"completed","output":[{"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()}]})
                }
                _ => {
                    json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":id,"name":name,"input":args}]})
                }
            };
            (200, body.to_string(), "application/json")
        };
        let home = tempfile::tempdir().unwrap();
        let (url, server) = export_server(vec![
            reply("count", "count_input", json!({})),
            reply(
                "empty",
                "transform_input",
                json!({"operation":"lines-head","lines":1}),
            ),
            (401, "ignored".into(), "application/json"),
        ]);
        export_config(home.path(), &url, api);
        let text = terminal(
            &["--exact"],
            home.path(),
            false,
            &[
                ("›", "count empty input"),
                ("\"bytes\":0", "/save counted.json"),
                ("Saved counted.json", "empty result"),
                ("Done", "/save empty.txt"),
                ("Saved empty.txt", "fail task"),
                ("HTTP 401", "/save stale.txt"),
                ("no completed result", "/exit"),
                ("Goodbye.", ""),
            ],
        );
        assert!(!text.contains("MODEL_REWRITE_SENTINEL"));
        assert!(
            std::fs::read(home.path().join("empty.txt"))
                .unwrap()
                .is_empty()
        );
        assert!(!home.path().join("stale.txt").exists());
        let counts: Value =
            serde_json::from_slice(&std::fs::read(home.path().join("counted.json")).unwrap())
                .unwrap();
        assert_eq!(counts["bytes"], 0);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|r| r["stream"] == false));
        assert_independent_export_requests(
            api,
            &requests,
            &["count empty input", "empty result", "fail task"],
        );
    }
}

#[test]
fn model_plan_interactive_export_saves_definitions_and_json_preset_errors_clear_stale_results() {
    for api in ["chat", "responses", "anthropic"] {
        let reply = |id: &str, args: Value| {
            let body = match api {
                "chat" => {
                    json!({"choices":[{"finish_reason":"tool_calls","message":{"content":"DO_NOT_PUBLISH_MODEL_TEXT","tool_calls":[{"id":id,"type":"function","function":{"name":"transform_input","arguments":args.to_string()}}]}}]})
                }
                "responses" => {
                    json!({"status":"completed","output":[{"type":"function_call","call_id":id,"name":"transform_input","arguments":args.to_string()}]})
                }
                _ => {
                    json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":id,"name":"transform_input","input":args}]})
                }
            };
            (200, body.to_string(), "application/json")
        };
        let home = tempfile::tempdir().unwrap();
        let (url, server) = export_server(vec![
            reply("json", json!({"operation":"json-minify"})),
            reply("tail", json!({"operation":"lines-tail","lines":2})),
        ]);
        export_config(home.path(), &url, api);
        let text = terminal(
            &["--exact", "--export-plan"],
            home.path(),
            false,
            &[
                ("›", "make JSON plan"),
                ("operation = \"json-minify\"", "/save plan.toml"),
                ("Saved plan.toml", "/run builtin:extract"),
                ("cannot use a JSON preset", "/save stale.toml"),
                ("no completed result", "make tail plan"),
                ("lines = 2", "/save tail.toml"),
                ("Saved tail.toml", "/exit"),
                ("Goodbye.", ""),
            ],
        );
        let visible = console::strip_ansi_codes(&text);
        assert!(visible.contains("Validate/export native plan") && visible.contains("Plan ready"));
        assert!(!text.contains("DO_NOT_PUBLISH_MODEL_TEXT"));
        assert!(!home.path().join("stale.toml").exists());
        let first: toml::Value =
            toml::from_str(&std::fs::read_to_string(home.path().join("plan.toml")).unwrap())
                .unwrap();
        assert_eq!(first["steps"][0]["operation"].as_str(), Some("json-minify"));
        let second: toml::Value =
            toml::from_str(&std::fs::read_to_string(home.path().join("tail.toml")).unwrap())
                .unwrap();
        assert_eq!(second["steps"][0]["lines"].as_integer(), Some(2));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        for (r, prompt) in requests.iter().zip(["make JSON plan", "make tail plan"]) {
            assert_eq!(r["tools"].as_array().unwrap().len(), 1);
            assert_eq!(r["stream"], false);
            let history = if api == "responses" {
                &r["input"]
            } else {
                &r["messages"]
            };
            assert_eq!(
                history.as_array().unwrap().len(),
                if api == "anthropic" { 1 } else { 2 }
            );
            assert_eq!(
                history.as_array().unwrap().last().unwrap()["content"],
                prompt
            );
        }
    }
}

#[test]
fn request_preview_remains_json_in_real_terminal_without_setup_or_activity() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.txt");
    std::fs::write(&input, "PREVIEW_TERMINAL_DATA").unwrap();
    let output = terminal(
        &[
            "--preview",
            "--model",
            "fixture",
            "-i",
            input.to_str().unwrap(),
            "task",
        ],
        root.path(),
        false,
        &[("\"version\": 1", "")],
    );
    let shown = console::strip_ansi_codes(&output);
    assert!(shown.contains("first-model-request-preview"), "{shown}");
    assert!(shown.contains("PREVIEW_TERMINAL_DATA"));
    assert!(shown.contains("\"stream\": true"));
    assert!(!shown.contains("Let's connect") && !shown.contains("API key:"));
    assert!(!root.path().join("config").exists());
}
