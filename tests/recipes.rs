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
