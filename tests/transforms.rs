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

#[test]
fn csv_import_keeps_string_cells_and_composes_with_recipes_and_real_pipes_offline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("broken.toml"), "invalid model settings").unwrap();
    let original = "\u{feff}id,amount,note\r\n0007,123456789012345678901234567890,\"中,\"\"文\"\"\r\nline\"\r\n0007,1.2300E+400,\r\n";
    std::fs::write(root.join("table.csv"), original).unwrap();
    let out = invoke(
        root,
        &[
            "--config",
            "broken.toml",
            "data",
            "csv-to-jsonl",
            "-i",
            "table.csv",
            "-o",
            "rows.jsonl",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let expected = "{\"id\":\"0007\",\"amount\":\"123456789012345678901234567890\",\"note\":\"中,\\\"文\\\"\\r\\nline\"}\n{\"id\":\"0007\",\"amount\":\"1.2300E+400\",\"note\":\"\"}\n";
    assert_eq!(
        std::fs::read(root.join("rows.jsonl")).unwrap(),
        expected.as_bytes()
    );
    let plan = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "csv-to-jsonl",
            "--step",
            "jsonl-select:/id",
            "--step",
            "dedupe",
            "--step",
            "jsonl-to-array",
            "-o",
            "ids.toml",
        ],
        b"not CSV",
    );
    assert!(plan.status.success());
    let out = invoke(
        root,
        &["data", "--recipe", "ids.toml", "-i", "table.csv"],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"[\"0007\"]\n");
    let mut first = command(root)
        .args(["data", "csv-to-jsonl", "-i", "table.csv"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .args([
            "data",
            "--step",
            "jsonl-select:/id",
            "--step",
            "dedupe",
            "--step",
            "jsonl-to-array",
        ])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = second.wait_with_output().unwrap();
    assert!(first.wait().unwrap().success() && output.status.success());
    assert_eq!(output.stdout, b"[\"0007\"]\n");
    assert!(output.stderr.is_empty());
    for (args, expected) in [
        (vec!["data", "csv-to-jsonl"], b"".as_slice()),
        (
            vec!["data", "--step", "csv-to-jsonl", "--step", "jsonl-to-array"],
            b"[]\n".as_slice(),
        ),
    ] {
        let out = invoke(root, &args, b"id,name\n");
        assert!(out.status.success());
        assert_eq!(out.stdout, expected);
    }
    assert_eq!(
        std::fs::read(root.join("table.csv")).unwrap(),
        original.as_bytes()
    );
    assert!(!root.join("config").exists());
}

#[test]
fn csv_invalid_headers_and_late_rows_never_publish_partial_json_or_replace_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.jsonl"), b"original\r\n").unwrap();
    for input in [
        "",
        "a,a\n1,2\n",
        "a,\n1,2\n",
        "a,b\n1,2\nPRIVATE\n",
        "a,b\nx,y\n\"PRIVATE",
        "a,b\nx,y\n\"x\"PRIVATE,z",
        "a,b\nx,y\n\n",
        "a,b\nx,y\nPRIVATE\"x,z",
        "a,b\rx,y",
    ] {
        for save in [false, true] {
            let mut args = vec!["data", "csv-to-jsonl"];
            if save {
                args.extend(["-o", "keep.jsonl", "--force"]);
            }
            let out = invoke(root, &args, input.as_bytes());
            assert!(!out.status.success() && out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
            assert_eq!(
                std::fs::read(root.join("keep.jsonl")).unwrap(),
                b"original\r\n"
            );
            assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
        }
    }
    let out = invoke(
        root,
        &["data", "csv-to-jsonl"],
        b"a,b\n\"one\ntwo\",ok\nx\n",
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("record 3"));
    for options in [vec!["--pointer", "/a"], vec!["--reverse"]] {
        let mut child = command(root)
            .args(["data", "csv-to-jsonl"])
            .args(options)
            .args(["-o", "keep.jsonl", "--force"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > end {
                child.kill().ok();
                panic!("invalid CSV options waited for input");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(
            std::fs::read(root.join("keep.jsonl")).unwrap(),
            b"original\r\n"
        );
    }
}

#[test]
fn csv_expansion_and_initial_input_limits_preserve_existing_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.jsonl"), b"original").unwrap();
    let expansion = format!("{}\n{}", "h".repeat(4096), "x\n".repeat(4096));
    let oversized = format!("h\n{}", "x".repeat(8 * 1024 * 1024));
    for (args, input) in [
        (
            vec!["data", "csv-to-jsonl", "-o", "keep.jsonl", "--force"],
            expansion.as_bytes(),
        ),
        (
            vec![
                "--max-input",
                "9437184",
                "data",
                "csv-to-jsonl",
                "-o",
                "keep.jsonl",
                "--force",
            ],
            oversized.as_bytes(),
        ),
    ] {
        let out = invoke(root, &args, input);
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep.jsonl")).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
}

#[test]
fn csv_model_tool_recovers_and_uses_original_input_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let input = "id,note\r\n0007,\"中,\"\"文\"\"\nline\"\r\n0007,\r\n";
        let rows = "{\"id\":\"0007\",\"note\":\"中,\\\"文\\\"\\nline\"}\n{\"id\":\"0007\",\"note\":\"\"}\n";
        let (url, server) = model_turns(
            api,
            vec![
                (
                    json!({"operation":"csv-to-jsonl","reverse":true}),
                    Value::Null,
                ),
                (json!({"operation":"csv-to-jsonl"}), json!(rows)),
                (
                    json!({"steps":[{"operation":"csv-to-jsonl"},{"operation":"jsonl-select","pointer":"/id"},{"operation":"dedupe"},{"operation":"jsonl-to-array"}]}),
                    json!("[\"0007\"]"),
                ),
                (json!({"operation":"csv-to-jsonl"}), json!(rows)),
            ],
        );
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "extract table ids",
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
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn csv_export_offline_recipe_and_process_pipe_keep_scalar_text_and_input_files() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("invalid.toml"), "invalid model config").unwrap();
    let input=b"\xef\xbb\xbf{\"id\":\"0007\",\"n\":123456789012345678901234567890,\"note\":\"a,b\"}\r\n{\"note\":\"one\\r\\ntwo\",\"n\":1.2300E+400,\"id\":\"0008\"}\n";
    std::fs::write(root.join("records.jsonl"), input).unwrap();
    let expected=b"\"id\",\"n\",\"note\"\n\"0007\",\"123456789012345678901234567890\",\"a,b\"\n\"0008\",\"1.2300E+400\",\"one\r\ntwo\"\n";
    let out = invoke(
        root,
        &[
            "--config",
            "invalid.toml",
            "data",
            "jsonl-to-csv",
            "-i",
            "records.jsonl",
            "-o",
            "table.csv",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("table.csv")).unwrap(), expected);
    let plan = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "jsonl-to-csv",
            "-o",
            "export.toml",
        ],
        b"not data",
    );
    assert!(plan.status.success());
    let out = invoke(
        root,
        &[
            "--config",
            "invalid.toml",
            "data",
            "--recipe",
            "export.toml",
            "-i",
            "records.jsonl",
        ],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
    let mut first = command(root)
        .args(["data", "jsonl-to-csv", "-i", "records.jsonl"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .args([
            "data",
            "--step",
            "csv-to-jsonl",
            "--step",
            "jsonl-select:/n",
            "--step",
            "jsonl-to-array",
        ])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let result = second.wait_with_output().unwrap();
    assert!(first.wait().unwrap().success() && result.status.success());
    assert_eq!(
        result.stdout,
        b"[\"123456789012345678901234567890\",\"1.2300E+400\"]\n"
    );
    assert!(result.stderr.is_empty());
    assert_eq!(std::fs::read(root.join("records.jsonl")).unwrap(), input);
    assert!(!root.join("config").exists());
}

#[test]
fn csv_export_bad_late_records_and_options_protect_stdout_and_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.csv"), b"KEEP").unwrap();
    for bad in [
        "",
        "\n",
        "{}",
        "[]",
        "\"PRIVATE\"",
        "{\"PRIVATE\":[]}",
        "{\"x\":1}\n{\"x\":\"ok\"}\n{\"other\":\"PRIVATE\"}\n",
        "{\"x\":1}\n{\"x\":2,\"\\u0078\":3}",
        "{\"x\":1}\n{\"x\":\"PRIVATE\"",
    ] {
        for args in [
            vec!["data", "jsonl-to-csv"],
            vec!["data", "jsonl-to-csv", "-o", "keep.csv", "--force"],
        ] {
            let out = invoke(root, &args, bad.as_bytes());
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
            assert_eq!(std::fs::read(root.join("keep.csv")).unwrap(), b"KEEP");
        }
    }
    let bad = invoke(
        root,
        &["data", "jsonl-to-csv"],
        b"{\"x\":1}\n{\"x\":2}\n{\"y\":3}",
    );
    assert!(String::from_utf8_lossy(&bad.stderr).contains("record 3"));
    for args in [
        vec!["data", "jsonl-to-csv", "--reverse"],
        vec!["data", "jsonl-to-csv", "--pointer", ""],
        vec!["data", "--step", "jsonl-to-csv:reverse"],
    ] {
        let mut child = command(root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let held = child.stdin.take().unwrap();
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "invalid CSV options waited for stdin"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        let out = child.wait_with_output().unwrap();
        assert!(out.stdout.is_empty());
    }
    assert!(!std::fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn csv_export_model_recovers_and_composes_from_original_input_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let input=b"{\"data\":[{\"id\":\"0007\",\"n\":123456789012345678901234567890},{\"n\":1.2300E+400,\"id\":\"0008\"}]}";
        let steps = json!({"steps":[{"operation":"json-select","pointer":"/data"},{"operation":"array-to-jsonl"},{"operation":"jsonl-to-csv"}]});
        let csv =
            "\"id\",\"n\"\n\"0007\",\"123456789012345678901234567890\"\n\"0008\",\"1.2300E+400\"\n";
        let (url, server) = model_turns(
            api,
            vec![
                (json!({"operation":"jsonl-to-csv"}), Value::Null),
                (steps.clone(), json!(csv)),
                (
                    json!({"operation":"jsonl-to-csv","pointer":""}),
                    Value::Null,
                ),
                (steps, json!(csv)),
            ],
        );
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "export records as CSV",
            ],
            input,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"verified\n");
        assert!(out.stderr.is_empty());
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn projection_offline_fields_recipe_and_real_pipe_keep_values_and_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("invalid.toml"), "not a model config").unwrap();
    let input=b"\xef\xbb\xbf{\"payload\":{\"id\":\"0007\",\"n\":123456789012345678901234567890},\"unused\":\"PRIVATE_INPUT\"}\r\n{\"payload\":{\"n\":1.2300E+400,\"id\":\"0008\"}}\n";
    std::fs::write(root.join("events.jsonl"), input).unwrap();
    let expected=b"{\"id\":\"0007\",\"amount\":123456789012345678901234567890}\n{\"id\":\"0008\",\"amount\":1.2300E+400}\n";
    let out = invoke(
        root,
        &[
            "--config",
            "invalid.toml",
            "data",
            "jsonl-project",
            "--field",
            "id=/payload/id",
            "--field",
            "amount=/payload/n",
            "-i",
            "events.jsonl",
        ],
        b"",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
    let plan = invoke(
        root,
        &[
            "--config",
            "invalid.toml",
            "data",
            "jsonl-project",
            "--field",
            "id=/payload/id",
            "--field",
            "amount=/payload/n",
            "--show-plan",
            "-o",
            "project.toml",
        ],
        b"PRIVATE_IGNORED",
    );
    assert!(plan.status.success());
    assert!(plan.stdout.is_empty());
    let definition = std::fs::read_to_string(root.join("project.toml")).unwrap();
    assert!(!definition.contains("PRIVATE"));
    assert!(definition.contains("[[steps.fields]]"));
    let replay = invoke(
        root,
        &["data", "--recipe", "project.toml", "-i", "events.jsonl"],
        b"",
    );
    assert!(replay.status.success());
    assert_eq!(replay.stdout, expected);
    std::fs::write(
        root.join("csv.toml"),
        definition + "\n[[steps]]\noperation = \"jsonl-to-csv\"\n",
    )
    .unwrap();
    let csv=b"\"id\",\"amount\"\n\"0007\",\"123456789012345678901234567890\"\n\"0008\",\"1.2300E+400\"\n";
    let replay = invoke(
        root,
        &[
            "data",
            "--recipe",
            "csv.toml",
            "-i",
            "events.jsonl",
            "-o",
            "table.csv",
        ],
        b"",
    );
    assert!(replay.status.success());
    assert!(replay.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("table.csv")).unwrap(), csv);
    let mut first = command(root)
        .args([
            "data",
            "jsonl-project",
            "--field",
            "id=/payload/id",
            "--field",
            "amount=/payload/n",
            "-i",
            "events.jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = command(root)
        .args(["data", "jsonl-to-csv"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(first.wait().unwrap().success() && out.status.success());
    assert_eq!(out.stdout, csv);
    assert!(out.stderr.is_empty());
    let literal = invoke(
        root,
        &[
            "data",
            "jsonl-project",
            "--field",
            "copy=/a=b",
            "--field",
            "whole=",
        ],
        b"{\"a=b\":-0}",
    );
    assert!(literal.status.success());
    assert_eq!(literal.stdout, b"{\"copy\":-0,\"whole\":{\"a=b\":-0}}\n");
    assert_eq!(std::fs::read(root.join("events.jsonl")).unwrap(), input);
    assert!(!root.join("config").exists());
}

#[test]
fn projection_bad_records_never_publish_prefixes_or_replace_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.jsonl"), b"KEEP").unwrap();
    let prefix = "{\"payload\":{\"id\":1,\"n\":-0}}\n{\"payload\":{\"n\":null,\"id\":2}}\n";
    for bad in [
        "\n",
        "{\"payload\":{\"id\":\"PRIVATE\"}}",
        "{\"payload\":{\"id\":3,\"n\":1,\"n\":2}}",
        "{\"payload\":{\"id\":3,\"n\":\"PRIVATE\"},\"bad\":[1,]}",
        "\"PRIVATE\"",
        "{\"payload\":{\"id\":3,\"n\":\"PRIVATE\"",
    ] {
        for file in [false, true] {
            let mut args = vec![
                "data",
                "jsonl-project",
                "--field",
                "id=/payload/id",
                "--field",
                "amount=/payload/n",
            ];
            if file {
                args.extend(["-o", "keep.jsonl", "--force"]);
            }
            let out = invoke(root, &args, (prefix.to_owned() + bad).as_bytes());
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(error.contains("record 3"), "{error}");
            assert!(!error.contains("PRIVATE"));
            assert_eq!(std::fs::read(root.join("keep.jsonl")).unwrap(), b"KEEP");
        }
    }
    let out = invoke(root, &["data", "jsonl-project", "--field", "whole="], b"");
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(!std::fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn projection_invalid_definitions_fail_before_stdin_or_output_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.toml"), b"KEEP").unwrap();
    std::fs::write(
        root.join("bad.toml"),
        "version = 1\n[[steps]]\noperation = \"trim\"\nfields = []\n",
    )
    .unwrap();
    let cases = vec![
        vec!["data", "jsonl-project"],
        vec!["data", "jsonl-project", "--field", "PRIVATE"],
        vec!["data", "jsonl-project", "--field", "=/a"],
        vec![
            "data",
            "jsonl-project",
            "--field",
            "id=/a",
            "--field",
            "id=/b",
        ],
        vec!["data", "trim", "--field", "id=/PRIVATE"],
        vec!["data", "jsonl-project", "--field", "id=/PRIVATE~2"],
        vec!["data", "jsonl-project", "--field", "id=/a", "--pointer", ""],
        vec!["data", "jsonl-project", "--field", "id=/a", "--reverse"],
        vec!["data", "--step", "jsonl-project", "--field", "id=/a"],
        vec!["data", "--recipe", "bad.toml"],
        vec!["data", "--recipe", "bad.toml", "--show-plan"],
    ];
    for mut args in cases {
        args.extend(["-o", "keep.toml", "--force"]);
        let mut child = command(root)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let held = child.stdin.take().unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(!status.success(), "{args:?}");
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                panic!("invalid projection waited for stdin: {args:?}")
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        let out = child.wait_with_output().unwrap();
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
        assert_eq!(std::fs::read(root.join("keep.toml")).unwrap(), b"KEEP");
    }
    assert!(!std::fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn projection_model_tool_recovers_and_composes_in_every_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let input=b"{\"data\":[{\"payload\":{\"id\":\"0007\",\"n\":123456789012345678901234567890}},{\"payload\":{\"n\":1.2300E+400,\"id\":\"0008\"}}]}";
        let fields =
            json!([{"name":"id","pointer":"/payload/id"},{"name":"amount","pointer":"/payload/n"}]);
        let steps = json!({"steps":[{"operation":"json-select","pointer":"/data"},{"operation":"array-to-jsonl"},{"operation":"jsonl-project","fields":fields},{"operation":"jsonl-to-csv"}]});
        let csv = "\"id\",\"amount\"\n\"0007\",\"123456789012345678901234567890\"\n\"0008\",\"1.2300E+400\"\n";
        let (url, server) = model_turns(
            api,
            vec![
                (
                    json!({"operation":"jsonl-project","fields":[{"name":"id","pointer":"/data/0/PRIVATE"}]}),
                    Value::Null,
                ),
                (steps.clone(), json!(csv)),
                (json!({"operation":"trim","fields":[]}), Value::Null),
                (steps, json!(csv)),
            ],
        );
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "project nested records and export CSV",
            ],
            input,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"verified\n");
        assert!(out.stderr.is_empty());
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

fn model(api: &str, args: Value, expected: Value) -> (String, std::thread::JoinHandle<()>) {
    model_turns(api, vec![(args, expected)])
}

#[test]
fn jsonl_selection_recovers_and_composes_in_every_provider_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let input = "{\"payload\":{\"id\":123456789012345678901234567890}}\n{\"payload\":{\"id\":1.2300E+400}}\n{\"payload\":{\"id\":123456789012345678901234567890}}\n";
        let (url, server) = model_turns(
            api,
            vec![
                (
                    json!({"operation":"jsonl-select","pointer":"/missing/PRIVATE_POINTER"}),
                    Value::Null,
                ),
                (
                    json!({"operation":"jsonl-select","pointer":"/payload/id"}),
                    json!(
                        "123456789012345678901234567890\n1.2300E+400\n123456789012345678901234567890\n"
                    ),
                ),
                (
                    json!({"steps":[{"operation":"jsonl-select","pointer":"/payload/id"},{"operation":"dedupe"},{"operation":"jsonl-to-array"}]}),
                    json!("[123456789012345678901234567890,1.2300E+400]"),
                ),
            ],
        );
        let out = invoke(
            root.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "extract ids",
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
                assert!(ops.contains(&json!("jsonl-select")));
                assert!(ops.contains(&json!("csv-to-jsonl")));
                assert!(ops.contains(&json!("jsonl-to-csv")));
                assert!(ops.contains(&json!("jsonl-project")));
                assert!(ops.contains(&json!("lines-include")));
                assert!(ops.contains(&json!("lines-exclude")));
                assert_eq!(schema["properties"]["contains"]["type"], "string");
                assert_eq!(
                    schema["properties"]["steps"]["items"]["properties"]["contains"],
                    schema["properties"]["contains"]
                );
                assert_eq!(schema["properties"]["fields"]["maxItems"], 64);
                assert_eq!(
                    schema["properties"]["fields"]["items"]["required"],
                    json!(["name", "pointer"])
                );
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
                assert_eq!(
                    steps["items"]["properties"]["fields"],
                    schema["properties"]["fields"]
                );
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

#[test]
fn literal_line_filters_work_offline_and_chain_through_real_process_pipes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("broken.toml"), "broken model configuration").unwrap();
    let source =
        "\u{feff}ERROR a.* 中\r\ninfo ok\r\nERROR a.* 中\nerror a.*\n\nERROR healthcheck\nlast\r";
    std::fs::write(root.join("app.log"), source).unwrap();
    let out = invoke(
        root,
        &[
            "--config",
            "broken.toml",
            "data",
            "lines-include",
            "--contains",
            "a.*",
            "-i",
            "app.log",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        "ERROR a.* 中\nERROR a.* 中\nerror a.*\n".as_bytes()
    );
    assert!(out.stderr.is_empty());
    let mut first = command(root)
        .args([
            "data",
            "--step",
            "lines-include:ERROR",
            "--step",
            "lines-exclude:healthcheck",
            "-i",
            "app.log",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = command(root)
        .args(["data", "dedupe"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(first.wait().unwrap().success() && out.status.success());
    assert_eq!(out.stdout, "ERROR a.* 中\n".as_bytes());
    assert!(out.stderr.is_empty());
    let out = invoke(
        root,
        &[
            "data",
            "lines-exclude",
            "--contains",
            "ERROR",
            "-i",
            "app.log",
        ],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"info ok\nerror a.*\n\nlast\r\n");
    let out = invoke(
        root,
        &["data", "lines-include", "--contains", "not present"],
        b"some input",
    );
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        std::fs::read(root.join("app.log")).unwrap(),
        source.as_bytes()
    );
    assert!(!root.join("config").exists());
}

#[test]
fn line_filter_recipes_export_literals_and_preserve_results_on_later_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let needle = "中:reverse.*\t\"\\";
    let step = format!("lines-include:{needle}");
    let plan = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            &step,
            "--step",
            "dedupe",
            "-o",
            "filter.toml",
        ],
        b"PRIVATE_DATA",
    );
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(plan.stdout.is_empty() && plan.stderr.is_empty());
    let definition = std::fs::read_to_string(root.join("filter.toml")).unwrap();
    let value: toml::Value = toml::from_str(&definition).unwrap();
    assert_eq!(value["steps"][0]["contains"].as_str(), Some(needle));
    assert!(!definition.contains("PRIVATE_DATA"));
    let input = format!("skip\r\n{needle}\r\n{needle}\n");
    let out = invoke(root, &["data", "--recipe", "filter.toml"], input.as_bytes());
    assert!(out.status.success());
    assert_eq!(out.stdout, format!("{needle}\n").as_bytes());
    let again = invoke(
        root,
        &["data", "--show-plan", "--recipe", "filter.toml"],
        b"ignored",
    );
    assert!(again.status.success());
    assert_eq!(again.stdout, definition.as_bytes());
    let out = invoke(
        root,
        &["data", "--step", "lines-include:x:reverse"],
        b"x:reverse\nx\n",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"x:reverse\n");
    std::fs::write(root.join("result.txt"), b"keep original\r\n").unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--step",
            "lines-include:PRIVATE",
            "--step",
            "json-minify",
            "-o",
            "result.txt",
            "--force",
        ],
        b"PRIVATE_NOT_JSON\n",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("step 2 failed"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
    assert_eq!(
        std::fs::read(root.join("result.txt")).unwrap(),
        b"keep original\r\n"
    );
    let out = invoke(
        root,
        &[
            "data",
            "lines-exclude",
            "--contains",
            "original",
            "-i",
            "result.txt",
            "-o",
            "result.txt",
            "--force",
        ],
        b"",
    );
    assert!(out.status.success());
    assert!(std::fs::read(root.join("result.txt")).unwrap().is_empty());
    assert!(!std::fs::read_dir(root).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn invalid_line_filters_fail_and_preview_finishes_while_stdin_stays_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.txt"), b"original").unwrap();
    let long = "界".repeat(1366);
    let mut cases = vec![
        vec!["data", "lines-include"],
        vec!["data", "lines-exclude", "--contains", ""],
        vec!["data", "lines-include", "--contains", "a\nb"],
        vec!["data", "lines-include", "--contains", "a\rb"],
        vec!["data", "lines-include", "--contains", &long],
        vec!["data", "trim", "--contains", "x"],
        vec!["data", "lines-include", "--contains", "x", "--reverse"],
        vec!["data", "lines-exclude", "--contains", "x", "--pointer", ""],
        vec!["data", "--step", "lines-include:"],
        vec!["data", "--step", "trim", "--contains", "x"],
    ];
    for (index, definition) in [
        "version=1\n[[steps]]\noperation='lines-include'\ncontains=7\n",
        "version=1\n[[steps]]\noperation='lines-exclude'\ncontains=''\n",
        "version=1\n[[steps]]\noperation='trim'\ncontains='x'\n",
    ]
    .into_iter()
    .enumerate()
    {
        std::fs::write(root.join(format!("bad{index}.toml")), definition).unwrap();
    }
    cases.extend([
        vec!["data", "--recipe", "bad0.toml"],
        vec!["data", "--recipe", "bad1.toml"],
        vec!["data", "--recipe", "bad2.toml"],
    ]);
    for args in cases {
        let mut child = command(root)
            .args(&args)
            .args(["-o", "keep.txt", "--force"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let held = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > end {
                child.kill().ok();
                panic!("invalid filter waited for stdin");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"original");
    }
    let mut child = command(root)
        .args(["data", "--show-plan", "--step", "lines-exclude:healthcheck"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held = child.stdin.take().unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > end {
            child.kill().ok();
            panic!("filter preview waited for stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(held);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success() && out.stderr.is_empty());
    assert!(String::from_utf8_lossy(&out.stdout).contains("contains = \"healthcheck\""));
    let source = "x\n".repeat(100001);
    let out = invoke(
        root,
        &[
            "data",
            "lines-include",
            "--contains",
            "absent",
            "-o",
            "keep.txt",
            "--force",
        ],
        source.as_bytes(),
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"original");
    assert!(!root.join("config").exists());
}

#[test]
fn line_filter_tool_calls_share_schema_recover_and_restart_from_original_in_all_protocols() {
    let input = "ERROR keep\nINFO original\nERROR healthcheck\nERROR keep\n";
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let calls = vec![
            (
                json!({"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-exclude","contains":"healthcheck"},{"operation":"dedupe"}]}),
                json!("ERROR keep\n"),
            ),
            (
                json!({"operation":"lines-include","contains":"absent"}),
                json!(""),
            ),
            (
                json!({"operation":"lines-include","contains":null}),
                Value::Null,
            ),
            (
                json!({"operation":"trim","contains":"PRIVATE"}),
                Value::Null,
            ),
            (
                json!({"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"json-minify"}]}),
                Value::Null,
            ),
            (
                json!({"operation":"lines-exclude","contains":"ERROR"}),
                json!("INFO original\n"),
            ),
        ];
        let (url, server) = model_turns(api, calls);
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "filter logs",
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
        assert!(!dir.path().join("config").exists());
    }
}

#[test]
fn offline_line_windows_are_exact_and_need_no_model_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("invalid.toml"), "not model configuration").unwrap();
    let input = " a \r\n\r\n a \n尾";
    for (operation, n, expected) in [
        ("lines-head", "3", " a \n\n a \n"),
        ("lines-tail", "2", " a \n尾\n"),
        ("lines-tail", "99", " a \n\n a \n尾\n"),
    ] {
        for flag in ["--lines", "-n"] {
            let out = invoke(
                root,
                &["--config", "invalid.toml", "data", operation, flag, n],
                input.as_bytes(),
            );
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, expected.as_bytes());
            assert!(out.stderr.is_empty());
        }
        assert!(
            invoke(root, &["data", operation, "--lines", n], b"")
                .stdout
                .is_empty()
        );
    }
    assert!(!root.join("config").exists());
}

#[test]
fn invalid_line_windows_fail_before_data_or_output_access() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.txt"), b"keep").unwrap();
    for mut args in [
        vec!["data", "lines-head"],
        vec!["data", "lines-tail", "--lines", "0"],
        vec!["data", "lines-head", "--lines", "100001"],
        vec!["data", "lines-head", "--lines", "1.2"],
        vec!["data", "trim", "--lines", "2"],
        vec!["data", "--step", "lines-tail:2", "--lines", "2"],
        vec!["data", "--step", "lines-head:bad"],
        vec!["data", "--step", "lines-tail:0"],
        vec!["data", "--step", "lines-head:2:reverse"],
    ] {
        args.extend(["-i", "missing-input", "-o", "keep.txt", "--force"]);
        let out = invoke(root, &args, b"ignored");
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!error.contains("cannot open missing-input"), "{error}");
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"keep");
    }
    assert!(!std::fs::read_dir(root).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn line_windows_compose_on_original_input_in_every_model_protocol() {
    for api in ["chat", "responses", "anthropic"] {
        let root = tempfile::tempdir().unwrap();
        let args = json!({"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-tail","lines":2}]});
        let (url, server) = model(api, args, json!("ERROR repeat\nERROR repeat\n"));
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
            b"ERROR old\nINFO middle\nERROR repeat\nERROR repeat\n",
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

#[test]
fn jsonl_where_offline_files_and_real_pipe_keep_records_and_lexical_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("broken.toml"), "not model configuration").unwrap();
    let input=b"\xef\xbb\xbf{\"level\":\"INFO\",\"message\":\"ERROR\"}\r\n {\"id\":1.2300E+400,\"level\":\"\\u0045RROR\",\"x\":1,\"x\":-0} \r\n{\"level\":\"ERROR\",\"id\":123456789012345678901234567890}\n";
    std::fs::write(root.join("events.jsonl"), input).unwrap();
    let expected=b"{\"id\":1.2300E+400,\"level\":\"\\u0045RROR\",\"x\":1,\"x\":-0}\n{\"level\":\"ERROR\",\"id\":123456789012345678901234567890}\n";
    let out = invoke(
        root,
        &[
            "--config",
            "broken.toml",
            "data",
            "jsonl-where",
            "--pointer",
            "/level",
            "--equals",
            "ERROR",
            "-i",
            "events.jsonl",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
    let mut first = command(root)
        .args([
            "data",
            "jsonl-where",
            "--pointer",
            "/level",
            "--equals",
            "ERROR",
            "-i",
            "events.jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let next = command(root)
        .args([
            "data",
            "--step",
            "jsonl-select:/id",
            "--step",
            "jsonl-to-array",
        ])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(first.wait().unwrap().success() && next.status.success());
    assert_eq!(
        next.stdout,
        b"[1.2300E+400,123456789012345678901234567890]\n"
    );
    assert_eq!(std::fs::read(root.join("events.jsonl")).unwrap(), input);
    assert!(!root.join("config").exists());
    for text in [
        b"".as_slice(),
        b"{\"level\":\"INFO\"}\n",
        b"{\"level\":null}\n{\"level\":7}\n{\"level\":true}\n{\"level\":[]}\n",
    ] {
        let out = invoke(
            root,
            &[
                "data",
                "jsonl-where",
                "--pointer",
                "/level",
                "--equals",
                "ERROR",
            ],
            text,
        );
        assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    }
    let empty = invoke(
        root,
        &["data", "jsonl-where", "--pointer", "", "--equals", ""],
        b"\"\"\n\"x\"\nnull",
    );
    assert!(empty.status.success());
    assert_eq!(empty.stdout, b"\"\"\n");
}

#[test]
fn jsonl_where_plan_export_roundtrips_literals_and_composes_with_csv() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let literal = "ERROR\r\n\t界";
    let out = invoke(
        root,
        &[
            "data",
            "jsonl-where",
            "--pointer",
            "/a~1b/~0",
            "--equals",
            literal,
            "--show-plan",
            "-o",
            "plan.toml",
        ],
        b"PRIVATE_IGNORED",
    );
    assert!(out.status.success() && out.stdout.is_empty());
    let plan = std::fs::read_to_string(root.join("plan.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&plan).unwrap();
    assert_eq!(parsed["version"].as_integer(), Some(1));
    assert_eq!(parsed["steps"][0]["equals"].as_str(), Some(literal));
    assert!(!plan.contains("PRIVATE"));
    let source = serde_json::json!({"a/b":{"~":literal},"n":3}).to_string();
    let expected = format!("{source}\n");
    let run = invoke(root, &["data", "--recipe", "plan.toml"], source.as_bytes());
    assert!(run.status.success());
    assert_eq!(run.stdout, expected.as_bytes());
    let preview = invoke(
        root,
        &["data", "--recipe", "plan.toml", "--show-plan"],
        b"ignored",
    );
    assert_eq!(preview.stdout, plan.as_bytes());
    std::fs::write(root.join("csv.toml"),"version=1\n[[steps]]\noperation='csv-to-jsonl'\n[[steps]]\noperation='jsonl-where'\npointer='/level'\nequals='ERROR'\n[[steps]]\noperation='jsonl-project'\nfields=[{name='id',pointer='/id'}]\n[[steps]]\noperation='jsonl-to-csv'\n").unwrap();
    let csv = invoke(
        root,
        &["data", "--recipe", "csv.toml"],
        b"id,level\n0007,INFO\n0008,ERROR\n",
    );
    assert!(csv.status.success());
    assert_eq!(csv.stdout, b"\"id\"\n\"0008\"\n");
    let mut child = command(root)
        .args([
            "data",
            "jsonl-where",
            "--pointer",
            "",
            "--equals",
            "",
            "--show-plan",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held = child.stdin.take().unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > end {
            child.kill().ok();
            panic!("plan export waited for stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(held);
    assert!(child.wait_with_output().unwrap().status.success());
}

#[test]
fn jsonl_where_invalid_definitions_fail_before_stdin_input_output_or_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep"), b"keep").unwrap();
    std::fs::write(root.join("bad-config"), b"invalid settings").unwrap();
    let oversized = "界".repeat(1366);
    let cases = vec![
        vec!["data", "jsonl-where", "--pointer", "/level"],
        vec!["data", "jsonl-where", "--equals", "ERROR"],
        vec!["data", "jsonl-where", "--pointer", "/~2", "--equals", "x"],
        vec![
            "data",
            "jsonl-where",
            "--pointer",
            "",
            "--equals",
            &oversized,
        ],
        vec!["data", "trim", "--equals", ""],
        vec![
            "data",
            "jsonl-where",
            "--pointer",
            "",
            "--equals",
            "x",
            "--contains",
            "x",
        ],
        vec![
            "data",
            "jsonl-where",
            "--pointer",
            "",
            "--equals",
            "x",
            "--reverse",
        ],
        vec!["data", "--step", "jsonl-where"],
        vec!["data", "--step", "trim", "--equals", "x"],
        vec![
            "data",
            "jsonl-where",
            "--pointer",
            "",
            "--equals",
            "x",
            "--show-plan",
            "-i",
            "missing",
        ],
    ];
    for args in cases {
        let mut child = command(root)
            .args(["--config", "bad-config", "-o", "keep", "--force"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let held = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > end {
                child.kill().ok();
                panic!("invalid definition waited for stdin");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"keep");
    }
    for recipe in [
        "version=1\n[[steps]]\noperation='jsonl-where'\npointer=''\nequals=1\n",
        "version=1\n[[steps]]\noperation='jsonl-where'\npointer=''\nequals='x'\nequals='y'\n",
    ] {
        std::fs::write(root.join("bad.toml"), recipe).unwrap();
        let out = invoke(
            root,
            &[
                "data", "--recipe", "bad.toml", "-i", "missing", "-o", "keep", "--force",
            ],
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"keep");
    }
}

#[test]
fn jsonl_where_late_errors_discarded_records_and_overwrite_rules_protect_complete_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep"), b"keep").unwrap();
    for tail in [
        "PRIVATE_INVALID",
        "",
        "{}",
        "{\"level\":\"INFO\",\"level\":\"ERROR\"}",
        "{\"level\":\"\\uD800\"}",
    ] {
        let input = format!("{{\"level\":\"ERROR\"}}\n{tail}\n");
        let out = invoke(
            root,
            &[
                "data",
                "jsonl-where",
                "--pointer",
                "/level",
                "--equals",
                "ERROR",
                "-o",
                "keep",
                "--force",
            ],
            input.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("record 2") && !error.contains("PRIVATE"),
            "{error}"
        );
        assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"keep");
    }
    let oversized = "{\"level\":\"INFO\"}\n".repeat(100001);
    let out = invoke(
        root,
        &[
            "data",
            "jsonl-where",
            "--pointer",
            "/level",
            "--equals",
            "ERROR",
            "-o",
            "keep",
            "--force",
        ],
        oversized.as_bytes(),
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"keep");
    let out = invoke(
        root,
        &[
            "data",
            "jsonl-where",
            "--pointer",
            "/level",
            "--equals",
            "ERROR",
            "-o",
            "keep",
        ],
        b"{\"level\":\"ERROR\"}",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"keep");
    let out = invoke(
        root,
        &[
            "data",
            "jsonl-where",
            "--pointer",
            "/level",
            "--equals",
            "ERROR",
            "-o",
            "keep",
            "--force",
        ],
        b"{\"level\":\"INFO\"}",
    );
    assert!(out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"");
    assert!(!std::fs::read_dir(root).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn jsonl_where_model_recovers_and_composes_on_original_input_in_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let input=b"{\"level\":\"INFO\",\"id\":0,\"message\":\"ERROR\"}\n{\"level\":\"\\u0045RROR\",\"id\":1.2300E+400}\n";
        let filter = json!({"operation":"jsonl-where","pointer":"/level","equals":"ERROR"});
        let pipeline = json!({"steps":[{"operation":"jsonl-where","pointer":"/level","equals":"ERROR"},{"operation":"jsonl-select","pointer":"/id"},{"operation":"jsonl-to-array"}]});
        let (url, server) = model_turns(
            api,
            vec![
                (
                    json!({"operation":"jsonl-where","pointer":"/level","equals":null}),
                    Value::Null,
                ),
                (
                    filter,
                    json!("{\"level\":\"\\u0045RROR\",\"id\":1.2300E+400}\n"),
                ),
                (
                    json!({"operation":"jsonl-where","pointer":"/missing","equals":"PRIVATE"}),
                    Value::Null,
                ),
                (
                    json!({"operation":"jsonl-where","pointer":"/level","equals":"INFO"}),
                    json!("{\"level\":\"INFO\",\"id\":0,\"message\":\"ERROR\"}\n"),
                ),
                (pipeline, json!("[1.2300E+400]")),
            ],
        );
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "filter records",
            ],
            input,
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

#[test]
fn keyed_jsonl_commands_preserve_records_count_decoded_values_and_sort_stably() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input=b"{ \"id\":\"b\",\"time\":\"2\",\"n\":1.2300E+400 }\r\n{\"id\":\"a\",\"time\":\"10\",\"n\":-0}\n{\"id\":\"\\u0062\",\"time\":\"2\",\"n\":9007199254740993}\n";
    let first = "{\"id\":\"b\",\"time\":\"2\",\"n\":1.2300E+400}\n";
    let second = "{\"id\":\"a\",\"time\":\"10\",\"n\":-0}\n";
    let third = "{\"id\":\"\\u0062\",\"time\":\"2\",\"n\":9007199254740993}\n";
    for (args, expected) in [
        (
            vec!["data", "jsonl-dedupe", "--pointer", "/id"],
            format!("{first}{second}"),
        ),
        (
            vec!["data", "jsonl-sort", "--pointer", "/time"],
            format!("{second}{first}{third}"),
        ),
        (
            vec!["data", "jsonl-sort", "--pointer", "/time", "--reverse"],
            format!("{first}{third}{second}"),
        ),
        (
            vec!["data", "jsonl-count-by", "--pointer", "/id"],
            "{\"value\":\"b\",\"count\":2}\n{\"value\":\"a\",\"count\":1}\n".into(),
        ),
    ] {
        let out = invoke(root, &args, input);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes());
        assert!(out.stderr.is_empty());
    }
    for op in ["jsonl-sort", "jsonl-dedupe", "jsonl-count-by"] {
        let out = invoke(root, &["data", op, "--pointer", ""], b"");
        assert!(out.status.success());
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn keyed_jsonl_recipes_preview_roundtrip_and_run_through_a_real_pipe() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let args = [
        "data",
        "--show-plan",
        "--step",
        "csv-to-jsonl",
        "--step",
        "jsonl-dedupe:/id",
        "--step",
        "jsonl-sort:/time:reverse",
        "--step",
        "jsonl-count-by:/level",
        "--step",
        "jsonl-to-csv",
        "-o",
        "plan.toml",
    ];
    let out = invoke(root, &args, b"not data");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    let plan = std::fs::read_to_string(root.join("plan.toml")).unwrap();
    assert!(plan.contains("/time:reverse"));
    assert!(!plan.contains("reverse = true"));
    let input = b"id,time:reverse,level\nb,2,WARN\na,1,ERROR\nb,2,WARN\nc,1,ERROR\n";
    let out = invoke(root, &["data", "--recipe", "plan.toml"], input);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        b"\"value\",\"count\"\n\"ERROR\",\"2\"\n\"WARN\",\"1\"\n"
    );
    let mut first = command(root)
        .args(["data", "jsonl-count-by", "--pointer", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .args(["data", "jsonl-to-csv"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    first
        .stdin
        .take()
        .unwrap()
        .write_all(b"\"a\"\n\"b\"\n\"a\"\n")
        .unwrap();
    assert!(first.wait().unwrap().success());
    let out = second.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        b"\"value\",\"count\"\n\"a\",\"2\"\n\"b\",\"1\"\n"
    );
}

#[test]
fn keyed_jsonl_rejects_definitions_before_input_and_preserves_files_on_late_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep"), b"untouched").unwrap();
    for op in ["jsonl-sort", "jsonl-dedupe", "jsonl-count-by"] {
        for options in [
            vec![],
            vec!["--pointer", "/~2"],
            vec!["--pointer", "", "--equals", "x"],
            vec!["--pointer", "", "--lines", "1"],
        ] {
            let mut args = vec!["data", op, "-i", "DOES_NOT_EXIST", "-o", "keep", "--force"];
            args.extend(options);
            let out = invoke(root, &args, b"");
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("cannot open DOES_NOT_EXIST"));
            assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"untouched");
        }
        for bad in [
            b"{\"k\":\"a\"}\n{\"k\":1,\"payload\":\"PRIVATE\"}".as_slice(),
            b"{\"k\":\"a\"}\n{\"k\":\"a\",\"bad\":[1,],\"payload\":\"PRIVATE\"}".as_slice(),
        ] {
            for file in [false, true] {
                let mut args = vec!["data", op, "--pointer", "/k"];
                if file {
                    args.extend(["-o", "keep", "--force"]);
                }
                let out = invoke(root, &args, bad);
                assert!(!out.status.success());
                assert!(out.stdout.is_empty());
                let error = String::from_utf8_lossy(&out.stderr);
                assert!(error.contains("record 2"));
                assert!(!error.contains("PRIVATE"));
                assert_eq!(std::fs::read(root.join("keep")).unwrap(), b"untouched");
            }
        }
    }
    assert!(std::fs::read_dir(root).unwrap().all(|p| {
        !p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zi-result-")
    }));
}

#[test]
fn keyed_jsonl_tools_recover_use_original_input_and_compose_in_all_protocols() {
    for api in ["chat", "responses", "anthropic"] {
        let dir = tempfile::tempdir().unwrap();
        let input=b"{\"id\":\"b\",\"level\":\"WARN\",\"t\":\"2\"}\n{\"id\":\"a\",\"level\":\"ERROR\",\"t\":\"1\"}\n{\"id\":\"\\u0062\",\"level\":\"WARN\",\"t\":\"2\"}\n";
        let (url, server) = model_turns(
            api,
            vec![
                (
                    json!({"operation":"jsonl-count-by","pointer":null}),
                    Value::Null,
                ),
                (
                    json!({"operation":"jsonl-count-by","pointer":"/level"}),
                    json!("{\"value\":\"WARN\",\"count\":2}\n{\"value\":\"ERROR\",\"count\":1}\n"),
                ),
                (
                    json!({"operation":"jsonl-dedupe","pointer":"/id"}),
                    json!(
                        "{\"id\":\"b\",\"level\":\"WARN\",\"t\":\"2\"}\n{\"id\":\"a\",\"level\":\"ERROR\",\"t\":\"1\"}\n"
                    ),
                ),
                (
                    json!({"operation":"jsonl-sort","pointer":"/missing"}),
                    Value::Null,
                ),
                (
                    json!({"steps":[{"operation":"jsonl-dedupe","pointer":"/id"},{"operation":"jsonl-sort","pointer":"/t","reverse":true},{"operation":"jsonl-count-by","pointer":"/level"},{"operation":"jsonl-to-csv"}]}),
                    json!("\"value\",\"count\"\n\"WARN\",\"1\"\n\"ERROR\",\"1\"\n"),
                ),
            ],
        );
        let out = invoke(
            dir.path(),
            &[
                "--api",
                api,
                "--base-url",
                &url,
                "--model",
                "fixture",
                "analyze records",
            ],
            input,
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
