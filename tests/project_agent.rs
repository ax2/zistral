use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn command(root: &Path) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    c.current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    c.env("NO_PROXY", "*");
    c.env("CARGO_TERM_COLOR", "never");
    c.env("APPDATA", root.join("config"));
    c.env("XDG_CONFIG_HOME", root.join("config"));
    c
}
fn listener() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    (l, url)
}
fn request(l: &TcpListener) -> (TcpStream, Value) {
    let deadline = Instant::now() + Duration::from_secs(90);
    let stream = loop {
        match l.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "agent stopped before next request"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut r = BufReader::new(stream.try_clone().unwrap());
    let mut len = 0;
    loop {
        let mut line = String::new();
        assert!(r.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap();
            }
        }
    }
    let mut bytes = vec![0; len];
    r.read_exact(&mut bytes).unwrap();
    (stream, serde_json::from_slice(&bytes).unwrap())
}
fn reply(mut s: TcpStream, v: Value) {
    let body = v.to_string();
    write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
}
fn call(api: &str, name: &str, args: Value) -> Value {
    match api {
        "chat" => {
            json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"call","type":"function","function":{"name":name,"arguments":args.to_string()}}]}}]})
        }
        "responses" => {
            json!({"status":"completed","output":[{"type":"function_call","call_id":"call","name":name,"arguments":args.to_string()}]})
        }
        _ => {
            json!({"stop_reason":"tool_use","content":[{"type":"tool_use","id":"call","name":name,"input":args}]})
        }
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
fn last_tool(api: &str, r: &Value) -> Value {
    let text = match api {
        "chat" => r["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
        "responses" => r["input"].as_array().unwrap().last().unwrap()["output"]
            .as_str()
            .unwrap(),
        _ => r["messages"].as_array().unwrap().last().unwrap()["content"][0]["content"]
            .as_str()
            .unwrap(),
    };
    serde_json::from_str(text).unwrap()
}
fn plan(status: &str) -> Value {
    json!({"steps":[{"step":"Inspect, fix, verify","status":status}]})
}

#[test]
fn real_rust_project_failure_edit_retest_loop_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='zi-agent-fixture'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("AGENTS.md"),
            "Keep the KEEP marker. Run cargo test after changes.",
        )
        .unwrap();
        std::fs::write(root.path().join(".gitignore"), ".zi-test-target/\n").unwrap();
        std::fs::write(root.path().join("src/lib.rs"),"// KEEP\npub fn add(a:i32,b:i32)->i32{a - b}\n#[test]fn add_works(){assert_eq!(add(2,3),5);}\n").unwrap();
        let (l, url) = listener();
        let server = std::thread::spawn(move || {
            let actions = vec![
                ("update_plan", plan("in_progress")),
                ("list_files", json!({})),
                ("search_files", json!({"query":"a - b"})),
                ("read_file", json!({"path":"src/lib.rs"})),
                (
                    "run_command",
                    json!({"command":"cargo test --offline --quiet --target-dir .zi-test-target","timeout_seconds":60}),
                ),
                (
                    "edit_file",
                    json!({"path":"src/lib.rs","old_text":"a - b","new_text":"a + b"}),
                ),
                (
                    "write_file",
                    json!({"path":"notes/result.md","content":"Fixed add and verified with cargo test.\n"}),
                ),
                (
                    "run_command",
                    json!({"command":"cargo test --offline --quiet --target-dir .zi-test-target","timeout_seconds":60}),
                ),
                ("update_plan", plan("completed")),
            ];
            for step in 0..=actions.len() {
                let (s, r) = request(&l);
                if step == 0 {
                    let system = if api == "anthropic" {
                        r["system"].as_str().unwrap()
                    } else {
                        let key = if api == "responses" {
                            "input"
                        } else {
                            "messages"
                        };
                        r[key][0]["content"].as_str().unwrap()
                    };
                    assert!(system.contains("KEEP"));
                    assert!(system.contains("Project environment"));
                    let tools = r["tools"].as_array().unwrap();
                    assert_eq!(tools.len(), 15);
                } else {
                    let previous = last_tool(api, &r);
                    assert!(
                        previous.get("error").is_none(),
                        "{api} step {step}: {previous}"
                    );
                    match step {
                        2 => assert!(
                            previous["entries"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|f| f["path"] == "Cargo.toml")
                        ),
                        3 => assert_eq!(previous["matches"][0]["path"], "src/lib.rs"),
                        4 => assert!(previous["content"].as_str().unwrap().contains("a - b")),
                        5 => {
                            assert_eq!(previous["success"], false);
                            assert_eq!(previous["timed_out"], false);
                        }
                        6 | 7 => assert_eq!(previous["changed"], true),
                        8 => {
                            assert_eq!(previous["success"], true, "{previous}");
                            assert!(previous["stdout"].as_str().unwrap().contains("1 passed"));
                        }
                        _ => (),
                    }
                }
                if step < actions.len() {
                    reply(s, call(api, actions[step].0, actions[step].1.clone()));
                } else {
                    reply(s, answer(api, "fixed and verified"));
                }
            }
        });
        let output = command(root.path())
            .args([
                "--cwd",
                root.path().to_str().unwrap(),
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "Fix add and run its tests",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{api}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"fixed and verified\n");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let fixed = std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap();
        assert!(fixed.contains("// KEEP"));
        assert!(fixed.contains("a + b"));
        assert!(root.path().join("notes/result.md").exists());
        server.join().unwrap();
    }
}

#[test]
fn readonly_and_default_data_modes_cannot_mutate_project() {
    for readonly in [true, false] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("keep.txt"), "keep").unwrap();
        let (l, url) = listener();
        let server = std::thread::spawn(move || {
            let (s, r) = request(&l);
            assert!(
                !r["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["function"]["name"] == "write_file")
            );
            reply(
                s,
                call(
                    "chat",
                    "write_file",
                    json!({"path":"keep.txt","content":"bad"}),
                ),
            );
            let (s, r) = request(&l);
            assert!(last_tool("chat", &r)["error"].is_string());
            reply(
                s,
                call("chat", "run_command", json!({"command":"echo bad"})),
            );
            let (s, r) = request(&l);
            assert!(last_tool("chat", &r)["error"].is_string());
            reply(s, answer("chat", "no changes"));
        });
        let mut c = command(root.path());
        if readonly {
            c.args(["--cwd", root.path().to_str().unwrap(), "--read-only"]);
        }
        let output = c
            .args(["--base-url", &url, "--model", "fixture", "Inspect only"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"no changes\n");
        assert_eq!(
            std::fs::read_to_string(root.path().join("keep.txt")).unwrap(),
            "keep"
        );
        server.join().unwrap();
    }
}

#[test]
fn pending_plan_prevents_premature_success() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    let server = std::thread::spawn(move || {
        let (s, _) = request(&l);
        reply(s, call("chat", "update_plan", plan("in_progress")));
        let (s, _) = request(&l);
        reply(s, answer("chat", "premature"));
        let (s, r) = request(&l);
        assert!(
            r["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("plan still")
        );
        reply(s, call("chat", "update_plan", plan("completed")));
        let (s, _) = request(&l);
        reply(s, answer("chat", "done"));
    });
    let output = command(root.path())
        .args([
            "--agent",
            "--base-url",
            &url,
            "--model",
            "fixture",
            "Complete the plan",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"done\n");
    server.join().unwrap();
}
