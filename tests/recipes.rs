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
