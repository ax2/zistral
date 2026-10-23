use crate::Cli;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
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

# Local server example:
# base_url = "http://localhost:11434/v1"
# model = "your-local-model"
"#;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    model: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key_env: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
}

pub struct Settings {
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
            if candidate.api_key.is_some() {
                "found"
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
    let mut key = candidate.api_key;
    while key.is_none() {
        let answer = ask("API Key [hidden; Enter for no key]", true)?;
        key = nonempty(Some(answer.trim().to_owned()));
        if key.is_some()
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
    Ok(Some(FileConfig {
        model: Some(candidate.model),
        base_url: Some(candidate.base_url),
        api: Some(candidate.api),
        api_key: key,
        api_key_env: None,
        endpoint: candidate.endpoint,
    }))
}

pub fn configure(cli: &Cli, manual: bool) -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("zi config requires an interactive terminal; run it without piped input");
    }
    let path = cli
        .config
        .clone()
        .or_else(config_path)
        .context("cannot locate config directory; use zi config --config PATH")?;
    let file = read_config(&path, true)?;
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
    save_config(&path, &updated)?;
    writeln!(
        output,
        "Saved {}. You can now run zi with an instruction.",
        path.display()
    )?;
    let overrides: Vec<_> = [
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
    let openai = validate_url(&base_url)?.host_str() == Some("api.openai.com");
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
    loop {
        let label = if nonempty(file.api_key.clone()).is_some() {
            "API Key [hidden; Enter keeps saved key, '-' clears]"
        } else {
            "API Key [hidden; optional for local services]"
        };
        let answer = ask(label, true)?;
        let key = match answer.trim() {
            "" => nonempty(file.api_key.clone()),
            "-" => None,
            key => Some(key.to_owned()),
        };
        if openai && key.is_none() {
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

impl Settings {
    pub fn load(cli: &Cli) -> Result<Self> {
        let path = cli.config.clone().or_else(config_path);
        let file: FileConfig = match path {
            Some(p) => read_config(&p, cli.config.is_none())?,
            None => FileConfig::default(),
        };
        let model = nonempty(cli.model.clone().or(file.model))
            .context("run zi config first, or set ZI_MODEL / --model")?;
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
        let key_env = file.api_key_env.unwrap_or_else(|| "OPENAI_API_KEY".into());
        let api_key = nonempty(std::env::var("ZI_API_KEY").ok())
            .or_else(|| nonempty(std::env::var(&key_env).ok()))
            .or_else(|| nonempty(file.api_key));
        if url.host_str() == Some("api.openai.com") && api_key.is_none() {
            bail!("set ZI_API_KEY or {key_env}; local servers may run without a key");
        }
        let endpoint = if cli.base_url.is_none() && cli.api.is_none() {
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
