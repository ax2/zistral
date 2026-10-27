mod agent;
mod config;
mod csv_input;
mod csv_output;
mod data;
mod doctor;
mod imports;
mod output;
mod presets;
mod process;
mod project;
mod recipes;
mod retry;
mod selection;
mod stream;
mod transforms;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;

/// A small, pipe-native agent. Results go to stdout or an explicit output file.
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
    #[arg(short = 'i', long, global = true)]
    input: Option<PathBuf>,
    /// Save the complete result to a file; '-' keeps stdout. Default: never overwrite
    #[arg(short = 'o', long, global = true)]
    output: Option<PathBuf>,
    /// Replace an existing regular output file after a successful task
    #[arg(long, global = true, requires = "output")]
    force: bool,
    /// Use a builtin or user-defined task preset (zi presets lists names)
    #[arg(short = 's', long)]
    preset: Option<String>,
    /// User preset file (default: presets.toml beside the selected config)
    #[arg(long, env = "ZI_PRESETS", global = true)]
    presets: Option<PathBuf>,
    /// Set a literal preset variable; repeat for multiple NAME=VALUE pairs
    #[arg(long = "var", global = true)]
    vars: Vec<String>,
    /// Model identifier, overrides configuration
    #[arg(short, long, env = "ZI_MODEL")]
    model: Option<String>,
    /// API root, normally ending in /v1
    #[arg(long, env = "ZI_BASE_URL")]
    base_url: Option<String>,
    /// Wire protocol: chat, responses or anthropic
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
    #[arg(long, conflicts_with_all = ["count", "select", "json", "input", "prompt"])]
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
    /// Enable project file, planning, Git and shell tools in the current directory
    #[arg(long)]
    agent: bool,
    /// Select a project directory and enable project tools
    #[arg(short = 'C', long)]
    cwd: Option<PathBuf>,
    /// Enable project inspection but disable file changes and shell commands
    #[arg(long)]
    read_only: bool,
    /// Enable project tools but disable arbitrary shell commands
    #[arg(long)]
    no_exec: bool,
    /// Maximum duration of each project command (seconds)
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
    command_timeout: u64,
    /// Maximum model turns, excluding transient retries (default: project 32, data 8)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=128))]
    max_steps: Option<u32>,
    /// Timeout per HTTP request in seconds
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
    /// Extra attempts per model turn for transient HTTP errors (0-3; default: no retry)
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=3))]
    retries: u32,
    /// Maximum output tokens for the Anthropic Messages API
    #[arg(long, default_value_t = 4096, value_parser = clap::value_parser!(u32).range(1..=65536))]
    max_output_tokens: u32,
    /// Maximum input size in bytes
    #[arg(long, global = true, default_value_t = 8_388_608, value_parser = clap::value_parser!(u64).range(1..=67_108_864))]
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
    /// Transform explicit input locally, without a model or configuration
    #[command(
        after_help = "Examples:\n  zi data trim -i notes.txt\n  zi data --step trim --step dedupe --step sort:reverse -i notes.txt\n  zi data --recipe cleanup.toml -i notes.txt -o clean.txt\n  zi data --step json-select:/data/items --step array-to-jsonl -i response.json\n  zi data json-select --pointer /data/items -i response.json\n  zi data jsonl-select --pointer /payload/id -i events.jsonl\n  zi data --step csv-to-jsonl --step jsonl-select:/id --step dedupe --step jsonl-to-array -i table.csv\n  zi data jsonl-to-csv -i records.jsonl -o table.csv\n  zi data --step jsonl-select:/payload/id --step dedupe --step jsonl-to-array\n  zi data --show-plan --recipe cleanup.toml\n  zi data --show-plan --step trim --step dedupe -o cleanup.toml\n\nRecipe example (UTF-8 TOML):\n  version = 1\n  [[steps]]\n  operation = \"trim\"\n  [[steps]]\n  operation = \"sort\"\n  reverse = true\n\nUse one operation, repeated --step, or --recipe. --show-plan validates and exports\nthe resolved version-1 TOML without reading stdin or executing data operations;\n--input is rejected and -o saves the plan. Data-dependent failures still require a run.\nRecipes allow only native data operations: no shell, file paths or model requests.\nLimits: 16 steps, 64 KiB recipe, 8 MiB initial input, 16 MiB intermediate/final result."
    )]
    Data {
        #[arg(value_enum, required_unless_present_any = ["steps", "recipe"], conflicts_with_all = ["steps", "recipe"])]
        operation: Option<transforms::Operation>,
        /// Reverse line order; only valid with sort
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        reverse: bool,
        /// RFC 6901 pointer for json-select/jsonl-select; empty selects each root
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        pointer: Option<String>,
        /// Chain a native operation; repeat in execution order (sort:reverse, json-select:/path and jsonl-select:/path are supported)
        #[arg(long = "step", conflicts_with = "recipe")]
        steps: Vec<String>,
        /// Read an explicit versioned TOML recipe of native operations
        #[arg(long)]
        recipe: Option<PathBuf>,
        /// Validate and export a reusable TOML plan without reading data or running steps
        #[arg(long)]
        show_plan: bool,
    },
    /// List, inspect, render, export or initialize reusable task presets (offline)
    Presets {
        #[command(subcommand)]
        action: Option<PresetAction>,
    },
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

#[derive(Subcommand, Clone, Debug)]
enum PresetAction {
    /// List builtin and user-defined presets
    List,
    /// Inspect a preset without sending input to a model
    Show { name: String },
    /// Preview a resolved prompt; accepts --var NAME=VALUE
    Render { name: String },
    /// Export one reusable TOML preset; --var saves new defaults without rendering the template
    Export {
        /// Source builtin or user-defined preset
        name: String,
        /// Required custom name in the exported definition; cannot replace a builtin
        #[arg(long = "name")]
        new_name: String,
    },
    /// Create an example user preset file; never overwrite an existing file
    Init,
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
    // Validate requested modes even when --no-tools disables effective project capabilities.
    if (cli.command.is_some() || cli.count || cli.select.is_some() || cli.config_example)
        && (cli.agent || cli.cwd.is_some() || cli.read_only || cli.no_exec)
    {
        bail!(
            "project options (--agent, --cwd/-C, --read-only, --no-exec) are available for model tasks only; remove them for offline and management commands"
        );
    }
    if cli.retries > 0
        && (cli.command.is_some() || cli.count || cli.select.is_some() || cli.config_example)
    {
        bail!("--retries is for model tasks only; diagnostics and offline commands do not retry");
    }
    if cli.output.is_some()
        && cli.command.as_ref().is_some_and(|c| {
            !matches!(
                c,
                Commands::Data { .. }
                    | Commands::Presets {
                        action: Some(PresetAction::Export { .. })
                    }
            )
        })
    {
        bail!(
            "--output is available for data/model tasks and presets export, not other management commands"
        );
    }
    if cli.output.as_ref().is_some_and(|p| p.as_os_str() == "-") {
        if cli.force {
            bail!("--force requires a real output file, not stdout '-'");
        }
        cli.output = None;
    }
    if cli.output.is_some() && cli.stream {
        bail!("--stream cannot be combined with file output; use stdout for live output");
    }
    if std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
    if let Some(Commands::Data {
        operation,
        reverse,
        pointer,
        steps,
        recipe,
        show_plan,
    }) = &cli.command
    {
        if cli.preset.is_some()
            || !cli.vars.is_empty()
            || cli.count
            || cli.select.is_some()
            || cli.json
            || cli.stream
            || cli.config_example
            || !cli.prompt.is_empty()
            || project::enabled(&cli)
        {
            bail!("zi data accepts input and local operation options only; see zi data --help");
        }
        if *show_plan && cli.input.is_some() {
            bail!("--show-plan does not read data; omit --input to preview or export the plan");
        }
        let plan = recipes::Plan::prepare(
            *operation,
            *reverse,
            pointer.as_deref(),
            steps,
            recipe.as_deref(),
        )?;
        let target = prepare_output(&cli)?;
        let output = if *show_plan {
            plan.render()?
        } else {
            let input = read_input(&cli)?;
            plan.run(&input)?
        };
        if let Some(target) = target {
            return save_output(
                &output,
                target,
                &cli,
                !output.is_empty() && !output.ends_with('\n'),
            );
        }
        return if output.is_empty() {
            Ok(())
        } else {
            write_output(&output)
        };
    }
    if project::enabled(&cli) {
        process::install_interrupt_handler()?;
    }
    if cli.command.is_some() && cli.preset.is_some() {
        bail!("--preset is for model tasks; use zi presets to manage presets");
    }
    if let Some(Commands::Presets { action }) = &cli.command {
        if cli.input.is_some()
            || cli.count
            || cli.select.is_some()
            || cli.config_example
            || cli.json
            || cli.stream
            || !cli.prompt.is_empty()
        {
            bail!("zi presets does not accept input data or a model instruction");
        }
        let output = match action {
            None | Some(PresetAction::List) => {
                if !cli.vars.is_empty() {
                    bail!("use zi presets render NAME --var NAME=VALUE");
                }
                presets::list(&cli)?
            }
            Some(PresetAction::Show { name }) => presets::show(&cli, name, false)?,
            Some(PresetAction::Render { name }) => presets::show(&cli, name, true)?,
            Some(PresetAction::Export { name, new_name }) => {
                let output = presets::export(&cli, name, new_name)?;
                if let Some(target) = prepare_output(&cli)? {
                    return save_output(&output, target, &cli, !output.ends_with('\n'));
                }
                output
            }
            Some(PresetAction::Init) => presets::init(&cli)?,
        };
        return write_output(&output);
    }
    if cli.command.is_some() && !cli.vars.is_empty() {
        bail!("--var is only supported for preset tasks and zi presets render");
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
    presets::apply(&mut cli)?;
    if cli.config_example {
        if let Some(target) = prepare_output(&cli)? {
            return save_output(
                config::EXAMPLE,
                target,
                &cli,
                !config::EXAMPLE.ends_with('\n'),
            );
        }
        return write_output(config::EXAMPLE);
    }
    if (cli.count || cli.select.is_some()) && !cli.prompt.is_empty() {
        bail!("local --count/--select modes do not accept a prompt");
    }
    if !cli.count && cli.select.is_none() && cli.prompt.is_empty() {
        if cli.output.is_some() {
            bail!("provide an instruction for file output; interactive sessions use stdout");
        }
        if cli.input.is_none() && io::stdin().is_terminal() && io::stdout().is_terminal() {
            return session(&mut cli);
        }
        bail!("provide an instruction, or use --count / --select; see zi --help");
    }
    let target = prepare_output(&cli)?;
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
        transforms::compact_json(&output, data::RESULT_LIMIT).context(if cli.output.is_some() {
            "output is not valid JSON (nothing published)"
        } else {
            "output is not valid JSON (nothing written to stdout)"
        })?
    } else {
        output
    };
    if let Some(target) = target {
        return save_output(&output, target, &cli, !output.ends_with('\n'));
    }
    emit(&output, &cli)
}

fn prepare_output(cli: &Cli) -> Result<Option<output::Prepared>> {
    cli.output
        .as_ref()
        .map(|p| output::Prepared::new(p, cli.force))
        .transpose()
}

fn save_output(
    text: &str,
    target: output::Prepared,
    cli: &Cli,
    append_newline: bool,
) -> Result<()> {
    let bytes = target.save(text, append_newline)?;
    if ui::enabled(cli) || cli.verbose {
        if let Some(path) = &cli.output {
            eprintln!(
                "  {} {} ({} bytes)",
                console::style("Saved").green().bold(),
                ui::clean(&path.display().to_string()),
                bytes
            );
        }
    }
    Ok(())
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
        let mut scene = None;
        match prompt {
            "" => continue,
            "/exit" | "/quit" => {
                eprintln!("Goodbye.");
                return Ok(());
            }
            "/help" => {
                eprintln!(
                    "Ask a question, or use /presets, /run NAME [instruction], /models, /use NAME, /doctor, /config, /clear, /exit. Each prompt is an independent task; no chat history is sent or saved.\nFor files/pipes: cat file.txt | zi 'summarize' or zi -i file.txt 'summarize'."
                );
                continue;
            }
            "/presets" => {
                match presets::list(cli) {
                    Ok(list) => eprintln!("{}", ui::clean(&list)),
                    Err(error) => eprintln!("zi: {}", ui::clean(&error.to_string())),
                }
                continue;
            }
            _ if prompt.starts_with("/run ") => {
                let value = prompt[5..].trim();
                let (name, instruction) =
                    value.split_once(char::is_whitespace).unwrap_or((value, ""));
                scene = Some((name, instruction.trim()));
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
        let mut task = cli.clone();
        task.prompt = vec![prompt.into()];
        if let Some((name, instruction)) = scene {
            task.preset = Some(name.into());
            task.prompt = if instruction.is_empty() {
                Vec::new()
            } else {
                vec![instruction.into()]
            };
        }
        if let Err(error) = presets::apply(&mut task) {
            eprintln!("zi: {}", ui::clean(&error.to_string()));
            continue;
        }
        match agent::run(&settings, &task, "") {
            Ok(answer) => {
                if answer.streamed {
                    continue;
                }
                let answer = answer.text;
                if task.json {
                    match transforms::compact_json(&answer, data::RESULT_LIMIT) {
                        Ok(value) => emit(&value, &task)?,
                        Err(_) => {
                            eprintln!("zi: output is not valid JSON (nothing written to stdout)")
                        }
                    }
                } else {
                    emit(&answer, &task)?;
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
