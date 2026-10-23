use crate::Cli;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::PathBuf;

pub const EXAMPLE: &str = r#"# Save as ~/.config/zistral/config.toml (Windows: %APPDATA%\zistral\config.toml)
# CLI flags > ZI_* environment variables > this file > defaults.
model = "your-model-id"
base_url = "https://api.openai.com/v1"
# chat: OpenAI-compatible chat completions; responses: OpenAI Responses API
api = "chat"
# Store credentials in the environment, not this file.
api_key_env = "OPENAI_API_KEY"

# Local server example:
# base_url = "http://localhost:11434/v1"
# model = "your-local-model"
"#;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    model: Option<String>,
    base_url: Option<String>,
    api: Option<String>,
    api_key_env: Option<String>,
}

pub struct Settings {
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub api_key: Option<String>,
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

impl Settings {
    pub fn load(cli: &Cli) -> Result<Self> {
        let path = cli.config.clone().or_else(config_path);
        let file: FileConfig = match path {
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(text) => toml::from_str(&text)
                    .with_context(|| format!("invalid configuration {}", p.display()))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && cli.config.is_none() => {
                    FileConfig::default()
                }
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("cannot read configuration {}", p.display()));
                }
            },
            None => FileConfig::default(),
        };
        let model = nonempty(cli.model.clone().or(file.model))
            .context("set ZI_MODEL, --model, or model in config; use zi --config-example")?;
        let base_url = cli
            .base_url
            .clone()
            .or(file.base_url)
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        let url = reqwest::Url::parse(&base_url).context("invalid API base URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("API base URL must be an http(s) URL without credentials, query, or fragment");
        }
        let api = cli
            .api
            .clone()
            .or(file.api)
            .unwrap_or_else(|| "chat".into());
        if !matches!(api.as_str(), "chat" | "responses") {
            bail!("api must be chat or responses");
        }
        let key_env = file.api_key_env.unwrap_or_else(|| "OPENAI_API_KEY".into());
        let api_key = nonempty(std::env::var("ZI_API_KEY").ok())
            .or_else(|| nonempty(std::env::var(&key_env).ok()));
        if url.host_str() == Some("api.openai.com") && api_key.is_none() {
            bail!("set ZI_API_KEY or {key_env}; local servers may run without a key");
        }
        if url.scheme() == "http"
            && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            bail!("use HTTPS for remote API endpoints; HTTP is supported only on loopback");
        }
        Ok(Self {
            model,
            base_url: base_url.trim_end_matches('/').into(),
            api,
            api_key,
        })
    }
}
