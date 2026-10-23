mod agent;
mod config;
mod data;
mod doctor;
mod imports;
mod stream;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;

/// A small, pipe-native agent. stdout is always your result.
#[derive(Parser, Clone, Debug)]
#[command(
    name = "zi",
    bin_name = "zi",
    version,
    subcommand_precedence_over_arg = true,
    after_help = "Examples:\n  zi config\n  cat data.json | zi 'Join the first item of .abc'\n  cat article.txt | zi 对内容进行字数统计\n  cat article.txt | zi --count\n  cat data.json | zi --select /abc/0 --join ','\n\nRun zi config once to save your model service and API key.\nUse --config-example to print a configuration template."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// What to do (multiple unquoted words are joined with spaces)
    #[arg(trailing_var_arg = true)]
    prompt: Vec<String>,
    /// Read UTF-8 data from a file instead of stdin; '-' means stdin
    #[arg(short = 'i', long)]
    input: Option<PathBuf>,
    /// Model identifier, overrides configuration
    #[arg(short, long, env = "ZI_MODEL")]
    model: Option<String>,
    /// API root, normally ending in /v1
    #[arg(long, env = "ZI_BASE_URL")]
    base_url: Option<String>,
    /// Wire protocol: chat or responses
    #[arg(long, env = "ZI_API", value_parser = ["chat", "responses", "anthropic"])]
    api: Option<String>,
    /// Configuration file (default: platform config directory/zistral/config.toml)
    #[arg(long, env = "ZI_CONFIG", global = true)]
    config: Option<PathBuf>,
    /// Select a saved model profile for this invocation
    #[arg(short = 'p', long, env = "ZI_PROFILE", global = true)]
    profile: Option<String>,
    /// Stream text even when stdout is redirected (partial output on errors)
    #[arg(long, conflicts_with_all = ["no_stream", "json", "count", "select", "config_example"])]
    stream: bool,
    /// Wait for the complete answer, including in a terminal
    #[arg(long)]
    no_stream: bool,
    /// Print a configuration template without reading input or contacting a model
    #[arg(long)]
    config_example: bool,
    /// Require valid JSON output and emit compact JSON
    #[arg(long)]
    json: bool,
    /// Disable built-in agent tools (for text-only models)
    #[arg(long)]
    no_tools: bool,
    /// Disable terminal decorations and progress (pipe output is always plain)
    #[arg(long, global = true)]
    plain: bool,
    /// Print tool activity to stderr
    #[arg(short, long)]
    verbose: bool,
    /// Maximum model requests, including the final answer
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=64))]
    max_steps: u32,
    /// Timeout per HTTP request in seconds
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
    /// Maximum output tokens for the Anthropic Messages API
    #[arg(long, default_value_t = 4096, value_parser = clap::value_parser!(u32).range(1..=65536))]
    max_output_tokens: u32,
    /// Maximum input size in bytes
    #[arg(long, default_value_t = 8_388_608, value_parser = clap::value_parser!(u64).range(1..=67_108_864))]
    max_input: u64,
    /// Count text locally, without a model or API key
    #[arg(long, conflicts_with = "select")]
    count: bool,
    /// Select a JSON value locally using an RFC 6901 pointer, e.g. /abc/0
    #[arg(long)]
    select: Option<String>,
    /// Join selected array values with this separator (requires --select)
    #[arg(long, requires = "select")]
    join: Option<String>,
}

#[derive(Subcommand, Clone, Debug)]
enum Commands {
    /// List saved model profiles without displaying keys
    Models,
    /// Choose the default saved model profile
    Use { name: String },
    /// Diagnose configuration; --probe sends a small model request
    Doctor {
        #[arg(long)]
        probe: bool,
    },
    /// Save or import a model configuration (optionally named)
    Config {
        /// Save/edit this named profile, preserving all other models
        #[arg(long)]
        name: Option<String>,
        /// Skip detection and enter settings manually
        #[arg(long)]
        manual: bool,
        /// List detected model configurations without importing or showing keys
        #[arg(long, conflicts_with_all = ["manual", "name"])]
        list: bool,
    },
}

fn read_input(cli: &Cli) -> Result<String> {
    let mut reader: Box<dyn Read> = match &cli.input {
        Some(path) if path.as_os_str() != "-" => Box::new(
            std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?,
        ),
        _ if cli.input.is_some() || !io::stdin().is_terminal() => Box::new(io::stdin()),
        _ => return Ok(String::new()),
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(cli.max_input + 1)
        .read_to_end(&mut bytes)
        .context("cannot read input")?;
    if bytes.len() as u64 > cli.max_input {
        bail!(
            "input exceeds {} bytes; increase --max-input explicitly",
            cli.max_input
        );
    }
    let text = String::from_utf8(bytes)
        .context("input must be UTF-8; convert its encoding before piping to zi")?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

fn run(mut cli: Cli) -> Result<()> {
    if std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
    if let Some(command) = &cli.command {
        if !matches!(command, Commands::Config { .. }) {
            if cli.input.is_some()
                || cli.count
                || cli.select.is_some()
                || cli.config_example
                || cli.json
                || cli.stream
                || !cli.prompt.is_empty()
            {
                bail!("this command does not accept input data or an instruction");
            }
            return match command {
                Commands::Models => config::models(&cli),
                Commands::Use { name } => config::use_profile(&cli, name),
                Commands::Doctor { probe } => doctor::run(&cli, *probe),
                _ => unreachable!(),
            };
        }
    }
    if let Some(Commands::Config { manual, list, name }) = &cli.command {
        if cli.input.is_some()
            || cli.count
            || cli.select.is_some()
            || cli.config_example
            || cli.json
            || cli.no_tools
            || cli.verbose
            || cli.stream
            || !cli.prompt.is_empty()
        {
            bail!("invalid options for zi config; see zi config --help");
        }
        if *list {
            return config::list_imports();
        }
        return config::configure_named(&cli, *manual, name.as_deref());
    }
    if cli.config_example {
        return write_output(config::EXAMPLE);
    }
    if (cli.count || cli.select.is_some()) && !cli.prompt.is_empty() {
        bail!("local --count/--select modes do not accept a prompt");
    }
    if !cli.count && cli.select.is_none() && cli.prompt.is_empty() {
        if cli.input.is_none() && io::stdin().is_terminal() && io::stdout().is_terminal() {
            return session(&mut cli);
        }
        bail!("provide an instruction, or use --count / --select; see zi --help");
    }
    let input = read_input(&cli)?;
    let output = if cli.count {
        serde_json::to_string(&data::counts(&input))?
    } else if let Some(pointer) = &cli.select {
        data::select(&input, pointer, cli.join.as_deref())?
    } else {
        if io::stdin().is_terminal() && io::stdout().is_terminal() && config::needs_setup(&cli) {
            config::configure(&cli, false)?;
        }
        let settings = config::Settings::load(&cli)?;
        let answer = agent::run(&settings, &cli, &input)?;
        if answer.streamed {
            return Ok(());
        }
        answer.text
    };
    let output = if cli.json {
        let value: serde_json::Value = serde_json::from_str(&output)
            .context("output is not valid JSON (nothing written to stdout)")?;
        serde_json::to_string(&value)?
    } else {
        output
    };
    emit(&output, &cli)
}

fn emit(output: &str, cli: &Cli) -> Result<()> {
    if ui::enabled(cli) && !cli.count && cli.select.is_none() {
        ui::result(output, cli)
    } else {
        write_output(output)
    }
}

fn session(cli: &mut Cli) -> Result<()> {
    ui::welcome(cli);
    if config::needs_setup(cli) {
        eprintln!("Let's connect your model. Existing agent settings can be imported.");
        config::configure(cli, false)?;
    }
    let mut settings = config::Settings::load(cli)?;
    ui::model(&settings, cli);
    loop {
        if ui::enabled(cli) {
            eprint!("\n  {} ", console::style("›").cyan().bold());
        } else {
            eprint!("\nzi> ");
        }
        io::stderr().flush()?;
        let mut line = String::new();
        let read = io::stdin()
            .lock()
            .take(cli.max_input + 1)
            .read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }
        if read as u64 > cli.max_input {
            bail!("instruction exceeds --max-input");
        }
        let prompt = line.trim();
        match prompt {
            "" => continue,
            "/exit" | "/quit" => {
                eprintln!("Goodbye.");
                return Ok(());
            }
            "/help" => {
                eprintln!(
                    "Ask a question, or use /models, /use NAME, /doctor, /config, /clear, /exit. Each prompt is an independent task; no chat history is sent or saved.\nFor files/pipes: cat file.txt | zi 'summarize' or zi -i file.txt 'summarize'."
                );
                continue;
            }
            "/models" => {
                config::models(cli)?;
                continue;
            }
            _ if prompt.starts_with("/use ") => {
                match config::use_profile(cli, prompt[5..].trim()) {
                    Ok(()) => {
                        cli.profile = Some(prompt[5..].trim().into());
                        settings = config::Settings::load(cli)?;
                        ui::model(&settings, cli);
                    }
                    Err(error) => eprintln!("zi: {}", ui::clean(&error.to_string())),
                }
                continue;
            }
            "/doctor" => {
                if let Err(error) = doctor::run(cli, false) {
                    eprintln!("zi: {}", ui::clean(&error.to_string()));
                }
                continue;
            }
            "/config" => {
                config::configure(cli, false)?;
                settings = config::Settings::load(cli)?;
                ui::model(&settings, cli);
                continue;
            }
            "/clear" => {
                console::Term::stderr().clear_screen()?;
                ui::welcome(cli);
                ui::model(&settings, cli);
                continue;
            }
            _ if prompt.starts_with('/') => {
                eprintln!("Unknown command. Use /help.");
                continue;
            }
            _ => (),
        }
        cli.prompt = vec![prompt.into()];
        match agent::run(&settings, cli, "") {
            Ok(answer) => {
                if answer.streamed {
                    continue;
                }
                let answer = answer.text;
                if cli.json {
                    match serde_json::from_str::<serde_json::Value>(&answer) {
                        Ok(value) => emit(&serde_json::to_string(&value)?, cli)?,
                        Err(_) => {
                            eprintln!("zi: output is not valid JSON (nothing written to stdout)")
                        }
                    }
                } else {
                    emit(&answer, cli)?;
                }
            }
            Err(error) => eprintln!("zi: {}", ui::clean(&format!("{error:#}"))),
        }
    }
}

fn write_output(output: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(output.as_bytes())?;
    if !output.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        if error
            .chain()
            .filter_map(|e| e.downcast_ref::<io::Error>())
            .any(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!("zi: {}", ui::clean(&format!("{error:#}")));
        std::process::exit(1);
    }
}
