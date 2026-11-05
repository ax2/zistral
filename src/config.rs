use crate::Cli;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

pub const EXAMPLE: &str = r#"# Save as ~/.config/zistral/config.toml (Windows: %APPDATA%\zistral\config.toml)
# CLI flags > ZI_* environment variables > this file > defaults.
model = "your-model-id"
base_url = "https://api.openai.com/v1"
# chat: OpenAI-compatible chat completions; responses: OpenAI Responses API
api = "chat"
# Run zi config to save a key locally, or keep using an environment variable.
api_key_env = "OPENAI_API_KEY"

# Optional saved request budgets (unreleased main / next grouped release):
# max_output_tokens = 512
# chat_token_limit_field = "max_tokens" # Chat only; requires a cap
# zi config --clear-output-budget removes both preferences.

# Local server example:
# base_url = "http://localhost:11434/v1"
# model = "your-local-model"
"#;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    model: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key_env: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    max_output_tokens: Option<u32>,
    chat_token_limit_field: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profiles: BTreeMap<String, Profile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_profile: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    model: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key_env: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    max_output_tokens: Option<u32>,
    chat_token_limit_field: Option<String>,
}

impl From<&FileConfig> for Profile {
    fn from(f: &FileConfig) -> Self {
        Self {
            model: f.model.clone(),
            base_url: f.base_url.clone(),
            api: f.api.clone(),
            api_key_env: f.api_key_env.clone(),
            api_key: f.api_key.clone(),
            endpoint: f.endpoint.clone(),
            max_output_tokens: f.max_output_tokens,
            chat_token_limit_field: f.chat_token_limit_field.clone(),
        }
    }
}

impl From<Profile> for FileConfig {
    fn from(p: Profile) -> Self {
        Self {
            model: p.model,
            base_url: p.base_url,
            api: p.api,
            api_key_env: p.api_key_env,
            api_key: p.api_key,
            endpoint: p.endpoint,
            max_output_tokens: p.max_output_tokens,
            chat_token_limit_field: p.chat_token_limit_field,
            ..Self::default()
        }
    }
}

pub fn path(cli: &Cli) -> Result<PathBuf> {
    cli.config
        .clone()
        .or_else(config_path)
        .context("cannot locate config directory; use --config PATH")
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        bail!("profile name must be 1-64 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

fn selected(document: &FileConfig, name: &str) -> Result<FileConfig> {
    validate_name(name)?;
    if name == "default" {
        return Ok(Profile::from(document).into());
    }
    Ok(document
        .profiles
        .get(name)
        .with_context(|| format!("unknown profile {name}; run zi models"))?
        .clone()
        .into())
}

pub fn models(cli: &Cli) -> Result<()> {
    let document = read_config(&path(cli)?, cli.config.is_none())?;
    let active = cli
        .profile
        .as_deref()
        .or(document.active_profile.as_deref())
        .unwrap_or("default");
    let mut profiles = document.profiles.clone();
    if document.model.is_some() {
        profiles.insert("default".into(), Profile::from(&document));
    }
    if profiles.is_empty() {
        bail!("no saved models; run zi config --name NAME");
    }
    let mut output = io::stdout().lock();
    for (name, p) in profiles {
        writeln!(
            output,
            "{} {}  {} [{}; Key {}]",
            if name == active { "*" } else { " " },
            crate::ui::clean(&name),
            crate::ui::clean(p.model.as_deref().unwrap_or("(unset)")),
            crate::ui::clean(p.api.as_deref().unwrap_or("chat")),
            if nonempty(p.api_key).is_some() || p.api_key_env.is_some() {
                "saved/reference"
            } else {
                "not saved"
            }
        )?;
    }
    Ok(())
}

pub fn use_profile(cli: &Cli, name: &str) -> Result<()> {
    let path = path(cli)?;
    let mut document = read_config(&path, false)?;
    let file = selected(&document, name)?;
    if nonempty(file.model.clone()).is_none() {
        bail!("profile has no model; run zi config --name {name}");
    }
    profile_budget(&file)?;
    document.active_profile = if name == "default" {
        None
    } else {
        Some(name.into())
    };
    save_config(&path, &document)?;
    eprintln!(
        "Active profile: {name}. ZI_PROFILE / --profile and other overrides still take precedence."
    );
    Ok(())
}

/// Metadata changes deliberately do not validate generation settings: an obsolete
/// or misconfigured profile must remain renameable/removable without a service.
pub fn rename_profile(cli: &Cli, old: &str, new: &str) -> Result<()> {
    validate_name(old)?;
    validate_name(new)?;
    if old == "default" || new == "default" {
        bail!("default is reserved for root settings and cannot be renamed or replaced");
    }
    if old == new {
        bail!("new profile name must differ from the old name; configuration unchanged");
    }
    let path = path(cli)?;
    let mut document = read_config(&path, false)?;
    if document.profiles.contains_key(new) {
        bail!("target profile already exists; choose another name");
    }
    let profile = document
        .profiles
        .remove(old)
        .context("unknown saved profile; run zi models")?;
    document.profiles.insert(new.into(), profile);
    let active = document.active_profile.as_deref() == Some(old);
    if active {
        document.active_profile = Some(new.into());
    }
    save_config(&path, &document)?;
    eprintln!(
        "Renamed profile {old} to {new}. Saved default selection {}. External references are unchanged.",
        if active { "updated" } else { "unchanged" }
    );
    Ok(())
}

pub fn remove_profile(cli: &Cli, name: &str) -> Result<()> {
    validate_name(name)?;
    if name == "default" {
        bail!("default root settings cannot be removed; choose a named profile");
    }
    let path = path(cli)?;
    let mut document = read_config(&path, false)?;
    if document.active_profile.as_deref() == Some(name) {
        bail!("cannot remove the saved active profile; run zi use NAME to select another first");
    }
    document
        .profiles
        .remove(name)
        .context("unknown saved profile; run zi models")?;
    save_config(&path, &document)?;
    eprintln!(
        "Removed profile {name}. Other profiles and the saved default selection are unchanged."
    );
    Ok(())
}

pub enum ApiKeySource {
    Environment(String),
    SavedProfile,
    NotConfigured,
}

pub struct Settings {
    pub profile_name: String,
    pub api_key_source: ApiKeySource,
    pub output_budget: OutputBudget,
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
    .map(|p| p.join("zistral").join("config.toml"))
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

// Share the effective key decision with setup: a usable environment source
// satisfies required-key services without being copied into the saved literal.
fn resolve_api_key(saved: Option<&str>, key_env: &str) -> (Option<String>, ApiKeySource) {
    if let Some(key) = nonempty(std::env::var("ZI_API_KEY").ok()) {
        (Some(key), ApiKeySource::Environment("ZI_API_KEY".into()))
    } else if let Some(key) = nonempty(std::env::var(key_env).ok()) {
        (Some(key), ApiKeySource::Environment(key_env.into()))
    } else if let Some(key) = saved.filter(|key| !key.trim().is_empty()) {
        (Some(key.into()), ApiKeySource::SavedProfile)
    } else {
        (None, ApiKeySource::NotConfigured)
    }
}

fn read_config(path: &Path, allow_missing: bool) -> Result<FileConfig> {
    match std::fs::read_to_string(path) {
        // Parser diagnostics include source lines, which may contain an API key.
        Ok(text) => toml::from_str(&text).map_err(|_| {
            anyhow!(
                "invalid configuration {}; check TOML syntax and field names",
                path.display()
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound && allow_missing => Ok(FileConfig::default()),
        Err(e) => Err(e).with_context(|| format!("cannot read configuration {}", path.display())),
    }
}

pub(crate) fn validate_url(base_url: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(base_url).map_err(|_| anyhow!("invalid API base URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("API base URL must be an http(s) URL without credentials, query, or fragment");
    }
    if url.scheme() == "http"
        && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        bail!("use HTTPS for remote API endpoints; HTTP is supported only on loopback");
    }
    Ok(url)
}

pub fn list_imports() -> Result<()> {
    let found = crate::imports::discover();
    print_imports(&found, &mut io::stdout().lock())
}

fn print_imports(found: &crate::imports::Discovery, output: &mut impl Write) -> Result<()> {
    for note in &found.notes {
        writeln!(output, "{note}")?;
    }
    if found.candidates.is_empty() {
        writeln!(
            output,
            "No compatible model configurations found. Manual setup is available."
        )?;
    }
    for (i, candidate) in found.candidates.iter().enumerate() {
        writeln!(
            output,
            "{}. {}: {} [{}; Key {}]",
            i + 1,
            crate::imports::display(&candidate.source),
            crate::imports::display(&candidate.model),
            candidate.api,
            if candidate.key_available() {
                if candidate.api_key_env.is_some() {
                    "environment"
                } else {
                    "found"
                }
            } else {
                "missing"
            }
        )?;
    }
    Ok(())
}

fn import_config(
    found: crate::imports::Discovery,
    ask: &mut impl FnMut(&str, bool) -> Result<String>,
    output: &mut impl Write,
) -> Result<Option<FileConfig>> {
    print_imports(&found, output)?;
    if found.candidates.is_empty() {
        return Ok(None);
    }
    writeln!(output, "0. Manual setup / keep saved settings")?;
    let index = loop {
        let answer = ask("Configuration source [0]", false)?;
        if answer.trim().is_empty() || answer.trim() == "0" {
            return Ok(None);
        }
        if let Ok(index) = answer.trim().parse::<usize>() {
            if index > 0 && index <= found.candidates.len() {
                break index - 1;
            }
        }
        writeln!(
            output,
            "Choose a number from 0 to {}.",
            found.candidates.len()
        )?;
    };
    let candidate = found
        .candidates
        .into_iter()
        .nth(index)
        .expect("validated selection");
    let key_env = candidate.api_key_env;
    let environment_available = || {
        key_env
            .as_deref()
            .is_some_and(|name| nonempty(std::env::var(name).ok()).is_some())
    };
    let mut key = candidate.api_key;
    while key.is_none() && !environment_available() {
        let answer = ask("API Key [hidden; Enter for no key]", true)?;
        key = nonempty(Some(answer.trim().to_owned()));
        if key.is_some()
            || environment_available()
            || !matches!(
                validate_url(&candidate.base_url)?.host_str(),
                Some("api.openai.com" | "api.anthropic.com")
            )
        {
            break;
        }
        writeln!(output, "API Key is required for this service.")?;
    }
    writeln!(
        output,
        "Imported {}: {} ({}).",
        crate::imports::display(&candidate.source),
        crate::imports::display(&candidate.model),
        candidate.api
    )?;
    if key_env.is_some() {
        writeln!(
            output,
            "API Key environment reference retained; its value is not saved."
        )?;
    }
    Ok(Some(FileConfig {
        model: Some(candidate.model),
        base_url: Some(candidate.base_url),
        api: Some(candidate.api),
        api_key: key,
        api_key_env: key_env,
        endpoint: candidate.endpoint,
        ..FileConfig::default()
    }))
}

pub fn needs_setup(cli: &Cli) -> bool {
    let path = cli.config.clone().or_else(config_path);
    !path.is_some_and(|p| p.exists()) && cli.model.is_none()
}

pub fn configure(cli: &Cli, manual: bool) -> Result<()> {
    configure_named(cli, manual, None)
}

pub fn configure_named(cli: &Cli, manual: bool, name: Option<&str>) -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("zi config requires an interactive terminal; run it without piped input");
    }
    let path = cli
        .config
        .clone()
        .or_else(config_path)
        .context("cannot locate config directory; use zi config --config PATH")?;
    let mut document = read_config(&path, true)?;
    let name = name
        .or(cli.profile.as_deref())
        .or(document.active_profile.as_deref())
        .unwrap_or("default")
        .to_owned();
    validate_name(&name)?;
    let file = if name == "default" || document.profiles.contains_key(&name) {
        selected(&document, &name)?
    } else {
        FileConfig::default()
    };
    if crate::ui::enabled(cli) {
        crate::ui::heading("Model setup · detect → choose → ready");
    }
    let mut output = io::stderr().lock();
    writeln!(
        output,
        "Configure zi. Press Enter to keep the value in brackets; Ctrl+C cancels."
    )?;
    writeln!(output, "Config file: {}", path.display())?;
    let mut ask = |label: &str, secret: bool| -> Result<String> {
        eprint!("{label}: ");
        io::stderr().flush()?;
        if secret {
            rpassword::read_password().context("cannot read API key from terminal")
        } else {
            let mut line = String::new();
            if io::stdin().read_line(&mut line)? == 0 {
                bail!("configuration cancelled; nothing saved");
            }
            Ok(line.trim_end_matches(['\r', '\n']).into())
        }
    };
    let imported = if manual {
        None
    } else {
        import_config(crate::imports::discover(), &mut ask, &mut output)?
    };
    let updated = match imported {
        Some(file) => file,
        None => collect_config(cli, file, &mut ask, &mut output)?,
    };
    profile_budget(&updated)?;
    if name == "default" {
        let profiles = std::mem::take(&mut document.profiles);
        document = updated.clone();
        document.profiles = profiles;
        document.active_profile = None;
    } else {
        if document.profiles.len() >= 128 && !document.profiles.contains_key(&name) {
            bail!("at most 128 profiles are supported");
        }
        document
            .profiles
            .insert(name.clone(), Profile::from(&updated));
        document.active_profile = Some(name.clone());
    }
    save_config(&path, &document)?;
    writeln!(
        output,
        "Saved {}. You can now run zi with an instruction.",
        path.display()
    )?;
    let overrides: Vec<_> = [
        "ZI_PROFILE",
        "ZI_MODEL",
        "ZI_BASE_URL",
        "ZI_API",
        "ZI_API_KEY",
        updated.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY"),
    ]
    .into_iter()
    .filter(|name| std::env::var(name).is_ok_and(|v| !v.trim().is_empty()))
    .collect();
    if !overrides.is_empty() {
        writeln!(
            output,
            "Environment variables still override saved settings: {}",
            overrides.join(", ")
        )?;
    }
    Ok(())
}

fn ask_value(
    label: &str,
    default: &str,
    ask: &mut impl FnMut(&str, bool) -> Result<String>,
    output: &mut impl Write,
    validate: impl Fn(&str) -> Result<()>,
) -> Result<String> {
    loop {
        let answer = ask(&format!("{label} [{default}]"), false)?;
        let value = if answer.trim().is_empty() {
            default
        } else {
            answer.trim()
        };
        match validate(value) {
            Ok(()) => return Ok(value.into()),
            Err(error) => writeln!(output, "{error}")?,
        }
    }
}

fn collect_config(
    cli: &Cli,
    mut file: FileConfig,
    ask: &mut impl FnMut(&str, bool) -> Result<String>,
    output: &mut impl Write,
) -> Result<FileConfig> {
    let base_url = ask_value(
        "Service URL",
        cli.base_url
            .as_deref()
            .or(file.base_url.as_deref())
            .unwrap_or("https://api.openai.com/v1"),
        ask,
        output,
        |v| validate_url(v).map(|_| ()),
    )?;
    let model = ask_value(
        "Model name",
        cli.model.as_deref().or(file.model.as_deref()).unwrap_or(""),
        ask,
        output,
        |v| {
            if v.is_empty() {
                bail!("model name cannot be empty");
            }
            Ok(())
        },
    )?;
    let service = validate_url(&base_url)?;
    let openai = service.host_str() == Some("api.openai.com");
    let key_required = matches!(
        service.host_str(),
        Some("api.openai.com" | "api.anthropic.com")
    );
    let api = ask_value(
        "API protocol (chat/responses/anthropic)",
        cli.api
            .as_deref()
            .or(file.api.as_deref())
            .unwrap_or(if openai { "responses" } else { "chat" }),
        ask,
        output,
        |v| {
            if !matches!(v, "chat" | "responses" | "anthropic") {
                bail!("choose chat or responses or anthropic");
            }
            Ok(())
        },
    )?;
    let key_env = file.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
    let (_, source) = resolve_api_key(file.api_key.as_deref(), key_env);
    let environment = matches!(source, ApiKeySource::Environment(_));
    if let ApiKeySource::Environment(name) = source {
        writeln!(
            output,
            "Environment credential {} takes precedence over a saved key; its value is not saved.",
            serde_json::to_string(&name)?
        )?;
    }
    loop {
        let label = if environment {
            "API Key [hidden; Enter uses environment, '-' clears saved key]"
        } else if nonempty(file.api_key.clone()).is_some() {
            "API Key [hidden; Enter keeps saved key, '-' clears saved key]"
        } else {
            "API Key [hidden; optional for local services]"
        };
        let answer = ask(label, true)?;
        let key = match answer.trim() {
            "" => nonempty(file.api_key.clone()),
            "-" => None,
            key => Some(key.to_owned()),
        };
        if key_required && resolve_api_key(key.as_deref(), key_env).0.is_none() {
            writeln!(output, "API Key is required for this service.")?;
            continue;
        }
        file.api_key = key;
        break;
    }
    if file.base_url.as_deref() != Some(base_url.trim_end_matches('/'))
        || file.api.as_deref() != Some(api.as_str())
    {
        file.endpoint = None;
    }
    file.base_url = Some(base_url.trim_end_matches('/').into());
    file.model = Some(model);
    file.api = Some(api);
    Ok(file)
}

fn save_config(path: &Path, file: &FileConfig) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("cannot create config directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("cannot create config file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let text = toml::to_string_pretty(file).context("cannot serialize configuration")?;
    temporary
        .write_all(text.as_bytes())
        .context("cannot write configuration")?;
    temporary
        .as_file()
        .sync_all()
        .context("cannot flush configuration")?;
    temporary
        .persist(path)
        .map_err(|e| e.error)
        .context("cannot replace configuration; original file preserved")?;
    Ok(())
}

#[derive(Clone, Copy)]
pub struct OutputBudget {
    pub tokens: Option<u32>,
    pub field: &'static str,
    pub tokens_source: &'static str,
    pub field_source: &'static str,
}

impl OutputBudget {
    pub fn probe(self, api: &str) -> Self {
        let mut budget = self;
        // Preserve older Chat probes only when there is no explicit preference.
        if api == "chat" && self.tokens_source == "default" && self.field_source == "default" {
            budget.field = "max_tokens";
            budget.field_source = "probe default";
        }
        budget.tokens = Some(16);
        budget.tokens_source = "probe";
        budget
    }
}

fn resolve_output_budget(
    api: &str,
    tokens: Option<u32>,
    field: Option<&str>,
    tokens_source: &'static str,
    field_source: &'static str,
    probe: bool,
) -> Result<OutputBudget> {
    if tokens.is_some_and(|n| !(1..=65536).contains(&n)) {
        bail!("max_output_tokens / --max-output-tokens must be between 1 and 65536");
    }
    if field.is_some() && api != "chat" {
        bail!("chat_token_limit_field / --chat-token-limit-field requires the chat protocol");
    }
    if field.is_some_and(|f| !matches!(f, "max_completion_tokens" | "max_tokens")) {
        bail!("chat_token_limit_field must be max_completion_tokens or max_tokens");
    }
    if field.is_some() && tokens.is_none() && !probe {
        bail!(
            "--chat-token-limit-field requires --max-output-tokens or a saved output cap (except doctor --probe)"
        );
    }
    if api == "responses" && tokens.is_some_and(|n| n < 16) {
        bail!("--max-output-tokens must be at least 16 for responses");
    }
    Ok(OutputBudget {
        tokens: tokens.or((api == "anthropic").then_some(4096)),
        field: match api {
            "responses" => "max_output_tokens",
            "anthropic" => "max_tokens",
            _ if field == Some("max_tokens") => "max_tokens",
            _ => "max_completion_tokens",
        },
        tokens_source: if tokens.is_some() {
            tokens_source
        } else {
            "default"
        },
        field_source: if field.is_some() {
            field_source
        } else {
            "default"
        },
    })
}

fn profile_budget(file: &FileConfig) -> Result<OutputBudget> {
    let api = file.api.as_deref().unwrap_or("chat");
    if !matches!(api, "chat" | "responses" | "anthropic") {
        bail!("api must be chat or responses or anthropic");
    }
    resolve_output_budget(
        api,
        file.max_output_tokens,
        file.chat_token_limit_field.as_deref(),
        "profile",
        "profile",
        false,
    )
}

pub fn set_output_budget(
    cli: &Cli,
    name: Option<&str>,
    tokens: Option<u32>,
    field: Option<&str>,
    clear: bool,
) -> Result<()> {
    let path = path(cli)?;
    let mut document = read_config(&path, false)?;
    let name = name
        .or(cli.profile.as_deref())
        .or(document.active_profile.as_deref())
        .unwrap_or("default")
        .to_owned();
    let mut updated = selected(&document, &name)?;
    if nonempty(updated.model.clone()).is_none() {
        bail!("profile has no saved model; run zi config first");
    }
    if clear {
        updated.max_output_tokens = None;
        updated.chat_token_limit_field = None;
    } else {
        if let Some(tokens) = tokens {
            updated.max_output_tokens = Some(tokens);
        }
        if let Some(field) = field {
            updated.chat_token_limit_field = Some(field.into());
        }
    }
    profile_budget(&updated)?;
    if name == "default" {
        document.max_output_tokens = updated.max_output_tokens;
        document.chat_token_limit_field = updated.chat_token_limit_field;
    } else {
        document
            .profiles
            .insert(name.clone(), Profile::from(&updated));
    }
    save_config(&path, &document)?;
    eprintln!("Saved request budget for profile {name}. Default profile selection unchanged.");
    Ok(())
}

impl Settings {
    pub fn load(cli: &Cli) -> Result<Self> {
        Self::load_inner(cli, false)
    }

    // The catalogue uses the same profile, URL, protocol and credential selection,
    // but does not generate text: neither a model ID nor generation budgets apply.
    pub fn load_for_models(cli: &Cli) -> Result<Self> {
        Self::load_inner(cli, true)
    }

    fn load_inner(cli: &Cli, catalogue: bool) -> Result<Self> {
        let path = cli.config.clone().or_else(config_path);
        let file: FileConfig = match path {
            Some(p) => read_config(&p, cli.config.is_none())?,
            None => FileConfig::default(),
        };
        let name = cli
            .profile
            .as_deref()
            .or(file.active_profile.as_deref())
            .unwrap_or("default")
            .to_owned();
        let file = selected(&file, &name)?;
        let model = nonempty(cli.model.clone().or(file.model));
        let model = if catalogue {
            model.unwrap_or_default()
        } else {
            model.context("run zi config first, or set ZI_MODEL / --model")?
        };
        let base_url = cli
            .base_url
            .clone()
            .or(file.base_url)
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        let url = validate_url(&base_url)?;
        let api = cli
            .api
            .clone()
            .or(file.api)
            .unwrap_or_else(|| "chat".into());
        if !matches!(api.as_str(), "chat" | "responses" | "anthropic") {
            bail!("api must be chat or responses or anthropic");
        }
        let profile_tokens = if catalogue || cli.no_profile_budget {
            None
        } else {
            file.max_output_tokens
        };
        let profile_field = if catalogue || cli.no_profile_budget {
            None
        } else {
            file.chat_token_limit_field.as_deref()
        };
        let tokens = cli.max_output_tokens.or(profile_tokens);
        let field = cli.chat_token_limit_field.as_deref().or(profile_field);
        let output_budget = resolve_output_budget(
            &api,
            tokens,
            field,
            if cli.max_output_tokens.is_some() {
                "cli"
            } else {
                "profile"
            },
            if cli.chat_token_limit_field.is_some() {
                "cli"
            } else {
                "profile"
            },
            cli.chat_token_limit_field.is_some()
                && matches!(
                    cli.command.as_ref(),
                    Some(crate::Commands::Doctor { probe: true, .. })
                ),
        )?;
        let key_env = file.api_key_env.unwrap_or_else(|| "OPENAI_API_KEY".into());
        // Keep provenance beside the value selected by the actual request path.
        // Do not infer it later from a second config/environment read.
        let (api_key, api_key_source) = resolve_api_key(file.api_key.as_deref(), &key_env);
        if matches!(url.host_str(), Some("api.openai.com" | "api.anthropic.com"))
            && api_key.is_none()
            && !cli.preview
        {
            bail!("set ZI_API_KEY or {key_env}; local servers may run without a key");
        }
        let endpoint = if !catalogue && cli.base_url.is_none() && cli.api.is_none() {
            file.endpoint
        } else {
            None
        };
        if let Some(endpoint) = &endpoint {
            let exact = validate_url(endpoint)?;
            if url.origin() != exact.origin() {
                bail!("configured endpoint must have the same origin as base_url");
            }
        }
        Ok(Self {
            profile_name: name,
            api_key_source,
            output_budget,
            model,
            base_url: base_url.trim_end_matches('/').into(),
            api,
            api_key,
            endpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn collect(file: FileConfig, answers: &[&str]) -> (FileConfig, String, Vec<(String, bool)>) {
        let cli = Cli::parse_from(["zi", "config"]);
        let mut answers = answers.iter();
        let mut prompts = Vec::new();
        let mut ask = |label: &str, secret: bool| {
            prompts.push((label.to_owned(), secret));
            Ok(answers.next().expect("missing answer").to_string())
        };
        let mut output = Vec::new();
        let file = collect_config(&cli, file, &mut ask, &mut output).unwrap();
        (file, String::from_utf8(output).unwrap(), prompts)
    }

    #[test]
    fn new_configuration_roundtrip_and_hidden_key_prompt() {
        let (file, _, prompts) = collect(
            FileConfig::default(),
            &[
                "https://service.example/v1/",
                "my-model",
                "chat",
                "test-secret",
            ],
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        save_config(&path, &file).unwrap();
        let saved = read_config(&path, false).unwrap();
        assert_eq!(
            saved.base_url.as_deref(),
            Some("https://service.example/v1")
        );
        assert_eq!(saved.model.as_deref(), Some("my-model"));
        assert_eq!(saved.api_key.as_deref(), Some("test-secret"));
        assert!(prompts.last().unwrap().1);
        assert!(
            !prompts
                .iter()
                .any(|(label, _)| label.contains("test-secret"))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Updating an existing configuration must work on Windows as well.
        save_config(
            &path,
            &FileConfig {
                model: Some("new-model".into()),
                ..file
            },
        )
        .unwrap();
        assert_eq!(
            read_config(&path, false).unwrap().model.as_deref(),
            Some("new-model")
        );
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn existing_defaults_keep_key_and_environment_reference() {
        let file = FileConfig {
            base_url: Some("https://example.com/v1".into()),
            model: Some("existing".into()),
            api: Some("chat".into()),
            api_key: Some("test-secret".into()),
            api_key_env: Some("CUSTOM_API_KEY".into()),
            endpoint: None,
            ..FileConfig::default()
        };
        let (updated, _, prompts) = collect(file, &["", "", "", ""]);
        assert_eq!(updated.model.as_deref(), Some("existing"));
        assert_eq!(updated.api_key.as_deref(), Some("test-secret"));
        assert_eq!(updated.api_key_env.as_deref(), Some("CUSTOM_API_KEY"));
        assert!(!format!("{prompts:?}").contains("test-secret"));
    }

    #[test]
    fn invalid_values_retry_and_local_key_can_be_cleared() {
        let file = FileConfig {
            api_key: Some("test-secret".into()),
            ..FileConfig::default()
        };
        let (file, output, _) = collect(
            file,
            &[
                "http://remote.example/v1",
                "http://localhost:11434/v1",
                "",
                "local-model",
                "wrong",
                "chat",
                "-",
            ],
        );
        assert!(output.contains("HTTPS"));
        assert!(output.contains("cannot be empty"));
        assert!(output.contains("chat or responses"));
        assert!(file.api_key.is_none());
    }

    #[test]
    fn cancellation_preserves_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "model = 'keep-me'\napi_key = 'test-secret'\n";
        std::fs::write(&path, original).unwrap();
        let cli = Cli::parse_from(["zi", "config"]);
        let mut ask = |_: &str, _: bool| -> Result<String> { bail!("cancelled") };
        assert!(
            collect_config(
                &cli,
                read_config(&path, false).unwrap(),
                &mut ask,
                &mut Vec::new()
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn invalid_config_error_never_echoes_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "api_key = 'test-secret' invalid\n").unwrap();
        let error = read_config(&path, false).err().unwrap().to_string();
        assert!(error.contains("invalid configuration"));
        assert!(!error.contains("test-secret"));
    }
}
