use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
fn command(root: &Path, timeout: &str) -> Command {
    let binary =
        std::env::var_os("ZISTRAL_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_zi").into());
    let mut c = Command::new(binary);
    for (k, _) in std::env::vars() {
        if k.to_uppercase().starts_with("ZI_")
            || matches!(
                k.to_uppercase().as_str(),
                "OPENAI_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY"
            )
        {
            c.env_remove(k);
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
fn listener() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    (l, url)
}
fn config(root: &Path, url: &str, api: &str, extra: &str) {
    std::fs::write(
        root.join("config.toml"),
        format!(
            "model='fixture'\nbase_url='{url}'\napi='{api}'\napi_key='AUTH_SECRET_993'\n{extra}\n"
        ),
    )
    .unwrap();
}
fn accept(l: &TcpListener) -> (TcpStream, Value) {
    let end = Instant::now() + Duration::from_secs(10);
    let s = loop {
        match l.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    };
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    let mut reader = BufReader::new(s.try_clone().unwrap());
    let mut n = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                n = v.trim().parse::<usize>().unwrap();
            }
        }
    }
    assert!(n < 1048576);
    let mut b = vec![0; n];
    reader.read_exact(&mut b).unwrap();
    (s, serde_json::from_slice(&b).unwrap())
}
fn send(mut s: TcpStream, status: u16, kind: &str, body: &str) {
    let out = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(out.as_bytes()).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
}
fn answer(api: &str, complete: bool) -> String {
    match api{"responses"=>json!({"status":if complete{"completed"}else{"incomplete"},"output":[{"type":"message","content":[{"type":"output_text","text":"ANSWER_SECRET_993"}]}]}),"anthropic"=>json!({"stop_reason":if complete{"end_turn"}else{"max_tokens"},"content":[{"type":"text","text":"ANSWER_SECRET_993"}]}),_=>json!({"choices":[{"finish_reason":if complete{"stop"}else{"length"},"message":{"content":"ANSWER_SECRET_993"}}]})}.to_string()
}
fn report(out: &Output, success: bool) -> Value {
    assert_eq!(
        out.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value =
        serde_json::from_slice(&out.stdout).expect("stdout must contain one complete JSON report");
    assert_eq!(v["version"], 1);
    assert_eq!(v["kind"], "doctor-report");
    assert_eq!(v["ok"], success);
    assert!(out.stdout.len() <= 65536);
    for secret in [
        "AUTH_SECRET_993",
        "ANSWER_SECRET_993",
        "BODY_SECRET_993",
        "PRIVATE_FIELD_SECRET_993",
    ] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(secret));
    }
    v
}

#[test]
fn doctor_json_is_opt_in_offline_and_reports_the_effective_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(
        root.path(),
        &url,
        "chat",
        "max_output_tokens=128\nchat_token_limit_field='max_tokens'",
    );
    let before = std::fs::read(root.path().join("config.toml")).unwrap();
    let out = command(root.path(), "3")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    let v = report(&out, true);
    assert!(out.stderr.is_empty());
    assert_eq!(v["configuration"]["status"], "ok");
    let d = &v["configuration"]["details"];
    assert_eq!(d["profile"], "default");
    assert_eq!(d["model"], "fixture");
    assert_eq!(d["credential"]["configured"], true);
    assert_eq!(d["credential"]["source"], "profile");
    assert_eq!(d["output_budget"]["tokens_per_turn"], 128);
    assert_eq!(d["output_budget"]["field"], "max_tokens");
    assert_eq!(d["output_budget"]["tokens_source"], "profile");
    assert_eq!(v["probe"]["requested"], false);
    assert_eq!(v["probe"]["status"], "not_requested");
    assert_eq!(v["probe"]["max_requests"], 0);
    assert!(v["error"].is_null());
    assert!(l.accept().is_err());
    assert_eq!(
        std::fs::read(root.path().join("config.toml")).unwrap(),
        before
    );
    let ignored = command(root.path(), "3")
        .env("ZI_API_KEY", "AUTH_SECRET_993")
        .args(["--no-profile-budget", "doctor", "--json"])
        .output()
        .unwrap();
    let ignored = report(&ignored, true);
    assert_eq!(
        ignored["configuration"]["details"]["credential"]["source"],
        "environment"
    );
    assert_eq!(
        ignored["configuration"]["details"]["credential"]["environment_variable"],
        "ZI_API_KEY"
    );
    assert!(ignored["configuration"]["details"]["output_budget"]["tokens_per_turn"].is_null());
    assert_eq!(
        ignored["configuration"]["details"]["output_budget"]["profile_preferences_ignored"],
        true
    );
    assert!(l.accept().is_err());
    let human = command(root.path(), "3").arg("doctor").output().unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).starts_with("zi doctor"));
    assert!(serde_json::from_slice::<Value>(&human.stdout).is_err());
    let wrong = command(root.path(), "3")
        .args(["--json", "doctor"])
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());
    assert!(l.accept().is_err());
    let out = command(root.path(), "3")
        .args(["--max-output-tokens", "256", "doctor", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn doctor_json_config_errors_are_safe_json_and_never_probe() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    for text in [
        "model='PRIVATE_FIELD_SECRET_993'\napi_key='AUTH_SECRET_993'\nbroken=[".into(),
        "model='fixture'\nbase_url='https://AUTH_SECRET_993@example.com/v1'\n".to_owned(),
        format!(
            "model='fixture'\nbase_url='{url}'\nmax_output_tokens=0\napi_key='AUTH_SECRET_993'\n"
        ),
        format!("model='fixture'\nbase_url='{url}'\nchat_token_limit_field='max_tokens'\n"),
    ] {
        std::fs::write(root.path().join("config.toml"), &text).unwrap();
        let out = command(root.path(), "2")
            .args(["doctor", "--json", "--probe"])
            .output()
            .unwrap();
        let v = report(&out, false);
        assert_eq!(v["configuration"]["status"], "failed");
        assert!(v["configuration"]["details"].is_null());
        assert_eq!(v["error"]["kind"], "configuration");
        assert_eq!(v["probe"]["status"], "not_run");
        assert!(l.accept().is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("config.toml")).unwrap(),
            text
        );
    }
    std::fs::remove_file(root.path().join("config.toml")).unwrap();
    let out = command(root.path(), "2")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "configuration");
    assert!(l.accept().is_err());
}

#[test]
fn doctor_json_probe_success_uses_fixed_budget_in_each_protocol() {
    for (api, prefs, field) in [
        ("chat", "max_output_tokens=128", "max_completion_tokens"),
        (
            "chat",
            "max_output_tokens=128\nchat_token_limit_field='max_tokens'",
            "max_tokens",
        ),
        ("responses", "max_output_tokens=128", "max_output_tokens"),
        ("anthropic", "max_output_tokens=128", "max_tokens"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (l, url) = listener();
        config(root.path(), &url, api, prefs);
        let protocol = api.to_owned();
        let worker = std::thread::spawn(move || {
            let (s, r) = accept(&l);
            assert_eq!(r[field], 16);
            assert_eq!(r["stream"], false);
            assert!(r.get("tools").is_none());
            assert!(r.to_string().len() < 1024);
            send(s, 200, "application/json", &answer(&protocol, true));
        });
        let out = command(root.path(), "3")
            .args(["doctor", "--json", "--probe"])
            .output()
            .unwrap();
        worker.join().unwrap();
        let v = report(&out, true);
        assert_eq!(v["probe"]["status"], "ok");
        assert_eq!(v["probe"]["max_requests"], 1);
        assert_eq!(v["probe"]["max_output_tokens"], 16);
        assert_eq!(v["probe"]["field"], field);
        assert!(v["probe"]["elapsed_ms"].is_number());
        assert_eq!(
            v["configuration"]["details"]["output_budget"]["tokens_per_turn"],
            128
        );
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn doctor_json_http_errors_keep_status_and_hide_provider_bodies() {
    for api in ["chat", "responses", "anthropic"] {
        for status in [302, 401, 403, 429, 500] {
            let root = tempfile::tempdir().unwrap();
            let (l, url) = listener();
            let extra = l.try_clone().unwrap();
            extra.set_nonblocking(true).unwrap();
            config(root.path(), &url, api, "");
            let worker = std::thread::spawn(move || {
                let (s, _) = accept(&l);
                send(
                    s,
                    status,
                    "application/json",
                    "AUTH_SECRET_993 BODY_SECRET_993",
                );
            });
            let out = command(root.path(), "3")
                .args(["doctor", "--json", "--probe"])
                .output()
                .unwrap();
            worker.join().unwrap();
            let v = report(&out, false);
            assert_eq!(v["configuration"]["status"], "ok");
            assert_eq!(v["probe"]["status"], "failed");
            assert_eq!(v["error"]["kind"], "http");
            assert_eq!(v["error"]["http_status"], status);
            assert_eq!(v["error"]["retries"], 0);
            assert!(extra.accept().is_err());
        }
    }
}

#[test]
fn doctor_json_response_errors_do_not_expose_answers_or_execute_tools() {
    for api in ["chat", "responses", "anthropic"] {
        for malformed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (l, url) = listener();
            let extra = l.try_clone().unwrap();
            extra.set_nonblocking(true).unwrap();
            config(root.path(), &url, api, "");
            let protocol = api.to_owned();
            let worker = std::thread::spawn(move || {
                let (s, _) = accept(&l);
                let body = if malformed {
                    "{ BODY_SECRET_993 AUTH_SECRET_993".into()
                } else {
                    answer(&protocol, false)
                };
                send(s, 200, "application/json", &body);
            });
            let out = command(root.path(), "3")
                .args(["doctor", "--json", "--probe"])
                .output()
                .unwrap();
            worker.join().unwrap();
            let v = report(&out, false);
            assert_eq!(v["error"]["kind"], "response");
            assert!(v["error"]["http_status"].is_null());
            assert!(extra.accept().is_err());
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let worker = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        send(s,200,"application/json",&json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"c","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"must-not-exist\",\"content\":\"BODY_SECRET_993\"}"}}]}}]}).to_string());
    });
    let out = command(root.path(), "3")
        .args(["doctor", "--json", "--probe"])
        .output()
        .unwrap();
    worker.join().unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "response");
    assert!(!root.path().join("must-not-exist").exists());
}

#[test]
fn doctor_json_network_and_timeout_errors_are_redacted() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    config(
        root.path(),
        &(url.replacen("http://", "https://", 1) + "/BODY_SECRET_993"),
        "chat",
        "",
    );
    let worker = std::thread::spawn(move || {
        for _ in 0..3 {
            let end = Instant::now() + Duration::from_secs(10);
            let mut s = loop {
                match l.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end);
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            s.set_nonblocking(false).unwrap();
            s.write_all(b"not a TLS record\r\n").unwrap();
            s.shutdown(std::net::Shutdown::Write).unwrap();
        }
    });
    let out = command(root.path(), "5")
        .args(["doctor", "--json", "--probe"])
        .output()
        .unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "network");
    for args in [
        &["doctor", "--probe"][..],
        &["--no-tools", "--no-stream", "task"],
    ] {
        let out = command(root.path(), "5").args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        if args[0] != "doctor" {
            assert!(out.stdout.is_empty());
        }
        let stdout = String::from_utf8(out.stdout).unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("check service availability, TLS"));
        for hidden in ["BODY_SECRET_993", "AUTH_SECRET_993", "https://"] {
            assert!(!stdout.contains(hidden) && !stderr.contains(hidden));
        }
    }
    worker.join().unwrap();
    let (l, url) = listener();
    std::fs::write(
        root.path().join("config.toml"),
        format!("model='fixture'\nbase_url='{url}'\napi_key=\"AUTH_SECRET_993\\nINVALID\"\n"),
    )
    .unwrap();
    let out = command(root.path(), "3")
        .args(["doctor", "--json", "--probe"])
        .output()
        .unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "request");
    assert!(l.accept().is_err());
    let (l, url) = listener();
    config(root.path(), &url, "chat", "");
    let worker = std::thread::spawn(move || {
        let (s, _) = accept(&l);
        std::thread::sleep(Duration::from_secs(2));
        drop(s);
    });
    let out = command(root.path(), "1")
        .args(["doctor", "--json", "--probe"])
        .output()
        .unwrap();
    worker.join().unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "timeout");
}

#[test]
fn doctor_json_report_limit_is_checked_before_probe_without_partial_stdout() {
    let root = tempfile::tempdir().unwrap();
    let (l, url) = listener();
    let model = "PRIVATE_FIELD_SECRET_993".to_owned() + &"x".repeat(70000);
    std::fs::write(
        root.path().join("config.toml"),
        format!("model='{model}'\nbase_url='{url}'\n"),
    )
    .unwrap();
    let out = command(root.path(), "2")
        .args(["doctor", "--json", "--probe"])
        .output()
        .unwrap();
    let v = report(&out, false);
    assert_eq!(v["error"]["kind"], "report_limit");
    assert_eq!(v["probe"]["status"], "not_run");
    assert!(l.accept().is_err());
}

#[test]
fn doctor_json_configuration_errors_expose_only_typed_actionable_codes() {
    let root = tempfile::tempdir().unwrap();
    let (listener, url) = listener();
    let valid =
        format!("model='PRIVATE_FIELD_SECRET_993'\nbase_url='{url}'\napi_key='AUTH_SECRET_993'\n");
    let cases:[(Option<Vec<u8>>,&str,&str);12]=[
        (None,"config_file_unreadable","UTF-8"),
        (Some(vec![0xff,0xfe]),"config_file_unreadable","UTF-8"),
        (Some(b"model='PRIVATE_FIELD_SECRET_993'\napi_key='AUTH_SECRET_993'\nbroken=[".to_vec()),"config_format_invalid","TOML"),
        (Some(format!("{valid}active_profile='PRIVATE_UNKNOWN_PROFILE_993'\n").into_bytes()),"profile_invalid","profile"),
        (Some(format!("base_url='{url}'\napi_key='AUTH_SECRET_993'\n").into_bytes()),"model_missing","model"),
        (Some(b"model='PRIVATE_FIELD_SECRET_993'\nbase_url='https://AUTH_SECRET_993@example.com/v1?PRIVATE_QUERY_993'\n".to_vec()),"service_url_invalid","URL"),
        (Some(format!("{valid}api='PRIVATE_PROTOCOL_993'\n").into_bytes()),"protocol_invalid","chat"),
        (Some(format!("{valid}max_output_tokens=0\n").into_bytes()),"output_budget_invalid","cap"),
        (Some(format!("{valid}max_output_tokens=128\nchat_token_limit_field='PRIVATE_PARAMETER_993'\n").into_bytes()),"output_budget_invalid","cap"),
        (Some(b"model='PRIVATE_FIELD_SECRET_993'\nbase_url='https://api.openai.com/v1'\napi_key_env='ZISTRAL_DIAG_MISSING_993'\n".to_vec()),"credential_missing","key"),
        (Some(format!("{valid}endpoint='https://PRIVATE_ENDPOINT_993.example/v1'\n").into_bytes()),"endpoint_invalid","endpoint"),
        (Some(format!("{valid}endpoint='https://AUTH_SECRET_993@example.com/v1'\n").into_bytes()),"endpoint_invalid","endpoint"),
    ];
    let path = root.path().join("config.toml");
    for (bytes, code, advice) in cases {
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        if let Some(bytes) = &bytes {
            std::fs::write(&path, bytes).unwrap();
        }
        for probe in [false, true] {
            let mut c = command(root.path(), "2");
            c.env("ZISTRAL_DIAG_MISSING_993", "")
                .args(["doctor", "--json"]);
            if probe {
                c.arg("--probe");
            }
            let out = c.output().unwrap();
            let value = report(&out, false);
            assert_eq!(value["error"]["kind"], "configuration");
            assert_eq!(
                value["error"]["code"], code,
                "missing/incorrect typed code for configuration failure"
            );
            assert!(value["error"]["message"].as_str().unwrap().contains(advice));
            assert!(value["error"]["http_status"].is_null() && value["error"]["retries"].is_null());
            assert!(value["configuration"]["details"].is_null());
            assert_eq!(
                value["probe"]["status"],
                if probe { "not_run" } else { "not_requested" }
            );
            assert!(listener.accept().is_err());
            assert_eq!(std::fs::read(&path).ok(), bytes);
            assert!(!root.path().join("config.toml.lock").exists());
            for marker in [
                "PRIVATE_UNKNOWN_PROFILE_993",
                "PRIVATE_QUERY_993",
                "PRIVATE_PROTOCOL_993",
                "PRIVATE_PARAMETER_993",
                "PRIVATE_ENDPOINT_993",
                "ZISTRAL_DIAG_MISSING_993",
            ] {
                assert!(!String::from_utf8_lossy(&out.stdout).contains(marker));
                assert!(!String::from_utf8_lossy(&out.stderr).contains(marker));
            }
        }
    }
}

#[test]
fn doctor_configuration_codes_follow_effective_overrides_not_unused_settings() {
    let root = tempfile::tempdir().unwrap();
    let (listener, url) = listener();
    let text = format!(
        "model='root-model'\nbase_url='{url}'\nactive_profile='broken'\n[profiles.broken]\nbase_url='{url}'\napi='obsolete'\nmax_output_tokens=0\n"
    );
    std::fs::write(root.path().join("config.toml"), &text).unwrap();
    for flags in [
        vec!["-p", "default"],
        vec![
            "-p",
            "broken",
            "--model",
            "override-model",
            "--api",
            "chat",
            "--no-profile-budget",
        ],
    ] {
        let out = command(root.path(), "2")
            .args(flags)
            .args(["doctor", "--json"])
            .output()
            .unwrap();
        let value = report(&out, true);
        assert!(value["error"].is_null());
        assert_eq!(value["configuration"]["status"], "ok");
        assert_eq!(value["probe"]["status"], "not_requested");
        assert!(listener.accept().is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("config.toml")).unwrap(),
            text
        );
    }
}
