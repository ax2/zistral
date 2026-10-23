mod agent;
mod config;
mod data;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

/// A small, pipe-native agent. stdout is always your result.
#[derive(Parser, Debug)]
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
    #[arg(long, env = "ZI_API", value_parser = ["chat", "responses"])]
    api: Option<String>,
    /// Configuration file (default: platform config directory/zistral/config.toml)
    #[arg(long, env = "ZI_CONFIG", global = true)]
    config: Option<PathBuf>,
    /// Print a configuration template without reading input or contacting a model
    #[arg(long)]
    config_example: bool,
    /// Require valid JSON output and emit compact JSON
    #[arg(long)]
    json: bool,
    /// Disable built-in agent tools (for text-only models)
    #[arg(long)]
    no_tools: bool,
    /// Print tool activity to stderr
    #[arg(short, long)]
    verbose: bool,
    /// Maximum model requests, including the final answer
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=64))]
    max_steps: u32,
    /// Timeout per HTTP request in seconds
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
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

#[derive(Subcommand, Debug)]
enum Commands {
    /// Interactively save the service URL, model, protocol, and hidden API key
    Config,
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

fn run(cli: Cli) -> Result<()> {
    if cli.command.is_some() {
        if cli.input.is_some()
            || cli.count
            || cli.select.is_some()
            || cli.config_example
            || cli.json
            || cli.no_tools
            || cli.verbose
            || !cli.prompt.is_empty()
        {
            bail!("zi config only accepts --config PATH; see zi config --help");
        }
        return config::configure(&cli);
    }
    if cli.config_example {
        return write_output(config::EXAMPLE);
    }
    if (cli.count || cli.select.is_some()) && !cli.prompt.is_empty() {
        bail!("local --count/--select modes do not accept a prompt");
    }
    if !cli.count && cli.select.is_none() && cli.prompt.is_empty() {
        bail!("provide an instruction, or use --count / --select; see zi --help");
    }
    let input = read_input(&cli)?;
    let output = if cli.count {
        serde_json::to_string(&data::counts(&input))?
    } else if let Some(pointer) = &cli.select {
        data::select(&input, pointer, cli.join.as_deref())?
    } else {
        let settings = config::Settings::load(&cli)?;
        agent::run(&settings, &cli, &input)?
    };
    let output = if cli.json {
        let value: serde_json::Value = serde_json::from_str(&output)
            .context("output is not valid JSON (nothing written to stdout)")?;
        serde_json::to_string(&value)?
    } else {
        output
    };
    write_output(&output)
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
        eprintln!("zi: {error:#}");
        std::process::exit(1);
    }
}
