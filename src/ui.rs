use crate::{Cli, config::Settings};
use anyhow::Result;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::cell::Cell;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

pub fn enabled(cli: &Cli) -> bool {
    !cli.plain
        && !cli.preview
        && !cli.json
        && io::stdout().is_terminal()
        && io::stderr().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
}

// Provider text is untrusted. Remove terminal commands, including OSC hyperlinks,
// clipboard/title commands, CSI cursor controls, and C1 control characters.
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | '^' | '_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                            if c == '\x1b' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => (),
            }
        } else if !c.is_control() || matches!(c, '\n' | '\t') {
            out.push(c);
        }
    }
    out
}

// Only for already validated, compact JSON sent to a terminal. Escaping keeps
// literal DEL/C1 string values and keys intact without parsing/reserializing
// numbers, duplicate keys, or existing escape tokens. Raw pipe/file text bypasses it.
pub fn json_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.push_str(&format!("\\u{:04x}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

pub fn heading(title: &str) {
    eprintln!("\n  {} {}", style("◆").cyan(), style(title).bold());
}

pub fn welcome(cli: &Cli) {
    if enabled(cli) {
        eprintln!(
            "\n  {}",
            style("╭────────────────────────────────────────────────────╮").cyan()
        );
        eprintln!(
            "  {} {}",
            style("│").cyan(),
            style(concat!("◆ zi · Zistral ", env!("CARGO_PKG_VERSION"))).bold()
        );
        if let Ok(cwd) = cli
            .cwd
            .as_ref()
            .map(std::fs::canonicalize)
            .unwrap_or_else(std::env::current_dir)
        {
            let path = clean(&cwd.display().to_string());
            eprintln!(
                "  {} {}",
                style("│").cyan(),
                style(console::truncate_str(&path, 48, "…")).dim()
            );
        }
        eprintln!(
            "  {}",
            style("╰────────────────────────────────────────────────────╯").cyan()
        );
        eprintln!("  {}", style("Your lightweight terminal agent").dim());
        eprintln!("  Ask a question below. Each prompt starts a fresh task.");
        eprintln!(
            "  {}\n",
            style("/help  /config  /save PATH  /clear  /exit · Ctrl+C to stop").dim()
        );
    } else {
        eprintln!(
            "zi {} · /help /config /save PATH /exit · Each prompt is a fresh task.",
            env!("CARGO_PKG_VERSION")
        );
    }
}

pub fn model(settings: &Settings, cli: &Cli) {
    if enabled(cli) {
        eprintln!(
            "  {} {}  {} {}",
            style("model").dim(),
            style(clean(&settings.model)).cyan(),
            style("·").dim(),
            style(&settings.api).dim()
        );
    }
}

pub fn result(output: &str, cli: &Cli) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let mut code = false;
    for line in clean(output).lines() {
        if line.trim_start().starts_with("```") {
            code = !code;
            writeln!(stdout, "  {}", style(line).dim())?;
        } else if code {
            writeln!(stdout, "  {}", style(line).yellow())?;
        } else if line.starts_with('#') {
            writeln!(stdout, "{}", style(line).cyan().bold())?;
        } else if line.trim_start().starts_with(['-', '*', '>']) {
            writeln!(stdout, "{}", style(line).green())?;
        } else {
            writeln!(stdout, "{line}")?;
        }
    }
    if output.is_empty() {
        writeln!(stdout)?;
    }
    stdout.flush()?;
    if enabled(cli) {
        eprintln!();
    }
    Ok(())
}

pub struct Activity {
    bar: ProgressBar,
    start: Instant,
    rich: bool,
    verbose: bool,
    finish_label: &'static str,
    live: Cell<bool>,
    requests: Cell<u32>,
    tools: Cell<u32>,
    input_tokens: Cell<u64>,
    output_tokens: Cell<u64>,
}

impl Activity {
    pub fn new(settings: &Settings, cli: &Cli, bytes: usize) -> Self {
        let rich = enabled(cli);
        let bar = if rich {
            ProgressBar::new_spinner()
        } else {
            ProgressBar::hidden()
        };
        if rich {
            model(settings, cli);
            if bytes > 0 {
                eprintln!("  {} {} bytes", style("input").dim(), bytes);
            }
            bar.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {msg} {elapsed:.dim}")
                    .expect("static progress template")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            bar.enable_steady_tick(Duration::from_millis(100));
        }
        Self {
            bar,
            rich,
            live: Cell::new(false),
            verbose: cli.verbose,
            finish_label: if cli.export_plan {
                "Plan ready"
            } else if cli.output.is_some() {
                "Response ready"
            } else {
                "Done"
            },
            start: Instant::now(),
            requests: Cell::new(0),
            tools: Cell::new(0),
            input_tokens: Cell::new(0),
            output_tokens: Cell::new(0),
        }
    }

    pub fn request(&self, step: u32, max: u32) {
        if self.rich && !self.live.get() {
            self.bar
                .set_message(format!("Thinking · turn {step}/{max}"));
        } else if self.verbose {
            eprintln!("zi: turn {step}/{max}");
        }
    }

    pub fn attempt(&self) {
        self.requests.set(self.requests.get() + 1);
    }

    pub fn retry(&self, status: u16, count: u32, max: u32, delay: Duration) {
        let message = format!(
            "Retry {count}/{max} · HTTP {status} · {:.2}s",
            delay.as_secs_f64()
        );
        if self.rich && !self.live.get() {
            self.bar
                .println(format!("  {} {message}", style("↻").yellow()));
            self.bar
                .set_message(format!("Waiting to retry · {:.2}s", delay.as_secs_f64()));
        } else if self.rich || self.verbose {
            eprintln!("zi: {message}");
        }
    }

    pub fn streaming(&self) {
        if !self.live.replace(true) {
            self.bar.finish_and_clear();
        }
    }

    pub fn usage(&self, value: &serde_json::Value) {
        let usage = &value["usage"];
        self.input_tokens.set(
            self.input_tokens.get().saturating_add(
                usage["input_tokens"]
                    .as_u64()
                    .or_else(|| usage["prompt_tokens"].as_u64())
                    .unwrap_or(0),
            ),
        );
        self.output_tokens.set(
            self.output_tokens.get().saturating_add(
                usage["output_tokens"]
                    .as_u64()
                    .or_else(|| usage["completion_tokens"].as_u64())
                    .unwrap_or(0),
            ),
        );
    }

    pub fn tool(&self, name: &str, plan_export: bool) {
        self.tools.set(self.tools.get() + 1);
        let label = if plan_export {
            "Validate/export native plan"
        } else {
            match name {
                "count_input" => "Count input",
                "select_input" => "Select JSON",
                "transform_input" => "Transform input",
                "project_info" => "Project instructions",
                "list_files" => "List files",
                "read_file" => "Read file",
                "search_files" => "Search project",
                "write_file" => "Write file",
                "edit_file" => "Edit file",
                "delete_file" => "Delete file",
                "undo_edit" => "Undo file edit",
                "run_command" => "Run command",
                "git_status" => "Git status",
                "git_diff" => "Git diff",
                "update_plan" => "Task plan",
                _ => "Unknown tool",
            }
        };
        if self.rich && self.live.get() {
            eprintln!("\n  {} {label}", style("↳").cyan());
        } else if self.rich {
            self.bar.println(format!("  {} {label}", style("↳").cyan()));
        } else if self.verbose {
            eprintln!("zi: tool {}", clean(name));
        }
    }

    pub fn detail(&self, detail: &str) {
        let safe = clean(detail).replace(['\n', '\r'], " ");
        let safe = console::truncate_str(&safe, 160, "…");
        if self.rich && self.live.get() {
            eprintln!("  {}", console::style(safe).dim());
        } else if self.rich {
            self.bar
                .println(format!("    {}", console::style(safe).dim()));
        } else if self.verbose {
            eprintln!("zi: {safe}");
        }
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
        if self.rich {
            let tokens = if self
                .input_tokens
                .get()
                .saturating_add(self.output_tokens.get())
                > 0
            {
                format!(
                    " · {} in / {} out tokens",
                    self.input_tokens.get(),
                    self.output_tokens.get()
                )
            } else {
                String::new()
            };
            eprintln!(
                "  {} {}",
                style("✓").green(),
                style(format!(
                    "{} · {:.1}s · {} requests · {} tools{tokens}",
                    self.finish_label,
                    self.start.elapsed().as_secs_f64(),
                    self.requests.get(),
                    self.tools.get()
                ))
                .dim()
            );
            eprintln!();
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

/// Stateful sanitizing across chunk boundaries; no provider terminal commands.
pub struct StreamDisplay {
    pub any: bool,
    terminal: bool,
    state: u8,
    newline: bool,
}

impl StreamDisplay {
    pub fn new() -> Self {
        Self {
            any: false,
            terminal: io::stdout().is_terminal(),
            state: 0,
            newline: true,
        }
    }
    pub fn push(&mut self, text: &str) -> Result<()> {
        self.any = true;
        let mut safe = String::new();
        for c in text.chars() {
            if !self.terminal {
                safe.push(c);
                continue;
            }
            match self.state {
                0 if c == '\x1b' => self.state = 1,
                0 if !c.is_control() || matches!(c, '\n' | '\t') => safe.push(c),
                1 => {
                    self.state = match c {
                        '[' => 2,
                        ']' | 'P' | '^' | '_' => 3,
                        _ => 0,
                    }
                }
                2 if ('@'..='~').contains(&c) => self.state = 0,
                3 if c == '\x07' => self.state = 0,
                3 if c == '\x1b' => self.state = 4,
                4 => {
                    self.state = if c == '\\' {
                        0
                    } else if c == '\x1b' {
                        4
                    } else {
                        3
                    }
                }
                _ => (),
            }
        }
        if !safe.is_empty() {
            self.newline = safe.ends_with('\n');
            let mut stdout = io::stdout().lock();
            stdout.write_all(safe.as_bytes())?;
            stdout.flush()?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<()> {
        self.state = 0;
        if self.any && !self.newline {
            self.newline = true;
            let mut stdout = io::stdout().lock();
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
        Ok(())
    }
}
impl Drop for StreamDisplay {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_terminal_commands_are_removed() {
        assert_eq!(
            super::clean("a\x1b[2Jb\x1b]52;c;secret\x07c\x1b]0;title\x1b\\d\0\u{009b}\n"),
            "abcd\n"
        );
        assert_eq!(super::clean("你好\tworld"), "你好\tworld");
    }

    #[test]
    fn json_display_preserves_values_and_original_tokens() {
        // Validate before display, as main::emit does. Assert application text,
        // independently of whether a terminal host consumes literal C1 bytes.
        let controls: String = (0x7f..=0x9f).map(|c| char::from_u32(c).unwrap()).collect();
        let text =
            format!(r#"{{"{controls}":"LEFT{controls}RIGHT","escaped":"\u0080","text":"中文"}}"#);
        crate::transforms::validate_json(&text).unwrap();
        let shown = super::json_display(&text);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&shown).unwrap(),
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            "JSON display changed a string value or key"
        );
        assert!(!shown.chars().any(char::is_control));
        assert!(shown.contains(r#"\u007f\u0080"#));
        assert!(shown.contains(r#""escaped":"\u0080""#));
        let tokens = r#"{"n":1.2300E+400,"n":-0.00,"s":"\u4E2D","a":[true,null]}"#;
        crate::transforms::validate_json(tokens).unwrap();
        assert_eq!(super::json_display(tokens), tokens);
    }
}
