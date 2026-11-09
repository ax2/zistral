mod agent;
mod config;
mod csv_input;
mod csv_output;
mod data;
mod definition;
mod doctor;
mod imports;
mod integer;
mod last_result;
mod models;
mod output;
mod presets;
mod process;
mod profile;
mod project;
mod projection;
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
    after_help = "Examples:\n  zi config\n  zi use work\n  zi doctor --json\n  zi --preview --no-stream -i data.json 'Summarize the input'\n  cat data.json | zi 'Join the first item of .abc'\n  cat article.txt | zi 对内容进行字数统计\n  cat article.txt | zi --count\n  cat data.json | zi --select /abc/0 --join ','\n\nRun zi config once to save your model service and API key.\nUse zi COMMAND --help for command-specific options and examples.\nUse --config-example to print a configuration template.\nmodels is offline; models --remote and doctor --probe explicitly contact the service."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// What to do (multiple unquoted words are joined with spaces)
    #[arg(trailing_var_arg = true)]
    prompt: Vec<String>,
    /// Read UTF-8 data from a file instead of stdin; '-' means stdin
    #[arg(short = 'i', long, global = true, help_heading = "Input and output")]
    input: Option<PathBuf>,
    /// Save the complete result to a file; '-' keeps stdout. Default: never overwrite
    #[arg(short = 'o', long, global = true, help_heading = "Input and output")]
    output: Option<PathBuf>,
    /// Replace an existing regular output file after a successful task
    #[arg(
        long,
        global = true,
        requires = "output",
        help_heading = "Input and output"
    )]
    force: bool,
    /// Use a builtin or user-defined task preset (zi presets lists names)
    #[arg(short = 's', long, help_heading = "Task presets")]
    preset: Option<String>,
    /// User preset file (default: presets.toml beside the selected config)
    #[arg(
        long,
        env = "ZI_PRESETS",
        hide_env_values = true,
        global = true,
        help_heading = "Task presets"
    )]
    presets: Option<PathBuf>,
    /// Set a literal preset variable; repeat for multiple NAME=VALUE pairs
    #[arg(long = "var", global = true, help_heading = "Task presets")]
    vars: Vec<String>,
    /// Model identifier, overrides configuration
    #[arg(
        short,
        long,
        env = "ZI_MODEL",
        hide_env_values = true,
        help_heading = "Models and service"
    )]
    model: Option<String>,
    /// API root, normally ending in /v1
    #[arg(
        long,
        env = "ZI_BASE_URL",
        hide_env_values = true,
        help_heading = "Models and service"
    )]
    base_url: Option<String>,
    /// Wire protocol: chat, responses or anthropic
    #[arg(long, env = "ZI_API", hide_env_values = true, value_parser = ["chat", "responses", "anthropic"], help_heading = "Models and service")]
    api: Option<String>,
    /// Configuration file (default: platform config directory/zistral/config.toml)
    #[arg(
        long,
        env = "ZI_CONFIG",
        hide_env_values = true,
        global = true,
        help_heading = "Models and service"
    )]
    config: Option<PathBuf>,
    /// Select a saved model profile for this invocation
    #[arg(
        short = 'p',
        long,
        env = "ZI_PROFILE",
        hide_env_values = true,
        global = true,
        help_heading = "Models and service"
    )]
    profile: Option<String>,
    /// Stream text even when stdout is redirected (partial output on errors)
    #[arg(long, conflicts_with_all = ["no_stream", "json", "count", "select", "config_example"], help_heading = "Results and previews")]
    stream: bool,
    /// Wait for the complete answer, including in a terminal
    #[arg(long, help_heading = "Results and previews")]
    no_stream: bool,
    /// Print a configuration template without reading input or contacting a model
    #[arg(long, conflicts_with_all = ["count", "select", "json", "input", "prompt"], help_heading = "Models and service")]
    config_example: bool,
    /// Require valid JSON output and emit compact JSON
    #[arg(long, help_heading = "Results and previews")]
    json: bool,
    /// Return one successful native data tool result without a model rewrite
    #[arg(long, conflicts_with_all = ["no_tools", "stream", "count", "select", "config_example", "agent", "cwd", "read_only", "no_exec"], help_heading = "Results and previews")]
    exact: bool,
    /// Export the chosen native transform as a version-1 TOML recipe, without executing it
    #[arg(
        long,
        requires = "exact",
        conflicts_with = "json",
        help_heading = "Results and previews"
    )]
    export_plan: bool,
    /// Preview the first model request as JSON, without network/tool execution or credential headers
    #[arg(long, conflicts_with_all = ["count", "select", "config_example"], help_heading = "Results and previews")]
    preview: bool,
    /// Disable built-in agent tools (for text-only models)
    #[arg(long, help_heading = "Request controls")]
    no_tools: bool,
    /// Disable terminal decorations and progress (pipe output is always plain)
    #[arg(long, global = true, help_heading = "Display")]
    plain: bool,
    /// Print tool activity to stderr
    #[arg(short, long, help_heading = "Display")]
    verbose: bool,
    /// Enable project file, planning, Git and shell tools in the current directory
    #[arg(long, help_heading = "Project tools")]
    agent: bool,
    /// Select a project directory and enable project tools
    #[arg(short = 'C', long, help_heading = "Project tools")]
    cwd: Option<PathBuf>,
    /// Enable project inspection but disable file changes and shell commands
    #[arg(long, help_heading = "Project tools")]
    read_only: bool,
    /// Enable project tools but disable arbitrary shell commands
    #[arg(long, help_heading = "Project tools")]
    no_exec: bool,
    /// Maximum duration of each project command (seconds)
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600), help_heading = "Project tools")]
    command_timeout: u64,
    /// Maximum model turns, excluding transient retries (default: project 32, data 8)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=128), help_heading = "Request controls")]
    max_steps: Option<u32>,
    /// Timeout per HTTP request in seconds
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600), help_heading = "Request controls")]
    timeout: u64,
    /// Extra attempts per model turn for transient HTTP errors (0-3; default: no retry)
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=3), help_heading = "Request controls")]
    retries: u32,
    /// Requested output-token cap per model turn (Anthropic default: 4096; others: omitted)
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=65536), help_heading = "Request controls")]
    max_output_tokens: Option<u32>,
    /// Ignore saved model request-budget preferences for this invocation
    #[arg(long, help_heading = "Request controls")]
    no_profile_budget: bool,
    /// Chat cap field (model default: max_completion_tokens; probe default: max_tokens)
    #[arg(long, value_parser = ["max_completion_tokens", "max_tokens"], help_heading = "Request controls")]
    chat_token_limit_field: Option<String>,
    /// Maximum input size in bytes
    #[arg(long, global = true, default_value_t = 8_388_608, value_parser = clap::value_parser!(u64).range(1..=67_108_864), help_heading = "Input and output")]
    max_input: u64,
    /// Count text locally, without a model or API key
    #[arg(long, conflicts_with = "select", help_heading = "Offline shortcuts")]
    count: bool,
    /// Select a JSON value locally using an RFC 6901 pointer, e.g. /abc/0
    #[arg(long, help_heading = "Offline shortcuts")]
    select: Option<String>,
    /// Join selected array values with this separator (requires --select)
    #[arg(long, requires = "select", help_heading = "Offline shortcuts")]
    join: Option<String>,
}

#[derive(Subcommand, Clone, Debug)]
enum Commands {
    /// Transform explicit input locally, without a model or configuration
    #[command(
        after_help = "Examples:\n  zi data jsonl-redact --pointer /token --if-present -i mixed.jsonl\n  zi data --show-plan jsonl-redact --pointer /token --if-present -o mask.toml\n  zi data jsonl-profile -i events.jsonl\n  zi data --step json-select:/events --step array-to-jsonl --step jsonl-profile -i response.json\n  zi data jsonl-redact --pointer /credentials/token -i events.jsonl\n  zi data --step jsonl-redact:/credentials/token --step jsonl-redact:/user/email -i events.jsonl\n  zi data --list-recipes\n  zi data --recipe builtin:top-severity-counts -i events.jsonl\n  zi data jsonl-sort-int --pointer /count --reverse -i counts.jsonl\n  zi data --recipe builtin:severity-counts -i events.jsonl\n  zi data --show-plan --recipe builtin:recent-unique-events -o recent.toml\n  zi data jsonl-count-by --pointer /level -i events.jsonl\n  zi data jsonl-dedupe --pointer /id -i events.jsonl\n  zi data jsonl-sort --pointer /time --reverse -i events.jsonl\n  zi data jsonl-where --pointer /level --equals ERROR -i events.jsonl\n  zi data lines-tail --lines 50 -i app.log\n  zi data --step lines-include:ERROR --step lines-tail:20 -i app.log\n  zi data lines-include --contains ERROR -i app.log\n  zi data --step lines-include:ERROR --step lines-exclude:healthcheck -i app.log\n  zi data trim -i notes.txt\n  zi data --step trim --step dedupe --step sort:reverse -i notes.txt\n  zi data --recipe cleanup.toml -i notes.txt -o clean.txt\n  zi data --step json-select:/data/items --step array-to-jsonl -i response.json\n  zi data json-select --pointer /data/items -i response.json\n  zi data jsonl-select --pointer /payload/id -i events.jsonl\n  zi data --step csv-to-jsonl --step jsonl-select:/id --step dedupe --step jsonl-to-array -i table.csv\n  zi data jsonl-project --field id=/payload/id --field n=/payload/n -i events.jsonl\n  zi data jsonl-to-csv -i records.jsonl -o table.csv\n  zi data --step jsonl-select:/payload/id --step dedupe --step jsonl-to-array\n  zi data --show-plan --recipe cleanup.toml\n  zi data --show-plan --step trim --step dedupe -o cleanup.toml\n\nRecipe example (UTF-8 TOML):\n  version = 1\n  [[steps]]\n  operation = \"trim\"\n  [[steps]]\n  operation = \"sort\"\n  reverse = true\n\nUse one operation, repeated --step, or --recipe (FILE or builtin:NAME).\n--list-recipes lists embedded recipes offline without reading stdin; --input is rejected.\nBuiltin names are exact, case-sensitive references; unknown names never fall back to files.\n--show-plan validates and exports\nthe resolved version-1 TOML without reading stdin or executing data operations;\n--input is rejected and -o saves the plan. Data-dependent failures still require a run.\nRecipes allow only native data operations: no shell, file paths or model requests.\nLimits: 16 steps, 64 KiB recipe, 8 MiB initial input, 16 MiB intermediate/final result."
    )]
    Data {
        #[arg(value_enum, required_unless_present_any = ["steps", "recipe", "list_recipes"], conflicts_with_all = ["steps", "recipe", "list_recipes"])]
        operation: Option<transforms::Operation>,
        /// Reverse key order; valid with sort, jsonl-sort and jsonl-sort-int (JSONL ties stay stable)
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        reverse: bool,
        /// When a pointer is absent, preserve the record during redaction or skip it during JSONL filtering/counting
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        if_present: bool,
        /// RFC 6901 pointer for JSON selection/redaction/filtering/key operations; empty selects root
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        pointer: Option<String>,
        /// Name and JSON pointer for jsonl-project; repeat NAME=/path or NAME= for root
        #[arg(long = "field", requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        fields: Vec<String>,
        /// Explicit JSON literal for an absent projected field; repeat NAME=JSON_LITERAL
        #[arg(long = "field-default", requires = "fields", conflicts_with_all = ["steps", "recipe"])]
        field_defaults: Vec<String>,
        /// Case-sensitive literal substring for lines-include/lines-exclude; no CR/LF
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        contains: Option<String>,
        /// Lines to retain before and after each lines-include match (0-100000)
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        context: Option<usize>,
        /// Exact string for jsonl-where, or JSON integer literal for jsonl-where-int (max4096 bytes)
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        equals: Option<String>,
        /// Inclusive JSON integer lower bound for jsonl-where-int
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe", "equals"])]
        at_least: Option<String>,
        /// Inclusive JSON integer upper bound for jsonl-where-int
        #[arg(long, requires = "operation", conflicts_with_all = ["steps", "recipe", "equals"])]
        at_most: Option<String>,
        /// Number of first/last lines; required for lines-head/lines-tail (1-100000)
        #[arg(short = 'n', long, requires = "operation", conflicts_with_all = ["steps", "recipe"])]
        lines: Option<usize>,
        /// Chain a native operation; repeat in execution order (lines-head:N, lines-tail:N, sort:reverse, json-redact:/path, jsonl-redact:/path, json-select:/path, jsonl-select:/path, jsonl-dedupe:/path, jsonl-sort:/path, jsonl-sort-int:/path, jsonl-count-by:/path and line filters are supported)
        #[arg(long = "step", conflicts_with = "recipe")]
        steps: Vec<String>,
        /// Read a versioned TOML recipe file or a qualified builtin:NAME reference
        #[arg(long)]
        recipe: Option<PathBuf>,
        /// Validate and export a reusable TOML plan without reading data or running steps
        #[arg(long)]
        show_plan: bool,
        /// List embedded native recipes offline, without reading stdin or model settings
        #[arg(long, conflicts_with_all = ["operation", "steps", "recipe", "show_plan", "reverse", "if_present", "pointer", "fields", "field_defaults", "contains", "context", "equals", "at_least", "at_most", "lines"])]
        list_recipes: bool,
    },
    /// List, inspect, render, export, import or initialize reusable task presets (offline)
    Presets {
        #[command(subcommand)]
        action: Option<PresetAction>,
    },
    /// List saved profiles, or explicitly query the selected service for model IDs
    Models {
        /// Contact the selected service; do not change saved configuration
        #[arg(long)]
        remote: bool,
        /// Emit a versioned model catalogue (requires --remote)
        #[arg(long = "json", requires = "remote")]
        json_catalogue: bool,
    },
    /// Choose the default saved model profile
    Use { name: String },
    /// Diagnose configuration; --probe sends a small model request
    Doctor {
        #[arg(long)]
        probe: bool,
        /// Emit one versioned JSON diagnostic report (keys and response bodies hidden)
        #[arg(long = "json")]
        json_report: bool,
    },
    /// Save or import a model configuration (optionally named)
    #[command(
        after_help = "Examples:\n  zi config --name work\n  zi config --list\n  zi config --name work --manual\n  zi config --name work --max-output-tokens 512\n  zi config --name work --api-key-env WORK_API_KEY\n  zi config --rename work personal\n  zi config --name personal --clear-output-budget\n\nWithout maintenance flags, setup uses an interactive terminal; --manual skips detection.\nBudget, key-reference and profile maintenance edit existing settings offline.\n--api-key-env stores a variable name, not its value; set that variable separately.\nTask input/output options do not apply to this command."
    )]
    Config {
        /// Save/edit this named profile, preserving all other models
        #[arg(long, help_heading = "Models and service")]
        name: Option<String>,
        /// Skip detection and enter settings manually
        #[arg(long, help_heading = "Setup and import")]
        manual: bool,
        /// List detected model configurations without importing or showing keys
        #[arg(long, conflicts_with_all = ["manual", "name"], help_heading = "Setup and import")]
        list: bool,
        /// Save a per-turn token cap for an existing profile, without interactive setup
        #[arg(long = "max-output-tokens", value_parser = clap::value_parser!(u32).range(1..=65536), conflicts_with_all = ["manual", "list"], help_heading = "Saved request preferences")]
        request_limit: Option<u32>,
        /// Save the Chat budget field for an existing profile
        #[arg(long = "chat-token-limit-field", value_parser = ["max_completion_tokens", "max_tokens"], conflicts_with_all = ["manual", "list"], help_heading = "Saved request preferences")]
        chat_limit_field: Option<String>,
        /// Clear both saved output-budget preferences; keep model/key/default selection
        #[arg(long, conflicts_with_all = ["manual", "list", "request_limit", "chat_limit_field"], help_heading = "Saved request preferences")]
        clear_output_budget: bool,
        /// Rename a saved named profile; update the saved selection if necessary
        #[arg(long, num_args = 2, value_names = ["OLD", "NEW"], conflicts_with_all = ["name", "manual", "list", "request_limit", "chat_limit_field", "clear_output_budget", "remove"], help_heading = "Profile maintenance")]
        rename: Vec<String>,
        /// Remove an inactive named profile; default and saved active profiles are protected
        #[arg(long, value_name = "NAME", conflicts_with_all = ["name", "manual", "list", "request_limit", "chat_limit_field", "clear_output_budget"], help_heading = "Profile maintenance")]
        remove: Option<String>,
        /// Set the API-key environment reference of an existing profile, offline
        #[arg(long = "api-key-env", value_name = "NAME", value_parser = config::parse_key_env, conflicts_with_all = ["manual", "list", "request_limit", "chat_limit_field", "clear_output_budget", "rename", "remove"], help_heading = "Saved credentials")]
        key_env: Option<String>,
        /// Remove only the saved literal key; keep environment references and other settings
        #[arg(long, conflicts_with_all = ["manual", "list", "request_limit", "chat_limit_field", "clear_output_budget", "rename", "remove"], help_heading = "Saved credentials")]
        clear_saved_key: bool,
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
    /// Import a version-1 TOML library into the selected user preset file, offline
    Import {
        /// Explicit source file (not stdin); existing destination presets are merged
        source: PathBuf,
        /// Allow replacement of matching user names; builtins cannot be replaced
        #[arg(long)]
        replace: bool,
        /// Validate and list additions/replacements without creating files or directories
        #[arg(long)]
        dry_run: bool,
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
    let doctor_probe = matches!(
        cli.command.as_ref(),
        Some(Commands::Doctor { probe: true, .. })
    );
    let management_or_local =
        cli.command.is_some() || cli.count || cli.select.is_some() || cli.config_example;
    if cli.max_output_tokens.is_some() && management_or_local {
        bail!(
            "--max-output-tokens is for model tasks only; doctor --probe uses a fixed 16-token cap"
        );
    }
    if cli.chat_token_limit_field.is_some() && management_or_local && !doctor_probe {
        bail!("--chat-token-limit-field is for model tasks or doctor --probe only");
    }
    if cli.no_profile_budget
        && management_or_local
        && !matches!(cli.command.as_ref(), Some(Commands::Doctor { .. }))
    {
        bail!("--no-profile-budget is for model tasks or doctor only");
    }
    if cli.preview && cli.command.is_some() {
        bail!("--preview is for model tasks, not offline or management commands");
    }
    if cli.exact && cli.command.is_some() {
        bail!("--exact is for model data tasks, not offline or management commands");
    }
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
        if_present,
        pointer,
        fields,
        field_defaults,
        contains,
        context,
        equals,
        at_least,
        at_most,
        lines,
        steps,
        recipe,
        show_plan,
        list_recipes,
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
        if *list_recipes && cli.input.is_some() {
            bail!("--list-recipes does not read data; omit --input to list embedded recipes");
        }
        if *show_plan && cli.input.is_some() {
            bail!("--show-plan does not read data; omit --input to preview or export the plan");
        }
        let single = operation
            .map(|operation| -> Result<transforms::Step> {
                Ok(transforms::Step {
                    if_present: (*if_present).then_some(true),
                    operation,
                    reverse: *reverse,
                    pointer: pointer.clone(),
                    fields: projection::parse(fields, field_defaults)?,
                    contains: contains.clone(),
                    context: *context,
                    equals: equals.clone(),
                    at_least: at_least.clone(),
                    at_most: at_most.clone(),
                    lines: *lines,
                })
            })
            .transpose()?;
        let plan = if *list_recipes {
            None
        } else {
            Some(recipes::Plan::prepare(single, steps, recipe.as_deref())?)
        };
        let target = prepare_output(&cli)?;
        let output = match plan {
            Some(plan) if *show_plan => plan.render()?,
            Some(plan) => plan.run(&read_input(&cli)?)?,
            None => recipes::list(),
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
    if project::enabled(&cli) && !cli.preview {
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
            Some(PresetAction::Import {
                source,
                replace,
                dry_run,
            }) => presets::import(&cli, source, *replace, *dry_run)?,
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
                Commands::Models {
                    remote,
                    json_catalogue,
                } => {
                    if *remote {
                        models::run(&cli, *json_catalogue)
                    } else {
                        config::models(&cli)
                    }
                }
                Commands::Use { name } => config::use_profile(&cli, name),
                Commands::Doctor { probe, json_report } => {
                    if *json_report {
                        doctor::run_json(&cli, *probe)
                    } else {
                        doctor::run(&cli, *probe)
                    }
                }
                _ => unreachable!(),
            };
        }
    }
    if let Some(Commands::Config {
        manual,
        list,
        name,
        request_limit,
        chat_limit_field,
        clear_output_budget,
        rename,
        remove,
        key_env,
        clear_saved_key,
    }) = &cli.command
    {
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
        if key_env.is_some() || *clear_saved_key {
            return config::set_key_settings(
                &cli,
                name.as_deref(),
                key_env.as_deref(),
                *clear_saved_key,
            );
        }
        if !rename.is_empty() {
            return config::rename_profile(&cli, &rename[0], &rename[1]);
        }
        if let Some(name) = remove {
            return config::remove_profile(&cli, name);
        }
        if request_limit.is_some() || chat_limit_field.is_some() || *clear_output_budget {
            return config::set_output_budget(
                &cli,
                name.as_deref(),
                *request_limit,
                chat_limit_field.as_deref(),
                *clear_output_budget,
            );
        }
        if *list {
            return config::list_imports();
        }
        return config::configure_named(&cli, *manual, name.as_deref());
    }
    presets::apply(&mut cli)?;
    if cli.export_plan && cli.json {
        bail!("--export-plan produces TOML and cannot use JSON output or a JSON preset");
    }
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
    if cli.preview && cli.prompt.is_empty() {
        bail!(
            "--preview requires an instruction or a preset; it does not start an interactive session"
        );
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
    // A saved budget can constrain requests even without CLI budget flags.
    // Resolve the one effective snapshot before reading task data or preparing output.
    let model_settings = if cli.count || cli.select.is_some() {
        None
    } else {
        if !cli.preview
            && io::stdin().is_terminal()
            && io::stdout().is_terminal()
            && config::needs_setup(&cli)
        {
            // Preserve the first-launch output guard before asking for credentials.
            let _ = prepare_output(&cli)?;
            config::configure(&cli, false)?;
        }
        Some(config::Settings::load(&cli)?)
    };
    let target = prepare_output(&cli)?;
    let input = read_input(&cli)?;
    let output = if cli.count {
        serde_json::to_string(&data::counts(&input))?
    } else if let Some(pointer) = &cli.select {
        data::select(&input, pointer, cli.join.as_deref())?
    } else {
        let settings = model_settings.context("model configuration unavailable")?;
        if cli.preview {
            let preview = agent::preview(&settings, &cli, &input)?;
            let preview = if target.is_none() && io::stdout().is_terminal() {
                ui::json_display_with_limit(&preview, transforms::CLI_OUTPUT).context(
                    "terminal request preview exceeds 16 MiB after JSON display escaping",
                )?
            } else {
                preview
            };
            return if let Some(target) = target {
                save_output(&preview, target, &cli, true)
            } else {
                write_output(&preview)
            };
        }
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
        return save_output(
            &output,
            target,
            &cli,
            !output.ends_with('\n') && (!cli.exact || !output.is_empty()),
        );
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
            ui::write_status(format_args!(
                "  {} {} ({} bytes)",
                console::style("Saved").green().bold(),
                ui::clean(&path.display().to_string()),
                bytes
            ));
        }
    }
    Ok(())
}

fn emit(output: &str, cli: &Cli) -> Result<()> {
    if cli.exact && output.is_empty() {
        return Ok(());
    }
    if ui::enabled(cli) && !cli.exact && !cli.count && cli.select.is_none() {
        ui::result(output, cli)
    } else if !cli.count && cli.select.is_none() && io::stdout().is_terminal() {
        let shown = if cli.json {
            ui::json_display(output)
        } else {
            ui::clean(output)
        };
        write_output(&shown)
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
    let mut last_result = last_result::LastResult::default();
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
                    "Ask a question, or use /presets, /run NAME [instruction], /models, /use NAME, /doctor, /config, /save PATH, /save! PATH, /clear, /exit. Each prompt is an independent task; no chat history is sent or saved. /save exports only the latest completed result; /save! permits replacement. New tasks, model changes and /clear discard that result.\nFor files/pipes: cat file.txt | zi 'summarize' or zi -i file.txt 'summarize'."
                );
                continue;
            }
            "/save" | "/save!" => {
                eprintln!(
                    "Usage: /save PATH (new file), /save! PATH (explicit replacement). Use the unquoted path; spaces are literal."
                );
                continue;
            }
            _ if prompt.starts_with("/save ") || prompt.starts_with("/save! ") => {
                let (force, path) = if let Some(path) = prompt.strip_prefix("/save! ") {
                    (true, path.trim())
                } else {
                    (false, prompt[6..].trim())
                };
                match last_result.save(std::path::Path::new(path), force) {
                    Ok(bytes) => eprintln!("Saved {} ({} bytes).", ui::clean(path), bytes),
                    Err(error) => eprintln!(
                        "zi: {}",
                        ui::clean(&format!("{error:#}")).replace("use --force", "use /save! PATH")
                    ),
                }
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
                let name = prompt[5..].trim();
                let mut next_cli = cli.clone();
                next_cli.profile = Some(name.into());
                // Validate effective protocol/budget/key settings before saving
                // the selection or discarding a usable completed result.
                match config::Settings::load(&next_cli).and_then(|next_settings| {
                    config::use_profile(cli, name)?;
                    Ok(next_settings)
                }) {
                    Ok(next_settings) => {
                        last_result.clear();
                        cli.profile = next_cli.profile;
                        settings = next_settings;
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
                last_result.clear();
                config::configure(cli, false)?;
                settings = config::Settings::load(cli)?;
                ui::model(&settings, cli);
                continue;
            }
            "/clear" => {
                last_result.clear();
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
        last_result.clear();
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
        if task.export_plan && task.json {
            eprintln!("zi: --export-plan produces TOML and cannot use a JSON preset");
            continue;
        }
        match agent::run(&settings, &task, "") {
            Ok(answer) => {
                let output = if task.json {
                    match transforms::compact_json(&answer.text, data::RESULT_LIMIT) {
                        Ok(value) => value,
                        Err(_) => {
                            eprintln!("zi: output is not valid JSON (nothing written to stdout)");
                            continue;
                        }
                    }
                } else {
                    answer.text
                };
                if !answer.streamed {
                    emit(&output, &task)?;
                }
                if let Err(error) = last_result.retain(output) {
                    eprintln!("zi: {}", ui::clean(&error.to_string()));
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
        ui::write_status(format_args!("zi: {}", ui::clean(&format!("{error:#}"))));
        std::process::exit(1);
    }
}
