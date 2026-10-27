use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
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
                "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"
            )
        {
            c.env_remove(name);
        }
    }
    c.env("APPDATA", root.join("config"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("ZI_CONFIG", root.join("invalid-model-config.toml"));
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
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

#[test]
fn json_selection_plan_roundtrips_nested_pointer_and_exact_records() {
    let root = tempfile::tempdir().unwrap();
    let input = r#"{"data":{"a/b":{"~items:reverse":[{"n":1.2300E+400,"n":-0},"\u4e2d",123456789012345678901234567890]}}}"#;
    let out = invoke(
        root.path(),
        &[
            "data",
            "--show-plan",
            "--step",
            "json-select:/data",
            "--step",
            "json-select:/a~1b/~0items:reverse",
            "--step",
            "array-to-jsonl",
            "-o",
            "records.toml",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let plan = std::fs::read_to_string(root.path().join("records.toml")).unwrap();
    assert!(plan.contains("pointer = \"/a~1b/~0items:reverse\""));
    let records = invoke(
        root.path(),
        &["data", "--recipe", "records.toml"],
        input.as_bytes(),
    );
    assert!(
        records.status.success(),
        "{}",
        String::from_utf8_lossy(&records.stderr)
    );
    assert_eq!(
        records.stdout,
        b"{\"n\":1.2300E+400,\"n\":-0}\n\"\\u4e2d\"\n123456789012345678901234567890\n"
    );
    assert!(records.stderr.is_empty());
    let preview = invoke(
        root.path(),
        &["data", "--show-plan", "--recipe", "records.toml"],
        b"not JSON",
    );
    assert!(preview.status.success());
    assert_eq!(preview.stdout, plan.as_bytes());
    assert!(!root.path().join("config").exists());
}

#[test]
fn jsonl_selection_exports_reusable_plans_and_pipes_exact_values_offline() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("invalid-model-config.toml"),
        "broken config",
    )
    .unwrap();
    let source = "\u{feff}{\"a/b\":{\"~id:reverse\":123456789012345678901234567890}}\r\n{\"a/b\":{\"~id:reverse\":1.2300E+400}}\n{\"a/b\":{\"~id:reverse\":123456789012345678901234567890}}";
    std::fs::write(root.path().join("records.jsonl"), source).unwrap();
    let plan = invoke(
        root.path(),
        &[
            "data",
            "--show-plan",
            "--step",
            "jsonl-select:/a~1b/~0id:reverse",
            "--step",
            "dedupe",
            "--step",
            "jsonl-to-array",
            "-o",
            "ids.toml",
        ],
        b"not input data",
    );
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(plan.stdout.is_empty() && plan.stderr.is_empty());
    let recipe = std::fs::read(root.path().join("ids.toml")).unwrap();
    let out = invoke(
        root.path(),
        &["data", "--show-plan", "--recipe", "ids.toml"],
        b"ignored",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, recipe);
    let out = invoke(
        root.path(),
        &[
            "data",
            "--recipe",
            "ids.toml",
            "-i",
            "records.jsonl",
            "-o",
            "ids.json",
        ],
        b"ignored",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        std::fs::read(root.path().join("ids.json")).unwrap(),
        b"[123456789012345678901234567890,1.2300E+400]\n"
    );
    assert_eq!(
        std::fs::read(root.path().join("records.jsonl")).unwrap(),
        source.as_bytes()
    );
    let mut first = command(root.path())
        .args([
            "data",
            "jsonl-select",
            "--pointer",
            "/a~1b/~0id:reverse",
            "-i",
            "records.jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root.path())
        .args(["data", "jsonl-to-array"])
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = second.wait_with_output().unwrap();
    assert!(first.wait().unwrap().success() && out.status.success());
    assert_eq!(
        out.stdout,
        b"[123456789012345678901234567890,1.2300E+400,123456789012345678901234567890]\n"
    );
    let out = invoke(
        root.path(),
        &[
            "data",
            "--step",
            "json-select:/items",
            "--step",
            "array-to-jsonl",
            "--step",
            "jsonl-select:/id",
            "--step",
            "jsonl-to-array",
        ],
        br#"{"items":[{"id":-0},{"id":"\u4e2d\n"}]}"#,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"[-0,\"\\u4e2d\\n\"]\n");
    for (input, expected) in [("", ""), ("null\r\nfalse\n[ 1,2 ]", "null\nfalse\n[1,2]\n")] {
        let out = invoke(
            root.path(),
            &["data", "--step", "jsonl-select:"],
            input.as_bytes(),
        );
        assert!(out.status.success());
        assert_eq!(out.stdout, expected.as_bytes());
    }
    assert!(!root.path().join("config").exists());
}

#[test]
fn jsonl_selection_late_errors_never_publish_a_prefix_or_replace_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.jsonl"), b"original\r\n").unwrap();
    for second in [
        "{}",
        "{\"x\":1,\"x\":2}",
        "PRIVATE_RECORD",
        "",
        "{\"x\":1} trailing",
        "\u{feff}{\"x\":1}",
        "{\"x\":1,\"hidden\":INVALID}",
    ] {
        let input = format!("{{\"x\":\"PRIVATE_VALUE\"}}\n{second}\n");
        for save in [false, true] {
            let mut args = vec![
                "data",
                "--step",
                "jsonl-select:/x",
                "--step",
                "jsonl-to-array",
            ];
            if save {
                args.extend(["-o", "keep.jsonl", "--force"]);
            }
            let out = invoke(root.path(), &args, input.as_bytes());
            assert!(!out.status.success() && out.stdout.is_empty());
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(
                error.contains("step 1 failed") && error.contains("record 2"),
                "{error}"
            );
            assert!(!error.contains("PRIVATE"));
            assert_eq!(
                std::fs::read(root.path().join("keep.jsonl")).unwrap(),
                b"original\r\n"
            );
        }
    }
    for pointer in ["/01", "/-", "/9"] {
        let out = invoke(
            root.path(),
            &["data", "jsonl-select", "--pointer", pointer],
            b"[1,2]\n",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn jsonl_selection_invalid_definitions_fail_without_waiting_for_stdin() {
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.txt"), b"original").unwrap();
    for spec in [
        vec!["data", "jsonl-select"],
        vec!["data", "--step", "jsonl-select"],
        vec!["data", "jsonl-select", "--pointer", "bad"],
        vec!["data", "--step", "jsonl-select:/x~2"],
        vec!["data", "jsonl-select", "--pointer", "", "--reverse"],
    ] {
        let mut child = command(root.path())
            .args(spec)
            .args(["-o", "keep.txt", "--force"])
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
                panic!("invalid JSONL plan waited for input");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("keep.txt")).unwrap(),
            b"original"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn pipeline_json_selection_keeps_strings_quoted_and_empty_pointer_explicit() {
    let root = tempfile::tempdir().unwrap();
    for (args, input, expected) in [
        (
            vec!["data", "json-select", "--pointer", "/x"],
            r#"{"x":"\u4e2d\ntext"}"#,
            "\"\\u4e2d\\ntext\"\n",
        ),
        (
            vec!["data", "--step", "json-select:", "--step", "json-minify"],
            r#""\u4e2d""#,
            "\"\\u4e2d\"\n",
        ),
        (
            vec!["data", "json-select", "--pointer", ""],
            " null ",
            "null\n",
        ),
        (
            vec!["data", "json-select", "--pointer", "/"],
            r#"{"":false}"#,
            "false\n",
        ),
        (
            vec!["data", "json-select", "--pointer", "/a:reverse"],
            r#"{"a:reverse":[]}"#,
            "[]\n",
        ),
        (
            vec!["--select", "/x"],
            r#"{"x":"\u4e2d\ntext"}"#,
            "中\ntext\n",
        ),
    ] {
        let out = invoke(root.path(), &args, input.as_bytes());
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes());
        assert!(out.stderr.is_empty());
    }
    let plan = invoke(
        root.path(),
        &["data", "--show-plan", "--step", "json-select:"],
        b"",
    );
    assert!(plan.status.success());
    assert!(String::from_utf8_lossy(&plan.stdout).contains("pointer = \"\""));
}

#[test]
fn invalid_selection_definitions_exit_before_input_and_output_access() {
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.json"), b"original\r\n").unwrap();
    for args in [
        vec!["data", "json-select"],
        vec!["data", "trim", "--pointer", ""],
        vec!["data", "json-select", "--pointer", "bad"],
        vec!["data", "--step", "json-select:/x~2"],
        vec!["data", "json-select", "--pointer", "", "--reverse"],
    ] {
        let mut child = command(root.path())
            .args(&args)
            .args(["-o", "keep.json", "--force"])
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
                panic!("invalid plan waited for stdin: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("keep.json")).unwrap(),
            b"original\r\n"
        );
    }
    for spec in [
        "operation='json-select'\npointer=3",
        "operation='json-select'\npointer='/x~2'",
        "operation='trim'\npointer=''",
        "operation='json-select'",
    ] {
        std::fs::write(
            root.path().join("bad.toml"),
            format!("version=1\n[[steps]]\n{spec}\n"),
        )
        .unwrap();
        let out = invoke(
            root.path(),
            &[
                "data",
                "--recipe",
                "bad.toml",
                "--show-plan",
                "-o",
                "keep.json",
                "--force",
            ],
            b"PRIVATE_DATA",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("keep.json")).unwrap(),
            b"original\r\n"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn selection_runtime_failures_preserve_files_and_report_the_failing_step() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.json"), b"original\r\n").unwrap();
    for input in [
        r#"{"x":1,"x":2}"#,
        r#"{"y":1}"#,
        r#"{"x":1} trailing"#,
        r#"{"x":"PRIVATE_VALUE"}"#,
    ] {
        let out = invoke(
            root.path(),
            &[
                "data",
                "--step",
                "json-select:/x",
                "--step",
                "array-to-jsonl",
                "-o",
                "keep.json",
                "--force",
            ],
            input.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("step 1 failed") || error.contains("step 2 failed"),
            "{error}"
        );
        assert!(!error.contains("PRIVATE_VALUE"));
        assert_eq!(
            std::fs::read(root.path().join("keep.json")).unwrap(),
            b"original\r\n"
        );
    }
    for pointer in ["/01", "/-", "/9999999999999999999999999999999"] {
        let out = invoke(
            root.path(),
            &["data", "json-select", "--pointer", pointer],
            b"[1,2]",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn selection_recipe_replaces_input_only_after_a_complete_success() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("input.json"),
        br#"{"items":[{"id":123456789012345678901234567890},-0]}"#,
    )
    .unwrap();
    let out = invoke(
        root.path(),
        &[
            "data",
            "--step",
            "json-select:/items",
            "--step",
            "array-to-jsonl",
            "-i",
            "input.json",
            "-o",
            "input.json",
            "--force",
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
        std::fs::read(root.path().join("input.json")).unwrap(),
        b"{\"id\":123456789012345678901234567890}\n-0\n"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn escaped_pointer_plan_growth_is_rejected_before_data_or_output_preparation() {
    let root = tempfile::tempdir().unwrap();
    let step = format!(
        "[[steps]]\noperation='json-select'\npointer='/{}'\n",
        "\t".repeat(2047)
    );
    let spec = format!("version=1\n{}", step.repeat(16));
    assert!(spec.len() < 65536);
    std::fs::write(root.path().join("large.toml"), spec).unwrap();
    std::fs::write(root.path().join("keep.toml"), b"original\r\n").unwrap();
    for preview in [false, true] {
        let mut args = vec![
            "data",
            "--recipe",
            "large.toml",
            "-o",
            "keep.toml",
            "--force",
        ];
        if preview {
            args.push("--show-plan");
        } else {
            args.extend(["-i", "missing.json"]);
        }
        let out = invoke(root.path(), &args, b"");
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("normalized recipe exceeds 64 KiB"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(root.path().join("keep.toml")).unwrap(),
            b"original\r\n"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    let short = format!(
        "version=1\n{}",
        step.replace(&"\t".repeat(2047), &"\t".repeat(800))
            .repeat(16)
    );
    std::fs::write(root.path().join("large.toml"), short).unwrap();
    let out = invoke(
        root.path(),
        &[
            "data",
            "--show-plan",
            "--recipe",
            "large.toml",
            "-o",
            "small.toml",
        ],
        b"not data",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let normalized = std::fs::read(root.path().join("small.toml")).unwrap();
    assert!(normalized.len() < 65536);
    let again = invoke(
        root.path(),
        &["data", "--show-plan", "--recipe", "small.toml"],
        b"not data",
    );
    assert!(again.status.success());
    assert_eq!(again.stdout, normalized);
}

#[test]
fn ordered_steps_match_composed_operations_and_pipe_into_another_command() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(
        root.join("invalid-model-config.toml"),
        "broken model config",
    )
    .unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--step",
            "trim",
            "--step",
            "dedupe",
            "--step",
            "sort:reverse",
        ],
        " 乙 \r\n甲\n 乙\n\n".as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, "甲\n乙\n\n".as_bytes());
    assert!(out.stderr.is_empty());
    let different = invoke(
        root,
        &["data", "--step", "dedupe", "--step", "trim"],
        b"a\na \n",
    );
    assert!(different.status.success());
    assert_eq!(different.stdout, b"a\na\n");
    let mut first = command(root)
        .args(["data", "--step", "trim", "--step", "dedupe"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(root)
        .arg("--count")
        .stdin(Stdio::from(first.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    first
        .stdin
        .take()
        .unwrap()
        .write_all(b" b\na\nb \n")
        .unwrap();
    assert!(first.wait().unwrap().success());
    let counted = second.wait_with_output().unwrap();
    assert!(counted.status.success());
    let value: serde_json::Value = serde_json::from_slice(&counted.stdout).unwrap();
    assert_eq!(value["lines"], 2);
    assert!(counted.stderr.is_empty());
    assert!(!root.join("config").exists());
}

#[test]
fn explicit_recipe_keeps_json_precision_and_source_order_and_can_replace_input() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("recipe.toml"), "\u{feff}version = 1\n[[steps]]\noperation = 'json-format'\n[[steps]]\noperation = 'json-minify'\n").unwrap();
    let original = r#" { "z":1.2300E+400, "z": -0, "n":123456789012345678901234567890 } "#;
    std::fs::write(root.join("input.json"), original).unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--recipe",
            "recipe.toml",
            "-i",
            "input.json",
            "-o",
            "input.json",
            "--force",
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
        std::fs::read_to_string(root.join("input.json")).unwrap(),
        "{\"z\":1.2300E+400,\"z\":-0,\"n\":123456789012345678901234567890}\n"
    );
    let keys = invoke(
        root,
        &[
            "data",
            "--step",
            "json-format",
            "--step",
            "json-keys",
            "-i",
            "input.json",
            "-o",
            "-",
        ],
        b"",
    );
    assert!(keys.status.success());
    assert_eq!(keys.stdout, b"[\"z\",\"z\",\"n\"]\n");
    std::fs::write(
        root.join("reverse.toml"),
        "version=1\n[[steps]]\noperation='sort'\nreverse=true\n",
    )
    .unwrap();
    assert_eq!(
        invoke(root, &["data", "--recipe", "reverse.toml"], b"a\nb\na").stdout,
        b"b\na\na\n"
    );
}

#[test]
fn downstream_failure_never_emits_intermediate_or_replaces_existing_output() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("result.txt"), b"old result\r\n").unwrap();
    for args in [
        vec!["data", "--step", "trim", "--step", "json-minify"],
        vec![
            "data",
            "--step",
            "trim",
            "--step",
            "json-minify",
            "-o",
            "result.txt",
            "--force",
        ],
    ] {
        let out = invoke(root, &args, b"  partial secret content\n");
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("step 2 failed"), "{error}");
        assert!(!error.contains("secret content"));
        assert_eq!(
            std::fs::read(root.join("result.txt")).unwrap(),
            b"old result\r\n"
        );
        assert_eq!(
            std::fs::read_dir(root).unwrap().count(),
            1,
            "temporary output leaked"
        );
    }
    for op in ["trim", "dedupe", "sort"] {
        let out = invoke(
            root,
            &["data", "--step", op, "--step", "trim", "-o", "empty.txt"],
            b"",
        );
        assert!(out.status.success());
        assert_eq!(std::fs::read(root.join("empty.txt")).unwrap(), b"");
        std::fs::remove_file(root.join("empty.txt")).unwrap();
    }
}

#[test]
fn complete_recipe_and_option_validation_happens_before_reading_data() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let recipes = [
        "version=2\n[[steps]]\noperation='trim'\n",
        "version=1\nsteps=[]\n",
        "version=1\n[[steps]]\noperation='shell'\n",
        "version=1\n[[steps]]\noperation='trim'\nreverse=true\n",
        "version=1\n[[steps]]\noperation='trim'\npath='secret'\n",
        "version=1\ninclude='another.toml'\n[[steps]]\noperation='trim'\n",
        "version=1\n[[steps]]\noperation='trim'\nreverse='yes'\n",
    ];
    for spec in recipes {
        std::fs::write(root.join("recipe.toml"), spec).unwrap();
        let out = invoke(
            root,
            &[
                "data",
                "--recipe",
                "recipe.toml",
                "-i",
                "missing-input",
                "-o",
                "new.txt",
            ],
            b"",
        );
        assert!(!out.status.success(), "{spec}");
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!error.contains("cannot open missing-input"), "{error}");
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
    for args in [
        vec!["data"],
        vec!["data", "trim", "--step", "sort"],
        vec!["data", "--step", "trim", "--recipe", "recipe.toml"],
        vec!["data", "--step", "sort", "--reverse"],
        vec!["data", "--recipe", "recipe.toml", "--reverse"],
        vec!["data", "--step", "trim:reverse"],
        vec!["data", "--step", "$(touch marker)"],
        vec!["data", "--recipe", "missing-recipe"],
        vec!["--agent", "data", "--step", "sort"],
        vec!["--json", "data", "--step", "sort"],
    ] {
        let out = invoke(root, &args, b"data");
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty());
    }
    assert!(!root.join("marker").exists());
}

#[test]
fn step_recipe_input_and_intermediate_limits_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let mut args = vec!["data"];
    for _ in 0..17 {
        args.extend(["--step", "trim"]);
    }
    assert!(!invoke(root, &args, b"x").status.success());
    std::fs::write(
        root.join("recipe.toml"),
        format!("version=1\n{}", "[[steps]]\noperation='trim'\n".repeat(17)),
    )
    .unwrap();
    assert!(
        !invoke(root, &["data", "--recipe", "recipe.toml"], b"x")
            .status
            .success()
    );
    std::fs::write(root.join("recipe.toml"), format!("#{}", "x".repeat(65536))).unwrap();
    assert!(
        !invoke(root, &["data", "--recipe", "recipe.toml"], b"x")
            .status
            .success()
    );
    let out = invoke(
        root,
        &["data", "--step", "trim", "--max-input", "2"],
        b"abc",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    let out = invoke(root, &["data", "--step", "trim"], b"\xff");
    assert!(!out.status.success() && out.stdout.is_empty());
    let growing = format!(
        "{}{}0{}",
        "[".repeat(127),
        "0,".repeat(70000),
        "]".repeat(127)
    );
    let out = invoke(
        root,
        &["data", "--step", "json-format", "--step", "json-minify"],
        growing.as_bytes(),
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("step 1 failed"));
}

#[test]
fn preview_exports_reusable_ordered_recipes_for_all_native_operations() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for (operation, input) in [
        ("trim", " b \r\na \n"),
        ("dedupe", "b\nb\na\n"),
        ("sort", "b\na\n"),
        ("json-format", r#"{"n":1.2300E+400,"n":-0}"#),
        ("json-minify", r#" { "n":123456789012345678901234567890 } "#),
        ("json-keys", r#"{"z":0,"a":1,"z":2}"#),
        ("jsonl-to-array", "1.2300E+400\n-0\n"),
        ("array-to-jsonl", "[1.2300E+400,-0]"),
    ] {
        let preview = invoke(
            root,
            &["data", operation, "--show-plan"],
            b"\xffPRIVATE_STDIN",
        );
        assert!(
            preview.status.success(),
            "{}",
            String::from_utf8_lossy(&preview.stderr)
        );
        assert!(preview.stderr.is_empty());
        let text = std::str::from_utf8(&preview.stdout).unwrap();
        let parsed: toml::Value = toml::from_str(text).unwrap();
        assert_eq!(parsed["version"].as_integer(), Some(1));
        assert_eq!(parsed["steps"][0]["operation"].as_str(), Some(operation));
        assert!(!text.contains("PRIVATE"));
        std::fs::write(root.join("exported.toml"), &preview.stdout).unwrap();
        let actual = invoke(
            root,
            &["data", "--recipe", "exported.toml"],
            input.as_bytes(),
        );
        let direct = invoke(root, &["data", operation], input.as_bytes());
        assert!(actual.status.success() && direct.status.success());
        assert_eq!(actual.stdout, direct.stdout);
        assert!(actual.stderr.is_empty());
    }
    std::fs::write(root.join("ordered.toml"), "\u{feff}# source comment\nversion=1\n[[steps]]\noperation='trim'\nreverse=false\n[[steps]]\noperation='dedupe'\n[[steps]]\noperation='sort'\nreverse=true\n").unwrap();
    let file = invoke(
        root,
        &["data", "--show-plan", "--recipe", "ordered.toml"],
        b"",
    );
    let steps = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "trim",
            "--step",
            "dedupe",
            "--step",
            "sort:reverse",
        ],
        b"",
    );
    assert!(file.status.success() && steps.status.success());
    assert_eq!(file.stdout, steps.stdout);
    let text = String::from_utf8(file.stdout).unwrap();
    assert!(!text.contains("source comment") && !text.contains("false"));
    assert_eq!(text.matches("[[steps]]").count(), 3);
    let single = invoke(root, &["data", "sort", "--reverse", "--show-plan"], b"");
    assert_eq!(
        single.stdout,
        invoke(
            root,
            &["data", "--show-plan", "--step", "sort:reverse"],
            b""
        )
        .stdout
    );
}

#[test]
fn preview_finishes_while_stdin_is_still_open_and_rejects_explicit_data_input() {
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    for (args, success) in [
        (vec!["data", "json-keys", "--show-plan"], true),
        (
            vec![
                "data",
                "--show-plan",
                "--step",
                "trim",
                "-i",
                "missing-data",
            ],
            false,
        ),
        (
            vec!["-i", "-", "data", "--show-plan", "--step", "trim"],
            false,
        ),
    ] {
        let mut child = command(root.path())
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() > end {
                child.kill().ok();
                panic!("preview waited for stdin EOF: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert_eq!(
            out.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        if success {
            assert!(out.stderr.is_empty());
            assert!(String::from_utf8_lossy(&out.stdout).contains("json-keys"));
        } else {
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains("does not read data"));
        }
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn preview_rejects_invalid_plans_before_preparing_output_and_hides_definition_values() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("keep.toml"), b"original plan\r\n").unwrap();
    for spec in [
        "version=2\n[[steps]]\noperation='trim'\n".to_owned(),
        "version=1\nsteps=[]\n".into(),
        "version=1\n[[steps]]\noperation='trim'\n[[steps]]\noperation='dedupe'\nreverse=true\n"
            .into(),
        "version=1\n[[steps]]\noperation='PRIVATE_OPERATION'\n".into(),
        "version=1\n[[steps]]\noperation='trim'\npath='PRIVATE_PATH'\n".into(),
        "version=1\ninclude='PRIVATE_INCLUDE'\n[[steps]]\noperation='trim'\n".into(),
        format!("version=1\n{}", "[[steps]]\noperation='trim'\n".repeat(17)),
        format!("#{}", "x".repeat(65536)),
    ] {
        std::fs::write(root.join("bad.toml"), &spec).unwrap();
        let out = invoke(
            root,
            &[
                "data",
                "--show-plan",
                "--recipe",
                "bad.toml",
                "-o",
                "keep.toml",
                "--force",
            ],
            b"PRIVATE_DATA",
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!error.contains("PRIVATE"), "{error}");
        if spec.contains("reverse=true") {
            assert!(error.contains("step 2 is invalid"), "{error}");
        }
        assert_eq!(
            std::fs::read(root.join("keep.toml")).unwrap(),
            b"original plan\r\n"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    }
    for args in [
        vec!["data", "--show-plan"],
        vec![
            "data",
            "--show-plan",
            "--step",
            "trim",
            "--recipe",
            "bad.toml",
        ],
        vec!["data", "--show-plan", "--step", "trim:reverse"],
        vec!["data", "--show-plan", "--step", "$(touch marker)"],
    ] {
        let out = invoke(root, &args, b"");
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
    }
    assert!(!root.join("marker").exists());
}

#[test]
fn preview_file_export_preserves_no_clobber_and_reuses_native_publication() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(
        root.join("invalid-model-config.toml"),
        "not valid model config",
    )
    .unwrap();
    let args = [
        "data",
        "--show-plan",
        "--step",
        "trim",
        "--step",
        "dedupe",
        "-o",
        "cleanup.toml",
    ];
    let out = invoke(root, &args, b"\xffPRIVATE_DATA");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let original = std::fs::read(root.join("cleanup.toml")).unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "sort:reverse",
            "-o",
            "cleanup.toml",
        ],
        b"",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert_eq!(std::fs::read(root.join("cleanup.toml")).unwrap(), original);
    let out = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "sort:reverse",
            "-o",
            "cleanup.toml",
            "--force",
        ],
        b"",
    );
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    let plan = std::fs::read(root.join("cleanup.toml")).unwrap();
    assert_ne!(plan, original);
    let out = invoke(
        root,
        &["data", "--show-plan", "--recipe", "cleanup.toml", "-o", "-"],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, plan);
    assert_eq!(
        invoke(root, &["data", "--recipe", "cleanup.toml"], b"a\nb\n").stdout,
        b"b\na\n"
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    assert!(!root.join("config").exists());
}

#[test]
fn preview_validates_definition_without_claiming_data_execution_will_succeed() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let out = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "trim",
            "--step",
            "json-minify",
            "-o",
            "plan.toml",
        ],
        b"",
    );
    assert!(out.status.success() && out.stdout.is_empty());
    std::fs::write(root.join("keep.txt"), b"original result\r\n").unwrap();
    let out = invoke(
        root,
        &["data", "--recipe", "plan.toml", "-o", "keep.txt", "--force"],
        b" PRIVATE_NOT_JSON \n",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("step 2 failed") && !error.contains("PRIVATE_NOT_JSON"));
    assert_eq!(
        std::fs::read(root.join("keep.txt")).unwrap(),
        b"original result\r\n"
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
}
