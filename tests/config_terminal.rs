use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Exercise the real terminal, including Windows ConPTY and hidden password entry.
fn configure(path: &std::path::Path, answers: &[(&str, &str)]) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 180,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_zi"));
    cmd.args(["config", "--config", path.to_str().unwrap()]);
    for name in [
        "ZI_MODEL",
        "ZI_API",
        "ZI_BASE_URL",
        "ZI_API_KEY",
        "ZI_CONFIG",
        "OPENAI_API_KEY",
    ] {
        cmd.env_remove(name);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut position = 0;
    let mut cursor_queries = 0;
    for (prompt, answer) in answers
        .iter()
        .copied()
        .chain(std::iter::once(("Saved ", "")))
    {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(offset) = transcript[position..].find(prompt) {
                position += offset + prompt.len();
                break;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(bytes) => {
                    transcript.push_str(&String::from_utf8_lossy(&bytes));
                    // ConPTY asks the terminal emulator for its cursor position
                    // before starting console applications.
                    let queries = transcript.matches("\x1b[6n").count();
                    for _ in cursor_queries..queries {
                        writer.write_all(b"\x1b[1;1R").unwrap();
                    }
                    writer.flush().unwrap();
                    cursor_queries = queries;
                }
                Err(_) => {
                    child.kill().ok();
                    panic!("terminal did not reach {prompt:?}; output: {transcript}");
                }
            }
        }
        if prompt != "Saved " {
            // Wait for the end of the label before sending: rpassword must have
            // switched the console to hidden input before the key is typed.
            std::thread::sleep(Duration::from_millis(100));
            write!(writer, "{answer}\r").unwrap();
            writer.flush().unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!("config did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    while let Ok(bytes) = rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&bytes));
    }
    transcript
}

#[test]
fn terminal_setup_hides_key_and_can_keep_existing_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let transcript = configure(
        &path,
        &[
            ("Service URL", "http://localhost:11434/v1"),
            ("Model name", "terminal-test-model"),
            ("API protocol", "chat"),
            ("API Key", "terminal-test-secret"),
        ],
    );
    assert!(
        !transcript.contains("terminal-test-secret"),
        "key was echoed"
    );
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["model"].as_str(), Some("terminal-test-model"));
    assert_eq!(saved["api_key"].as_str(), Some("terminal-test-secret"));
    let transcript = configure(
        &path,
        &[
            ("Service URL", ""),
            ("Model name", ""),
            ("API protocol", ""),
            ("API Key", ""),
        ],
    );
    assert!(!transcript.contains("terminal-test-secret"));
    let updated: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved, updated);
}
