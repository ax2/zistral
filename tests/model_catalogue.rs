use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path) -> Command {
    command_timeout(root, "3")
}
fn command_timeout(root: &Path, timeout: &str) -> Command {
    let mut c = Command::new(
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into()),
    );
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
    c.current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .env("NO_PROXY", "*")
        .args([
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--timeout",
            timeout,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}
fn config(root: &Path, url: &str, api: &str, extra: &str) -> Vec<u8> {
    let text = format!("base_url='{url}'\napi='{api}'\napi_key='AUTH_SECRET_993'\n{extra}\n");
    std::fs::write(root.join("config.toml"), &text).unwrap();
    text.into_bytes()
}
fn listener() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    (l, url)
}
fn accept(l: &TcpListener) -> (TcpStream, String) {
    let end = Instant::now() + Duration::from_secs(10);
    let s = loop {
        match l.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end, "catalogue request did not arrive");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("{e}"),
        }
    };
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(s.try_clone().unwrap());
    let mut head = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    assert!(!head.to_lowercase().contains("content-length: "));
    (s, head)
}
fn send(mut s: TcpStream, status: u16, headers: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    );
    // Oversize/header-rejected responses can be closed early by the client.
    // Every case below checks its actual exit status and complete stdout.
    if let Err(e) = s.write_all(response.as_bytes()) {
        assert!(matches!(
            e.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::NotConnected
        ));
    }
}
fn checked(out: &Output, success: bool) {
    assert_eq!(
        out.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    if !success {
        assert!(out.stdout.is_empty(), "no partial catalogue on failure");
    }
    for secret in ["AUTH_SECRET_993", "BODY_SECRET_993", "PRIVATE_ROUTE_993"] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(secret));
    }
}
fn page(ids: &[&str], more: bool) -> String {
    json!({"data":ids.iter().map(|id|json!({"id":id,"type":"model"})).collect::<Vec<_>>(),
        "has_more":more,"first_id":ids.first(),"last_id":ids.last()})
    .to_string()
}

#[test]
fn saved_models_remain_offline_and_remote_ignores_generation_only_settings() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    let before = config(
        root.path(),
        &format!("{url}/v1"),
        "responses",
        "model='saved'\nmax_output_tokens=1\nchat_token_limit_field='not-a-generation-field'\nendpoint='https://other.example/PRIVATE_ROUTE_993'",
    );
    let out = command(root.path()).arg("models").output().unwrap();
    checked(&out, true);
    assert!(String::from_utf8_lossy(&out.stdout).contains("saved"));
    assert_eq!(
        l.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let server = std::thread::spawn(move || {
        let (s, h) = accept(&l);
        assert!(h.starts_with("GET /v1/models HTTP/1.1\r\n"));
        send(s, 200, "", r#"{"data":[{"id":"reported"}]}"#);
    });
    let out = command(root.path())
        .args(["models", "--remote"])
        .output()
        .unwrap();
    checked(&out, true);
    assert_eq!(out.stdout, b"reported\n");
    server.join().unwrap();
    assert_eq!(
        std::fs::read(root.path().join("config.toml")).unwrap(),
        before
    );
}

#[test]
fn all_protocols_use_model_catalogue_get_auth_and_no_task_body() {
    for (api, prefix, expected) in [
        ("chat", "/gateway/v1", "/gateway/v1/models"),
        ("responses", "/v1/", "/v1/models"),
        ("anthropic", "", "/v1/models?limit=1000"),
        ("anthropic", "/proxy/v1", "/proxy/v1/models?limit=1000"),
        (
            "anthropic",
            "/gateway%20space/v1/.// ",
            "/gateway%20space/v1/models?limit=1000",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        let before = config(root.path(), &format!("{url}{prefix}"), api, "");
        let server = std::thread::spawn(move || {
            let (s, h) = accept(&l);
            assert!(
                h.starts_with(&format!("GET {expected} HTTP/1.1\r\n")),
                "{h}"
            );
            let h = h.to_lowercase();
            assert!(h.contains("user-agent: zistral/"));
            if api == "anthropic" {
                assert!(h.contains("x-api-key: auth_secret_993"));
                assert!(h.contains("anthropic-version: 2023-06-01"));
                assert!(!h.contains("authorization:"));
            } else {
                assert!(h.contains("authorization: bearer auth_secret_993"));
                assert!(!h.contains("x-api-key:"));
            }
            let body = if api == "anthropic" {
                page(&["vendor/model-b", "模型甲"], false)
            } else {
                json!({"object":"list","data":[{"id":"vendor/model-b","object":"model","owned_by":"BODY_SECRET_993"},{"id":"模型甲"}]}).to_string()
            };
            send(s, 200, "", &body);
        });
        let out = command(root.path())
            .args(["models", "--remote", "--json"])
            .output()
            .unwrap();
        checked(&out, true);
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["kind"], "model-catalogue");
        assert_eq!(v["protocol"], api);
        assert_eq!(v["complete"], true);
        assert_eq!(v["pages"], 1);
        assert_eq!(v["models"], json!(["vendor/model-b", "模型甲"]));
        assert_eq!(v["profile"], "default");
        server.join().unwrap();
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            before
        );
    }
}

#[test]
fn selected_profile_and_effective_environment_key_are_used_without_saving() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    let before = config(
        root.path(),
        "http://127.0.0.1:1/v1",
        "chat",
        &format!(
            "model='root'\nactive_profile='work'\n[profiles.work]\nbase_url='{url}/v1'\napi='chat'\napi_key='wrong-key'\napi_key_env='CATALOGUE_FIXTURE_KEY'"
        ),
    );
    let server = std::thread::spawn(move || {
        let (s, h) = accept(&l);
        assert!(
            h.to_lowercase()
                .contains("authorization: bearer selected-key")
        );
        assert!(!h.contains("wrong-key"));
        send(s, 200, "", r#"{"data":[]}"#);
    });
    let out = command(root.path())
        .env("CATALOGUE_FIXTURE_KEY", "referenced-key")
        .env("ZI_API_KEY", "selected-key")
        .args(["models", "--remote", "--json"])
        .output()
        .unwrap();
    checked(&out, true);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["profile"], "work");
    assert_eq!(v["models"], json!([]));
    server.join().unwrap();
    assert_eq!(
        std::fs::read(root.path().join("config.toml")).unwrap(),
        before
    );
}

#[test]
fn anthropic_pagination_encodes_cursor_and_joins_all_pages_atomically() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "anthropic", "");
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        send(s, 200, "", &page(&["vendor/a&b?x=1"], true));
        let (s, h) = accept(&l);
        assert!(
            h.starts_with("GET /v1/models?limit=1000&after_id=vendor%2Fa%26b%3Fx%3D1 HTTP/1.1\r\n"),
            "{h}"
        );
        send(s, 200, "", &page(&["second"], false));
    });
    let out = command(root.path())
        .args(["models", "--remote", "--json"])
        .output()
        .unwrap();
    checked(&out, true);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["pages"], 2);
    assert_eq!(v["models"], json!(["vendor/a&b?x=1", "second"]));
    server.join().unwrap();
}

#[test]
fn invalid_catalogues_never_emit_a_prefix_or_provider_details() {
    for (api, body) in [
        ("chat", r#"{"error":{"message":"BODY_SECRET_993"}}"#),
        ("chat", r#"{"data":[{"id":"first"},{"id":2}]}"#),
        ("chat", r#"{"data":[{"id":"first"},{"id":"first"}]}"#),
        ("chat", r#"{"data":[{"id":""}]}"#),
        ("chat", r#"{"data":[{"id":"two words"}]}"#),
        ("chat", r#"{"data":[{"id":"bad\u001b[31m"}]}"#),
        ("chat", r#"{"data":[{"id":"AUTH_SECRET_993"}]}"#),
        ("chat", r#"{"object":"error","data":[]}"#),
        ("chat", r#"{"data":[{"id":"first","object":"message"}]}"#),
        ("chat", r#"{"data":[],"has_more":true}"#),
        ("anthropic", r#"{"data":[{"id":"first"}]}"#),
        (
            "anthropic",
            r#"{"data":[{"id":"first"}],"last_id":"wrong","has_more":true}"#,
        ),
        ("anthropic", r#"{"data":[],"last_id":null,"has_more":true}"#),
        ("chat", r#"{"data":[]} trailing BODY_SECRET_993"#),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        config(root.path(), &format!("{url}/v1"), api, "");
        let server = std::thread::spawn(move || {
            let (s, _) = accept(&l);
            send(s, 200, "", body);
        });
        let out = command(root.path())
            .args(["models", "--remote", "--json"])
            .output()
            .unwrap();
        checked(&out, false);
        server.join().unwrap();
    }
}

#[test]
fn later_page_failure_and_cursor_cycles_do_not_publish_earlier_models() {
    for body in [
        page(&["first"], false),
        "{\"error\":\"BODY_SECRET_993\"}".to_owned(),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        config(root.path(), &url, "anthropic", "");
        let server = std::thread::spawn(move || {
            let (s, _) = accept(&l);
            send(s, 200, "", &page(&["first"], true));
            let (s, _) = accept(&l);
            send(s, 200, "", &body);
        });
        let out = command(root.path())
            .args(["models", "--remote"])
            .output()
            .unwrap();
        checked(&out, false);
        server.join().unwrap();
    }
}

#[test]
fn http_failures_never_redirect_retry_or_echo_response_bodies() {
    for status in [302, 401, 403, 404, 429, 500] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        let (next, next_url) = listener();
        config(
            root.path(),
            &format!("{url}/PRIVATE_ROUTE_993/v1"),
            "chat",
            "",
        );
        let server = std::thread::spawn(move || {
            let (s, _) = accept(&l);
            send(
                s,
                status,
                &format!("Location: {next_url}/leak\r\nRetry-After: 0\r\n"),
                "BODY_SECRET_993",
            );
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(
                l.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
        let out = command(root.path())
            .args(["models", "--remote"])
            .output()
            .unwrap();
        checked(&out, false);
        assert!(String::from_utf8_lossy(&out.stderr).contains(&status.to_string()));
        server.join().unwrap();
        assert_eq!(
            next.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[test]
fn response_record_and_page_limits_fail_without_partial_output() {
    let big = json!({"data":[{"id":"first"}],"padding":"X".repeat(1_048_576)}).to_string();
    let many = json!({"data":(0..4097).map(|n|json!({"id":format!("m{n}")})).collect::<Vec<_>>()})
        .to_string();
    for body in [big, many] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        config(root.path(), &format!("{url}/v1"), "chat", "");
        let server = std::thread::spawn(move || {
            let (s, _) = accept(&l);
            send(s, 200, "", &body);
        });
        let out = command(root.path())
            .args(["models", "--remote"])
            .output()
            .unwrap();
        checked(&out, false);
        server.join().unwrap();
    }
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "anthropic", "");
    let server = std::thread::spawn(move || {
        for n in 0..10 {
            let (s, _) = accept(&l);
            send(s, 200, "", &page(&[&format!("m{n}")], true));
        }
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            l.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let out = command(root.path())
        .args(["models", "--remote"])
        .output()
        .unwrap();
    checked(&out, false);
    assert!(String::from_utf8_lossy(&out.stderr).contains("10 page"));
    server.join().unwrap();
}

#[test]
fn timeout_and_header_errors_are_safe_and_management_modes_fail_before_network() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        std::thread::sleep(Duration::from_millis(1300));
        drop(s);
    });
    let out = command_timeout(root.path(), "1")
        .args(["models", "--remote"])
        .output()
        .unwrap();
    checked(&out, false);
    assert!(String::from_utf8_lossy(&out.stderr).contains("timed out"));
    server.join().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let out = command(root.path())
        .env("ZI_API_KEY", "BAD\nBODY_SECRET_993")
        .args(["models", "--remote"])
        .output()
        .unwrap();
    checked(&out, false);
    assert!(String::from_utf8_lossy(&out.stderr).contains("credential settings"));
    for flags in [
        vec!["models", "--json"],
        vec!["--json", "models", "--remote"],
        vec!["--retries", "1", "models", "--remote"],
        vec!["--max-output-tokens", "12", "models", "--remote"],
        vec!["--preview", "models", "--remote"],
        vec!["--agent", "models", "--remote"],
        vec!["models", "--remote", "--input", "missing"],
        vec!["models", "--remote", "--output", "existing"],
    ] {
        std::fs::write(root.path().join("existing"), "keep").unwrap();
        let out = command(root.path()).args(flags).output().unwrap();
        checked(&out, false);
        assert_eq!(
            std::fs::read(root.path().join("existing")).unwrap(),
            b"keep"
        );
    }
    assert_eq!(
        l.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn unknown_length_size_total_size_and_exact_record_boundary_are_enforced() {
    for chunked in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        config(root.path(), &url, "chat", "");
        let server = std::thread::spawn(move || {
            let (mut s, _) = accept(&l);
            let body = "X".repeat(1_048_577);
            let response = if chunked {
                format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                    body.len()
                )
            } else {
                format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}")
            };
            if let Err(e) = s.write_all(response.as_bytes()) {
                assert!(matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::NotConnected
                ));
            }
        });
        let out = command(root.path())
            .args(["models", "--remote"])
            .output()
            .unwrap();
        checked(&out, false);
        assert!(String::from_utf8_lossy(&out.stderr).contains("size limit"));
        server.join().unwrap();
    }
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "anthropic", "");
    let server = std::thread::spawn(move || {
        for n in 0..5 {
            let (s, _) = accept(&l);
            let id = format!("m{n}");
            let body=json!({"data":[{"id":id}],"has_more":true,"last_id":id,"padding":"X".repeat(900_000)}).to_string();
            send(s, 200, "", &body);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            l.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let out = command(root.path())
        .args(["models", "--remote"])
        .output()
        .unwrap();
    checked(&out, false);
    assert!(String::from_utf8_lossy(&out.stderr).contains("size limit"));
    server.join().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        let body =
            json!({"data":(0..4096).map(|n|json!({"id":format!("m{n}")})).collect::<Vec<_>>()})
                .to_string();
        send(s, 200, "", &body);
    });
    let out = command(root.path())
        .args(["models", "--remote", "--json"])
        .output()
        .unwrap();
    checked(&out, true);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["models"].as_array().unwrap().len(), 4096);
    assert_eq!(v["models"][4095], "m4095");
    server.join().unwrap();
}

#[test]
fn catalogue_does_not_wait_for_or_read_open_stdin() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let server = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        send(s, 200, "", r#"{"data":[{"id":"reported"}]}"#);
    });
    let mut child = command(root.path())
        .args(["models", "--remote"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut held_input = child.stdin.take().unwrap();
    held_input.write_all(b"INPUT_SECRET_993").unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > end {
            child.kill().unwrap();
            panic!("catalogue waited for task stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().unwrap();
    checked(&out, true);
    assert_eq!(out.stdout, b"reported\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("INPUT_SECRET_993"));
    drop(held_input);
    server.join().unwrap();
}
