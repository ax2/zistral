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

#[test]
fn line_window_plans_roundtrip_counts_and_preserve_pipeline_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let plan = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--step",
            "lines-include:ERROR",
            "--step",
            "lines-tail:2",
            "-o",
            "recent.toml",
        ],
        b"ignored",
    );
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(plan.stdout.is_empty());
    let text = std::fs::read_to_string(root.join("recent.toml")).unwrap();
    assert!(text.contains("lines = 2") && text.contains("lines-tail"));
    let out = invoke(
        root,
        &["data", "--recipe", "recent.toml"],
        b"ERROR old\nINFO mid\nERROR a\nERROR b\n",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"ERROR a\nERROR b\n");
    let preview = invoke(
        root,
        &["data", "--show-plan", "--recipe", "recent.toml"],
        b"ignored",
    );
    assert!(preview.status.success());
    assert_eq!(preview.stdout, text.as_bytes());
    let single = invoke(
        root,
        &["data", "lines-head", "--lines", "2", "--show-plan"],
        b"ignored",
    );
    assert!(single.status.success());
    assert!(String::from_utf8_lossy(&single.stdout).contains("lines = 2"));
}

#[test]
fn line_window_limits_and_later_failures_preserve_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.txt"), b"keep").unwrap();
    for text in [
        "version=1\n[[steps]]\noperation='lines-head'\n",
        "version=1\n[[steps]]\noperation='lines-tail'\nlines=0\n",
        "version=1\n[[steps]]\noperation='trim'\nlines=2\n",
        "version=1\n[[steps]]\noperation='lines-tail'\nlines='2'\n",
    ] {
        std::fs::write(root.join("bad.toml"), text).unwrap();
        let out = invoke(
            root,
            &[
                "data",
                "--recipe",
                "bad.toml",
                "-i",
                "missing-input",
                "-o",
                "keep.txt",
                "--force",
            ],
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("cannot open missing-input"));
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"keep");
    }
    for input in [b"PRIVATE_INVALID_JSON\n".to_vec(), b"x\n".repeat(100001)] {
        let out = invoke(
            root,
            &[
                "data",
                "--step",
                "lines-head:1",
                "--step",
                "json-minify",
                "-o",
                "keep.txt",
                "--force",
            ],
            &input,
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"keep");
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_INVALID_JSON"));
    }
}

#[test]
fn embedded_catalog_and_preview_finish_without_stdin_eof_or_model_configuration() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("invalid-model-config.toml"), "not valid TOML").unwrap();
    for (args, success) in [
        (vec!["data", "--list-recipes"], true),
        (
            vec!["data", "--show-plan", "--recipe", "builtin:severity-counts"],
            true,
        ),
        (vec!["data", "--list-recipes", "-i", "-"], false),
        (vec!["data", "--list-recipes", "-i", "missing-data"], false),
        (
            vec!["data", "--show-plan", "--recipe", "builtin:typo"],
            false,
        ),
    ] {
        let mut child = command(root)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > end {
                child.kill().ok();
                let _ = child.wait();
                panic!("offline recipe discovery waited for stdin EOF: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert_eq!(
            out.status.success(),
            success,
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if success {
            assert!(out.stderr.is_empty() && !out.stdout.is_empty());
        } else {
            assert!(out.stdout.is_empty());
        }
        if args.contains(&"--list-recipes") && success {
            let text = String::from_utf8(out.stdout).unwrap();
            let names: Vec<_> = text
                .lines()
                .filter(|line| line.starts_with("builtin:"))
                .map(|line| line.split(' ').next().unwrap())
                .collect();
            assert_eq!(
                names,
                [
                    "builtin:cleanup",
                    "builtin:severity-counts",
                    "builtin:unique-severity-counts",
                    "builtin:top-severity-counts",
                    "builtin:recent-unique-events"
                ]
            );
        }
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn embedded_recipes_run_from_an_empty_directory_and_export_replay_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let logs = concat!(
        "{\"id\":\"a\",\"level\":\"ERROR\",\"time\":\"02\",\"n\":1.2300E+400}\n",
        "{\"id\":\"b\",\"level\":\"INFO\",\"time\":\"03\"}\n",
        "{\"id\":\"a\",\"level\":\"INFO\",\"time\":\"04\"}\n",
        "{\"id\":\"c\",\"level\":\"ERROR\",\"time\":\"01\"}\n",
    );
    let lines: Vec<_> = logs.lines().collect();
    let recent = format!("{}\n{}\n{}\n", lines[1], lines[0], lines[3]);
    for (name, input, expected) in [
        ("cleanup", " b \r\na\nb\n a ", "b\na\n"),
        (
            "severity-counts",
            logs,
            "\"value\",\"count\"\n\"ERROR\",\"2\"\n\"INFO\",\"2\"\n",
        ),
        (
            "unique-severity-counts",
            logs,
            "\"value\",\"count\"\n\"ERROR\",\"2\"\n\"INFO\",\"1\"\n",
        ),
        ("recent-unique-events", logs, recent.as_str()),
    ] {
        let reference = format!("builtin:{name}");
        let out = invoke(root, &["data", "--recipe", &reference], input.as_bytes());
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        assert_eq!(out.stdout, expected.as_bytes(), "{name}");
        let preview = invoke(
            root,
            &[
                "data",
                "--recipe",
                &reference,
                "--show-plan",
                "-o",
                "export.toml",
            ],
            b"PRIVATE_IGNORED_DATA",
        );
        assert!(preview.status.success() && preview.stdout.is_empty() && preview.stderr.is_empty());
        let definition = std::fs::read_to_string(root.join("export.toml")).unwrap();
        assert!(definition.starts_with("version = 1\n") && !definition.contains("PRIVATE"));
        let replay = invoke(root, &["data", "--recipe", "export.toml"], input.as_bytes());
        assert!(replay.status.success() && replay.stderr.is_empty());
        assert_eq!(replay.stdout, out.stdout);
        std::fs::remove_file(root.join("export.toml")).unwrap();
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn exported_builtin_is_editable_without_modifying_the_builtin() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let preview = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--recipe",
            "builtin:recent-unique-events",
        ],
        b"",
    );
    assert!(preview.status.success() && preview.stderr.is_empty());
    let definition = String::from_utf8(preview.stdout)
        .unwrap()
        .replace("/id", "/event_id")
        .replace("/time", "/timestamp")
        .replace("lines = 20", "lines = 1");
    std::fs::write(root.join("my recent.toml"), definition).unwrap();
    let out = invoke(
        root,
        &["data", "--recipe", "my recent.toml"],
        b"{\"event_id\":\"a\",\"timestamp\":\"01\"}\n{\"event_id\":\"b\",\"timestamp\":\"02\"}\n",
    );
    assert!(out.status.success() && out.stderr.is_empty());
    assert_eq!(out.stdout, b"{\"event_id\":\"b\",\"timestamp\":\"02\"}\n");
    let original = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--recipe",
            "builtin:recent-unique-events",
        ],
        b"",
    );
    let text = String::from_utf8(original.stdout).unwrap();
    assert!(
        original.status.success()
            && text.contains("/id")
            && text.contains("/time")
            && text.contains("lines = 20")
    );
}

#[test]
fn recipe_catalog_rejects_processing_modes_before_io_and_preserves_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.txt"), b"original\r\n").unwrap();
    for options in [
        vec!["trim"],
        vec!["--step", "trim"],
        vec!["--recipe", "missing.toml"],
        vec!["--show-plan"],
        vec!["--reverse"],
        vec!["--pointer", "/level"],
        vec!["--field", "id=/id"],
        vec!["--contains", "ERROR"],
        vec!["--equals", "ERROR"],
        vec!["--lines", "2"],
        vec!["-i", "missing-data"],
        vec!["--json"],
        vec!["--stream"],
        vec!["--count"],
        vec!["--select", "/"],
        vec!["--preset", "summarize"],
        vec!["--agent"],
        vec!["--cwd", "missing-project"],
    ] {
        let mut args = vec!["data", "--list-recipes", "-o", "keep.txt", "--force"];
        args.extend(options);
        let out = invoke(root, &args, b"PRIVATE_INPUT");
        assert!(!out.status.success() && out.stdout.is_empty(), "{args:?}");
        assert_eq!(
            std::fs::read(root.join("keep.txt")).unwrap(),
            b"original\r\n"
        );
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            !error.contains("PRIVATE_INPUT") && !error.contains("cannot open missing"),
            "{error}"
        );
    }
    for name in [
        "builtin:",
        "builtin:Cleanup",
        "builtin:typo",
        "builtin:../cleanup",
        "builtin:cleanup.toml",
    ] {
        let out = invoke(
            root,
            &[
                "data",
                "--recipe",
                name,
                "-i",
                "missing-data",
                "-o",
                "keep.txt",
                "--force",
            ],
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("unknown builtin recipe") && error.contains("--list-recipes"),
            "{error}"
        );
        assert!(!error.contains("cannot open"));
        assert_eq!(
            std::fs::read(root.join("keep.txt")).unwrap(),
            b"original\r\n"
        );
    }
}

#[test]
fn builtin_failure_and_explicit_catalog_export_keep_normal_publication_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.txt"), b"original").unwrap();
    let out = invoke(
        root,
        &[
            "data",
            "--recipe",
            "builtin:severity-counts",
            "-o",
            "keep.txt",
            "--force",
        ],
        b"{\"level\":\"PRIVATE_VALUE\"}\n{\"level\":42}\n",
    );
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE_VALUE"));
    assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"original");
    for mut args in [
        vec!["data", "--list-recipes"],
        vec!["data", "--show-plan", "--recipe", "builtin:cleanup"],
    ] {
        let direct = invoke(root, &args, b"ignored");
        assert!(direct.status.success());
        args.extend(["-o", "keep.txt"]);
        let out = invoke(root, &args, b"ignored");
        assert!(!out.status.success() && out.stdout.is_empty());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"original");
        args.push("--force");
        let out = invoke(root, &args, b"ignored");
        assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), direct.stdout);
        std::fs::write(root.join("keep.txt"), b"original").unwrap();
    }
    let stdout = invoke(root, &["data", "--list-recipes", "-o", "-"], b"ignored");
    assert!(stdout.status.success() && stdout.stderr.is_empty());
    assert!(String::from_utf8_lossy(&stdout.stdout).contains("builtin:cleanup"));
}

#[cfg(unix)]
#[test]
fn builtin_namespace_has_explicit_filename_escape() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let definition = "version=1\n[[steps]]\noperation='json-minify'\n";
    std::fs::write(root.join("builtin:cleanup"), definition).unwrap();
    std::fs::write(root.join("builtin:unknown"), definition).unwrap();
    let named = invoke(
        root,
        &["data", "--recipe", "./builtin:cleanup"],
        b"{ \"n\" : 1 }",
    );
    assert!(named.status.success() && named.stderr.is_empty());
    assert_eq!(named.stdout, b"{\"n\":1}\n");
    let builtin = invoke(root, &["data", "--recipe", "builtin:cleanup"], b"b\na\n");
    assert!(builtin.status.success());
    assert_eq!(builtin.stdout, b"b\na\n");
    let unknown = invoke(
        root,
        &["data", "--recipe", "builtin:unknown", "--show-plan"],
        b"",
    );
    assert!(!unknown.status.success() && unknown.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown builtin recipe"));
    std::fs::write(root.join("配方 recent.toml"), definition).unwrap();
    let unicode = invoke(
        root,
        &["data", "--show-plan", "--recipe", "配方 recent.toml"],
        b"",
    );
    assert!(unicode.status.success() && unicode.stderr.is_empty());
    assert!(String::from_utf8_lossy(&unicode.stdout).contains("json-minify"));
}

// The macOS runner's filesystem rejects creating invalid UTF-8 filenames.
// Exercise that filename contract where the test filesystem supports it.
#[cfg(target_os = "linux")]
#[test]
fn explicit_recipe_preserves_non_utf8_linux_paths() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let definition = "version=1\n[[steps]]\noperation='json-minify'\n";
    let name = std::ffi::OsString::from_vec(b"recipe-\xff.toml".to_vec());
    std::fs::write(root.join(&name), definition).unwrap();
    let out = command(root)
        .args(["data", "--show-plan", "--recipe"])
        .arg(&name)
        .output()
        .unwrap();
    assert!(out.status.success() && out.stderr.is_empty());
    assert!(String::from_utf8_lossy(&out.stdout).contains("json-minify"));
}

#[test]
fn recipe_parse_locations_never_echo_definition_values_or_publish_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.toml"), b"original").unwrap();
    for text in [
        "version=1\n[[steps]]\noperation='PRIVATE_UNKNOWN_OPERATION'\n",
        "\u{feff}version=1\r\n[[steps]]\r\noperation='trim'\r\nPRIVATE_FIELD='PRIVATE_VALUE'\r\n",
        "version=1\n[[steps]]\noperation=\n",
    ] {
        std::fs::write(root.join("bad.toml"), text).unwrap();
        for preview in [false, true] {
            let mut args = vec!["data", "--recipe", "bad.toml", "-o", "keep.toml", "--force"];
            if preview {
                args.push("--show-plan");
            } else {
                args.extend(["-i", "missing-data"]);
            }
            let out = invoke(root, &args, b"PRIVATE_INPUT");
            assert!(!out.status.success() && out.stdout.is_empty());
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(
                error.contains("invalid recipe TOML at line") && error.contains("column"),
                "{error}"
            );
            assert!(!error.contains("PRIVATE") && !error.contains("cannot open"));
            assert_eq!(std::fs::read(root.join("keep.toml")).unwrap(), b"original");
        }
    }
}

#[test]
fn builtin_top_counts_rank_integers_stably_and_export_an_editable_offline_plan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let logs = "{\"level\":\"WARN\"}\n".repeat(2)
        + &"{\"level\":\"ERROR\"}\n".repeat(10)
        + &"{\"level\":\"INFO\"}\n".repeat(2);
    let out = invoke(
        root,
        &["data", "--recipe", "builtin:top-severity-counts"],
        logs.as_bytes(),
    );
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        b"\"value\",\"count\"\n\"ERROR\",\"10\"\n\"WARN\",\"2\"\n\"INFO\",\"2\"\n"
    );
    let preview = invoke(
        root,
        &[
            "data",
            "--show-plan",
            "--recipe",
            "builtin:top-severity-counts",
            "-o",
            "top.toml",
        ],
        b"NOT_JSON_IGNORED",
    );
    assert!(preview.status.success() && preview.stdout.is_empty() && preview.stderr.is_empty());
    let original = std::fs::read_to_string(root.join("top.toml")).unwrap();
    let plan: toml::Value = toml::from_str(&original).unwrap();
    assert_eq!(plan["steps"].as_array().unwrap().len(), 4);
    assert_eq!(
        plan["steps"][1]["operation"].as_str(),
        Some("jsonl-sort-int")
    );
    assert_eq!(plan["steps"][1]["reverse"].as_bool(), Some(true));
    let edited = original.replace("lines = 20", "lines = 1");
    std::fs::write(root.join("top.toml"), &edited).unwrap();
    let out = invoke(root, &["data", "--recipe", "top.toml"], logs.as_bytes());
    assert!(out.status.success() && out.stderr.is_empty());
    assert_eq!(out.stdout, b"\"value\",\"count\"\n\"ERROR\",\"10\"\n");
    assert_eq!(
        std::fs::read_to_string(root.join("top.toml")).unwrap(),
        edited
    );
    let source = include_str!("../docs/examples/top-severity-counts.toml");
    std::fs::write(root.join("source.toml"), source).unwrap();
    let out = invoke(
        root,
        &["data", "--show-plan", "--recipe", "source.toml"],
        b"",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, original.as_bytes());
}

#[test]
fn redaction_changes_only_the_explicit_value_and_keeps_lexical_payloads() {
    let root = tempfile::tempdir().unwrap();
    let source = r#" {"unchanged":"same","secret":"same","a/b":{"~key":["same",{"n":1.2300E+400,"n":-0}]},"big":9007199254740993,"s":"\u4e2d"} "#;
    let out = invoke(
        root.path(),
        &["data", "json-redact", "--pointer", "/secret"],
        source.as_bytes(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(out.stdout, br#"{"unchanged":"same","secret":"[REDACTED]","a/b":{"~key":["same",{"n":1.2300E+400,"n":-0}]},"big":9007199254740993,"s":"\u4e2d"}
"#);
    let out = invoke(
        root.path(),
        &["data", "json-redact", "--pointer", "/a~1b/~0key/1"],
        source.as_bytes(),
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, br#"{"unchanged":"same","secret":"same","a/b":{"~key":["same","[REDACTED]"]},"big":9007199254740993,"s":"\u4e2d"}
"#);
    let out = invoke(
        root.path(),
        &["data", "json-redact", "--pointer", ""],
        source.as_bytes(),
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"\"[REDACTED]\"\n");
}

#[test]
fn redaction_accepts_every_json_value_type_and_normalizes_jsonl_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let source = b" {\"x\":null} \r\n{\"x\":true}\r\n{\"x\":-0}\n{\"x\":1e9999}\n{\"x\":\"s\"}\n{\"x\":[1,2]}\n{\"x\":{\"a\":1}}";
    let out = invoke(
        root.path(),
        &["data", "jsonl-redact", "--pointer", "/x"],
        source,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"{\"x\":\"[REDACTED]\"}\n".repeat(7));
    let empty = invoke(
        root.path(),
        &["data", "jsonl-redact", "--pointer", "/x"],
        b"",
    );
    assert!(empty.status.success() && empty.stdout.is_empty() && empty.stderr.is_empty());
}

#[test]
fn redaction_rejects_ambiguous_invalid_or_missing_values_without_publishing_data() {
    let root = tempfile::tempdir().unwrap();
    let secret = "PRIVATE_REDACTION_TEST_SENTINEL";
    for record in [
        format!(r#"{{"x":"{secret}","x":0}}"#),
        format!(r#"{{"x":"{secret}","\u0078":0}}"#),
        format!(r#"{{"missing":"{secret}"}}"#),
        format!(r#"{{"x":"{secret}","bad":"\ud800"}}"#),
        format!(r#"{{"x":"{secret}","bad":}}"#),
    ] {
        std::fs::write(root.path().join("keep.txt"), "old result").unwrap();
        let input = format!("{{\"x\":0}}\n{record}\n");
        let out = invoke(
            root.path(),
            &[
                "data",
                "jsonl-redact",
                "--pointer",
                "/x",
                "-o",
                "keep.txt",
                "--force",
            ],
            input.as_bytes(),
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("record 2") && error.contains("jsonl-redact"),
            "{error}"
        );
        assert!(!error.contains(secret));
        assert_eq!(
            std::fs::read_to_string(root.path().join("keep.txt")).unwrap(),
            "old result"
        );
    }
    for args in [
        vec!["data", "json-redact"],
        vec!["data", "jsonl-redact", "--pointer", "bad"],
        vec!["data", "jsonl-redact", "--pointer", "/x~2"],
        vec!["data", "json-redact", "--pointer", "/x", "--reverse"],
        vec!["data", "json-redact", "--pointer", "/x", "--equals", "x"],
        vec!["data", "jsonl-redact", "--pointer", "/x", "--lines", "2"],
    ] {
        let out = invoke(
            root.path(),
            &[args, vec!["-i", "missing-input.json"]].concat(),
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("cannot open"));
    }
}

#[test]
fn redaction_plan_roundtrips_and_chains_explicit_fields_before_export() {
    let root = tempfile::tempdir().unwrap();
    let args = [
        "data",
        "--show-plan",
        "--step",
        "json-select:/events",
        "--step",
        "array-to-jsonl",
        "--step",
        "jsonl-redact:/credentials/token",
        "--step",
        "jsonl-redact:/user/email",
        "--step",
        "jsonl-to-array",
        "-o",
        "mask.toml",
    ];
    let out = invoke(root.path(), &args, b"not read");
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    let plan = std::fs::read_to_string(root.path().join("mask.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&plan).unwrap();
    assert_eq!(parsed["steps"].as_array().unwrap().len(), 5);
    assert!(!plan.contains("not read"));
    let input = br#"{"events":[{"credentials":{"token":"private"},"user":{"email":"private"},"n":1.2300E+400}]}"#;
    let out = invoke(root.path(), &["data", "--recipe", "mask.toml"], input);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        br#"[{"credentials":{"token":"[REDACTED]"},"user":{"email":"[REDACTED]"},"n":1.2300E+400}]
"#
    );
    let mut child = command(root.path())
        .args(["data", "--show-plan", "--step", "json-redact:/x"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() && start.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("plan export blocked on open stdin");
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
}

#[test]
fn redaction_uses_exact_unicode_keys_and_strict_array_indexes() {
    let root = tempfile::tempdir().unwrap();
    let input = "{\"中\":{\"\":[1,2]},\"e\\u0301\":\"remain\",\"é\":\"private\"}";
    let out = invoke(
        root.path(),
        &[
            "data",
            "--step",
            "json-redact:/中//1",
            "--step",
            "json-redact:/é",
        ],
        input.as_bytes(),
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "{\"中\":{\"\":[1,\"[REDACTED]\"]},\"e\\u0301\":\"remain\",\"é\":\"[REDACTED]\"}\n"
    );
    for pointer in [
        "/中//01",
        "/中//-",
        "/中//2",
        "/中//99999999999999999999999999",
    ] {
        let out = invoke(
            root.path(),
            &["data", "json-redact", "--pointer", pointer],
            input.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
    }
}

#[test]
fn json_profiles_are_offline_value_free_and_distinguish_missing_from_null() {
    let root = tempfile::tempdir().unwrap();
    let input =
        b"{\"token\":\"PRIVATE_PROFILE_VALUE\",\"user\":{\"email\":null}}\r\n{\"user\":null}\n";
    let out = invoke(root.path(), &["data", "jsonl-profile"], input);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty()
            && !String::from_utf8_lossy(&out.stdout).contains("PRIVATE_PROFILE_VALUE")
    );
    let p: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(p["records"], 2);
    let token = p["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["pointer"] == "/token")
        .unwrap();
    assert_eq!(token["present"], 1);
    assert_eq!(token["missing"], 1);
    assert_eq!(token["types"], serde_json::json!({"string":1}));
    let email = p["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["pointer"] == "/user/email")
        .unwrap();
    assert_eq!(email["types"], serde_json::json!({"null":1}));
    assert_eq!(email["missing"], 1);
    let single = invoke(
        root.path(),
        &["data", "json-profile"],
        b"{\n\"x\":1.2300E+400,\n\"empty\":\"\"\n}\n",
    );
    assert!(single.status.success());
    let p: serde_json::Value = serde_json::from_slice(&single.stdout).unwrap();
    assert_eq!(p["records"], 1);
    assert_eq!(p["paths"][2]["types"], serde_json::json!({"number":1}));
    let empty = invoke(root.path(), &["data", "jsonl-profile"], b"");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap(),
        serde_json::json!({"records":0,"paths":[]})
    );
}

#[test]
fn structural_profile_plan_exports_replays_and_chains_explicit_array_selection() {
    let root = tempfile::tempdir().unwrap();
    let out = invoke(
        root.path(),
        &[
            "data",
            "--show-plan",
            "--step",
            "json-select:/events",
            "--step",
            "array-to-jsonl",
            "--step",
            "jsonl-profile",
            "-o",
            "profile.toml",
        ],
        b"not data",
    );
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    let source = br#"{"events":[{"id":1,"token":"PRIVATE_PROFILE_SAMPLE"},{"id":"s"}]}"#;
    let out = invoke(root.path(), &["data", "--recipe", "profile.toml"], source);
    assert!(out.status.success());
    let p: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(p["records"], 2);
    assert_eq!(
        p["paths"][1]["types"],
        serde_json::json!({"number":1,"string":1})
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE_PROFILE_SAMPLE"));
    let selected = invoke(
        root.path(),
        &[
            "data",
            "--step",
            "json-select:/events",
            "--step",
            "array-to-jsonl",
            "--step",
            "jsonl-profile",
            "--step",
            "json-select:/records",
        ],
        source,
    );
    assert!(selected.status.success());
    assert_eq!(selected.stdout, b"2\n");
    let plan = std::fs::read_to_string(root.path().join("profile.toml")).unwrap();
    assert!(
        plan.contains("jsonl-profile")
            && !plan.contains("PRIVATE_PROFILE_SAMPLE")
            && !plan.contains("not data")
    );
}

#[test]
fn invalid_profiles_preserve_files_and_do_not_echo_values_or_ambiguous_keys() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.json"), b"old report").unwrap();
    for tail in [
        "",
        r#"{"private-key-719":1,"private-key-719":"PRIVATE_PROFILE_VALUE"}"#,
        r#"{"x":"\ud800"}"#,
        r#"{"x":} PRIVATE_PROFILE_VALUE"#,
    ] {
        let input = format!("{{}}\n{tail}\n");
        let out = invoke(
            root.path(),
            &["data", "jsonl-profile", "-o", "keep.json", "--force"],
            input.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("record 2"));
        assert!(!error.contains("private-key-719") && !error.contains("PRIVATE_PROFILE_VALUE"));
        assert_eq!(
            std::fs::read(root.path().join("keep.json")).unwrap(),
            b"old report"
        );
    }
    for extra in [
        vec!["--pointer", ""],
        vec!["--reverse"],
        vec!["--lines", "2"],
        vec!["--contains", "x"],
        vec!["--equals", "x"],
        vec!["--field", "x=/x"],
    ] {
        let out = invoke(
            root.path(),
            &[
                vec!["data", "json-profile"],
                extra,
                vec!["-i", "missing-input"],
            ]
            .concat(),
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("cannot open"));
    }
}

#[test]
fn profile_before_masking_exposes_missing_paths_without_automatically_modifying_input() {
    let root = tempfile::tempdir().unwrap();
    let data = b"{\"token\":\"PRIVATE_PROFILE_VALUE\",\"n\":1e9999}\n{\"token\":null,\"n\":-0}\n";
    std::fs::write(root.path().join("source.jsonl"), data).unwrap();
    let report = invoke(
        root.path(),
        &["data", "jsonl-profile", "-i", "source.jsonl"],
        b"",
    );
    assert!(report.status.success());
    let p: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(p["paths"][2]["pointer"], "/token");
    assert_eq!(p["paths"][2]["missing"], 0);
    let masked = invoke(
        root.path(),
        &[
            "data",
            "jsonl-redact",
            "--pointer",
            "/token",
            "-i",
            "source.jsonl",
        ],
        b"",
    );
    assert!(masked.status.success());
    assert_eq!(
        masked.stdout,
        b"{\"token\":\"[REDACTED]\",\"n\":1e9999}\n{\"token\":\"[REDACTED]\",\"n\":-0}\n"
    );
    assert_eq!(
        std::fs::read(root.path().join("source.jsonl")).unwrap(),
        data
    );
}

#[test]
fn conditional_redaction_masks_present_null_and_preserves_valid_missing_records() {
    let root = tempfile::tempdir().unwrap();
    let data=b" {\"user\":{\"token\":\"PRIVATE_OPTIONAL\"},\"n\":1.2300E+400} \r\n{\"user\":{\"token\":null},\"n\":-0}\n{\"user\":null}\n{\"user\":1e9999}\n{}\n1e9999\n";
    let expected=b"{\"user\":{\"token\":\"[REDACTED]\"},\"n\":1.2300E+400}\n{\"user\":{\"token\":\"[REDACTED]\"},\"n\":-0}\n{\"user\":null}\n{\"user\":1e9999}\n{}\n1e9999\n";
    let out = invoke(
        root.path(),
        &[
            "data",
            "jsonl-redact",
            "--pointer",
            "/user/token",
            "--if-present",
        ],
        data,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
    let strict = invoke(
        root.path(),
        &["data", "jsonl-redact", "--pointer", "/user/token"],
        data,
    );
    assert!(!strict.status.success() && strict.stdout.is_empty());
    for input in [br#"{}"#.as_slice(), br#"{"a":null}"#, b"1e9999"] {
        let out = invoke(
            root.path(),
            &["data", "json-redact", "--pointer", "/a/b", "--if-present"],
            input,
        );
        assert!(out.status.success());
        assert_eq!(out.stdout, [input, b"\n"].concat());
    }
}

#[test]
fn conditional_plan_roundtrips_per_step_and_missing_mode_does_not_claim_a_match() {
    let root = tempfile::tempdir().unwrap();
    let out = invoke(
        root.path(),
        &[
            "data",
            "--show-plan",
            "jsonl-redact",
            "--pointer",
            "/token",
            "--if-present",
            "-o",
            "optional.toml",
        ],
        b"\xffignored",
    );
    assert!(out.status.success() && out.stdout.is_empty());
    let source = std::fs::read_to_string(root.path().join("optional.toml")).unwrap();
    let plan: toml::Value = toml::from_str(&source).unwrap();
    assert_eq!(plan["steps"][0]["if_present"].as_bool(), Some(true));
    let out = invoke(
        root.path(),
        &["data", "--recipe", "optional.toml"],
        b"{\"token\":null}\n{}\n",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"{\"token\":\"[REDACTED]\"}\n{}\n");
    let nomatch = invoke(
        root.path(),
        &["data", "--recipe", "optional.toml"],
        b" {\"other\":1e9999} \n{}",
    );
    assert!(nomatch.status.success());
    assert_eq!(nomatch.stdout, b"{\"other\":1e9999}\n{}\n");
    std::fs::write(
        root.path().join("strict.toml"),
        source.replace("if_present = true", "if_present = false"),
    )
    .unwrap();
    let out = invoke(root.path(), &["data", "--recipe", "strict.toml"], b"{}\n");
    assert!(!out.status.success() && out.stdout.is_empty());
    let mixed = "version=1\n[[steps]]\noperation='jsonl-redact'\npointer='/user'\nif_present=true\n[[steps]]\noperation='jsonl-redact'\npointer='/user/token'\n";
    std::fs::write(root.path().join("mixed.toml"), mixed).unwrap();
    let failed = invoke(
        root.path(),
        &["data", "--recipe", "mixed.toml"],
        b"{\"user\":{\"token\":\"private\"}}\n",
    );
    assert!(!failed.status.success() && failed.stdout.is_empty());
    std::fs::write(
        root.path().join("mixed.toml"),
        format!("{mixed}if_present=true\n"),
    )
    .unwrap();
    let out = invoke(
        root.path(),
        &["data", "--recipe", "mixed.toml"],
        b"{\"user\":{\"token\":\"private\"}}\n",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"{\"user\":\"[REDACTED]\"}\n");
}

#[test]
fn optional_mode_never_suppresses_ambiguous_invalid_or_unsupported_record_errors() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.jsonl"), b"old result").unwrap();
    for tail in [
        r#"{"a":{},"a":null,"secret":"PRIVATE_OPTIONAL"}"#,
        r#"{"a":null,"other":"\ud800"}"#,
        r#"{"a":["\udc00"]}"#,
        r#"{"a":} PRIVATE_OPTIONAL"#,
    ] {
        let data = format!("{{\"a\":{{\"b\":null}}}}\n{tail}\n");
        let out = invoke(
            root.path(),
            &[
                "data",
                "jsonl-redact",
                "--pointer",
                "/a/b",
                "--if-present",
                "-o",
                "keep.jsonl",
                "--force",
            ],
            data.as_bytes(),
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("record 2") && !error.contains("PRIVATE_OPTIONAL"));
        assert_eq!(
            std::fs::read(root.path().join("keep.jsonl")).unwrap(),
            b"old result"
        );
    }
    for index in ["01", "-", "99999999999999999999999999999999999999999"] {
        let pointer = format!("/a/{index}");
        let out = invoke(
            root.path(),
            &[
                "data",
                "jsonl-redact",
                "--pointer",
                &pointer,
                "--if-present",
            ],
            b"{\"a\":[]}\n",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
    }
    let out = invoke(
        root.path(),
        &["data", "jsonl-redact", "--pointer", "/a/20", "--if-present"],
        b"{\"a\":[]}\n",
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"{\"a\":[]}\n");
}

#[test]
fn conditional_redaction_rejects_unrelated_modes_and_bad_boolean_definitions_before_io() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["data", "trim", "--if-present"],
        vec!["data", "jsonl-profile", "--if-present"],
        vec!["data", "json-redact", "--if-present"],
        vec!["data", "--step", "jsonl-redact:/x", "--if-present"],
        vec!["data", "--list-recipes", "--if-present"],
    ] {
        let out = invoke(
            root.path(),
            &[args, vec!["-i", "missing-input"]].concat(),
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("cannot open"));
    }
    for (op, value) in [
        ("trim", "true"),
        ("trim", "false"),
        ("json-redact", "'PRIVATE_OPTIONAL'"),
        ("json-redact", "1"),
    ] {
        let source = format!("version=1\n[[steps]]\noperation='{op}'\nif_present={value}\n");
        std::fs::write(root.path().join("bad.toml"), source).unwrap();
        let out = invoke(
            root.path(),
            &["data", "--recipe", "bad.toml", "-i", "missing-input"],
            b"",
        );
        assert!(!out.status.success() && out.stdout.is_empty());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(!error.contains("cannot open") && !error.contains("PRIVATE_OPTIONAL"));
    }
}

#[test]
fn opaque_profile_counts_do_not_gate_actual_optional_pointer_resolution() {
    let root = tempfile::tempdir().unwrap();
    let data=b"{\"a\":{\"0\":{\"token\":\"s\"}}}\n{\"a\":[{\"token\":\"t\"}]}\n{\"a\":[]}\n{\"a\":{\"0\":null}}\n";
    let out = invoke(root.path(), &["data", "jsonl-profile"], data);
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let path = report["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pointer"] == "/a/0/token")
        .unwrap();
    assert_eq!(path["present"], 1);
    assert_eq!(path["missing"], 3);
    let out = invoke(
        root.path(),
        &[
            "data",
            "jsonl-redact",
            "--pointer",
            "/a/0/token",
            "--if-present",
        ],
        data,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout,b"{\"a\":{\"0\":{\"token\":\"[REDACTED]\"}}}\n{\"a\":[{\"token\":\"[REDACTED]\"}]}\n{\"a\":[]}\n{\"a\":{\"0\":null}}\n");
    let invalid = invoke(
        root.path(),
        &[
            "data",
            "jsonl-redact",
            "--pointer",
            "/a/token",
            "--if-present",
        ],
        b"{\"a\":[]}\n",
    );
    assert!(!invalid.status.success() && invalid.stdout.is_empty());
}
