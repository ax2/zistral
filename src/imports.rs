use anyhow::{Result, bail};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

pub struct Candidate {
    pub source: String,
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub endpoint: Option<String>,
}

impl Candidate {
    pub fn key_available(&self) -> bool {
        self.api_key.is_some()
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|name| env_key(name).is_some())
    }
}

// Preserve the selected credential's kind. Environment values are inspected for
// presence but never retained as candidate literals or written as saved keys.
#[derive(Clone)]
enum Credential {
    Literal(String),
    Environment(String),
}

#[derive(Default)]
pub struct Discovery {
    pub candidates: Vec<Candidate>,
    pub notes: Vec<String>,
}

pub fn discover() -> Discovery {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    };
    match home {
        Some(home) => discover_at(
            &PathBuf::from(home),
            std::env::var_os("CODEX_HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .as_deref(),
            std::env::var_os("CLAUDE_CONFIG_DIR")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .as_deref(),
        ),
        None => Discovery::default(),
    }
}

// These are configuration readers, never plugin loaders or credential commands.
fn discover_at(home: &Path, codex_home: Option<&Path>, claude_home: Option<&Path>) -> Discovery {
    let mut found = Discovery::default();
    read_codex(codex_home.unwrap_or(&home.join(".codex")), &mut found);
    read_workbuddy(
        &home.join(".workbuddy/models.json"),
        "WorkBuddy",
        &mut found,
    );
    read_workbuddy(
        &home.join(".codebuddy/models.json"),
        "WorkBuddy/CodeBuddy",
        &mut found,
    );
    read_dsh(&home.join(".dsh"), &mut found);
    read_claude(
        claude_home.unwrap_or(&home.join(".claude")),
        &|name| std::env::var(name).ok(),
        &mut found,
    );
    found
}

fn text(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(2_097_153)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 2_097_152 {
        bail!("configuration exceeds size limit");
    }
    let text = String::from_utf8(bytes)?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

fn json(path: &Path) -> Option<Value> {
    serde_json::from_str(&text(path).ok()?).ok()
}
fn yaml(path: &Path) -> Option<Value> {
    serde_yaml_ng::from_str(&text(path).ok()?).ok()
}
fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_owned())
}
fn env_key(name: &str) -> Option<Credential> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|_| Credential::Environment(name.to_owned()))
}
fn literal_key(value: Option<String>) -> Option<Credential> {
    let value = value?;
    if let Some(name) = value
        .strip_prefix("${")
        .and_then(|s| s.strip_suffix('}'))
        .or_else(|| value.strip_prefix('$'))
    {
        env_key(name)
    } else {
        Some(Credential::Literal(value))
    }
}
fn api(value: &str) -> Option<&'static str> {
    match value {
        "chat" | "openai" | "openai-completions" | "openai-chat-completions" => Some("chat"),
        "responses" | "openai-responses" => Some("responses"),
        "anthropic" | "anthropic-messages" => Some("anthropic"),
        _ => None,
    }
}

fn candidate(
    source: String,
    model: String,
    url: String,
    protocol: &str,
    key: Option<Credential>,
    exact: bool,
    found: &mut Discovery,
) {
    if found.candidates.len() >= 512 {
        return;
    }
    let Some(protocol) = api(protocol) else {
        found.notes.push(format!(
            "{}: unsupported API protocol; skipped.",
            display(&source)
        ));
        return;
    };
    if crate::config::validate_url(&url).is_err() {
        found.notes.push(format!(
            "{}: invalid or unsupported service URL; skipped.",
            display(&source)
        ));
        return;
    }
    let suffix = match protocol {
        "chat" => "/chat/completions",
        "responses" => "/responses",
        _ => "/messages",
    };
    let trimmed = url.trim_end_matches('/');
    let base_url = trimmed.strip_suffix(suffix).unwrap_or(trimmed).to_owned();
    let endpoint = if exact {
        Some(url.clone())
    } else {
        (protocol == "anthropic" && trimmed.ends_with(suffix)).then(|| trimmed.to_owned())
    };
    let (api_key, api_key_env) = match key {
        Some(Credential::Literal(value)) => (Some(value), None),
        Some(Credential::Environment(name)) => (None, Some(name)),
        None => (None, None),
    };
    found.candidates.push(Candidate {
        source,
        model,
        base_url,
        api: protocol.into(),
        api_key,
        api_key_env,
        endpoint,
    });
}

// Remove terminal control characters from labels, never display credential fields.
pub fn display(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(140)
        .collect()
}

fn merge(base: &mut Value, patch: &Value) {
    if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            merge(base.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *base = patch.clone();
    }
}

type ClaudeModel = (String, String, Option<Credential>);

// Claude user settings are data, not a source of hooks, login tokens or commands.
fn claude_file_env<'a>(
    config: &'a Value,
    name: &str,
) -> std::result::Result<Option<&'a str>, &'static str> {
    let Some(env) = config.get("env") else {
        return Ok(None);
    };
    let env = env.as_object().ok_or("invalid environment settings")?;
    let mut entries = env.iter().filter(|(key, _)| {
        if cfg!(windows) {
            key.eq_ignore_ascii_case(name)
        } else {
            key.as_str() == name
        }
    });
    let first = entries.next();
    if entries.next().is_some() {
        return Err("ambiguous environment variable names");
    }
    first
        .map(|(_, value)| value.as_str().ok_or("invalid environment setting type"))
        .transpose()
}

fn claude_env(
    config: &Value,
    name: &str,
    inherited: &impl Fn(&str) -> Option<String>,
) -> std::result::Result<Option<String>, &'static str> {
    let value = match claude_file_env(config, name)? {
        // An empty file value masks the shell value. Values are not expanded.
        Some(value) => Some(value.to_owned()),
        None => inherited(name),
    };
    Ok(value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()))
}

fn claude_settings(
    config: &Value,
    inherited: &impl Fn(&str) -> Option<String>,
) -> std::result::Result<ClaudeModel, &'static str> {
    if !config.is_object()
        || config.get("model").is_some_and(|v| !v.is_string())
        || config.get("env").is_some_and(|v| {
            !v.as_object()
                .is_some_and(|e| e.values().all(Value::is_string))
        })
    {
        return Err("invalid model or environment settings");
    }
    if config
        .get("apiKeyHelper")
        .is_some_and(|v| v.as_str().is_none_or(|s| !s.trim().is_empty()))
    {
        return Err("command authentication cannot be imported");
    }
    if config
        .get("modelOverrides")
        .is_some_and(|v| !v.as_object().is_some_and(|m| m.is_empty()))
    {
        return Err("provider-specific model overrides cannot be imported");
    }
    for name in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_USE_BEDROCK_MANTLE",
    ] {
        if claude_env(config, name, inherited)?.is_some_and(|v| {
            !matches!(
                v.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        }) {
            return Err("cloud-provider authentication is not supported by this importer");
        }
    }
    if claude_env(config, "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST", inherited)?.is_some() {
        return Err("host-managed provider settings cannot be imported");
    }
    for name in ["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"] {
        if claude_env(config, name, inherited)?.is_some() {
            return Err("bearer or account authentication is not an x-api-key configuration");
        }
    }
    for name in [
        "ANTHROPIC_CUSTOM_HEADERS",
        "ANTHROPIC_BETAS",
        "CLAUDE_CODE_CLIENT_CERT",
        "CLAUDE_CODE_CLIENT_KEY",
        "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
    ] {
        if claude_env(config, name, inherited)?.is_some() {
            return Err("extra request headers or client certificates cannot be imported");
        }
    }
    if config
        .get("env")
        .and_then(Value::as_object)
        .is_some_and(|env| {
            env.keys().any(|name| {
                matches!(
                    name.to_ascii_uppercase().as_str(),
                    "HTTP_PROXY"
                        | "HTTPS_PROXY"
                        | "ALL_PROXY"
                        | "NO_PROXY"
                        | "NODE_EXTRA_CA_CERTS"
                        | "NODE_TLS_REJECT_UNAUTHORIZED"
                        | "SSL_CERT_FILE"
                        | "SSL_CERT_DIR"
                )
            })
        })
    {
        return Err("settings-file network overrides cannot be imported");
    }
    let mut model = claude_env(config, "ANTHROPIC_MODEL", inherited)?
        .or_else(|| string(config, "model"))
        .ok_or("an explicit provider model ID is required")?;
    let alias = match model.to_ascii_lowercase().as_str() {
        "sonnet" => Some("ANTHROPIC_DEFAULT_SONNET_MODEL"),
        "opus" => Some("ANTHROPIC_DEFAULT_OPUS_MODEL"),
        "haiku" => Some("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
        "fable" => Some("ANTHROPIC_DEFAULT_FABLE_MODEL"),
        _ => None,
    };
    if let Some(name) = alias {
        model = claude_env(config, name, inherited)?
            .ok_or("model alias needs an explicit provider model ID mapping")?;
    }
    if matches!(
        model.to_ascii_lowercase().as_str(),
        "default" | "inherit" | "best" | "opusplan" | "sonnet" | "opus" | "haiku" | "fable"
    ) || model.contains(['[', ']'])
    {
        return Err("dynamic model aliases and context modifiers cannot be imported");
    }
    let root = claude_env(config, "ANTHROPIC_BASE_URL", inherited)?
        .unwrap_or_else(|| "https://api.anthropic.com".into());
    let root = crate::config::validate_url(&root)
        .ok()
        .filter(|_| root.contains("://"))
        .ok_or("invalid or unsupported service URL")?;
    // Claude's API root is a prefix for /v1/messages, including any existing /v1.
    // The exact endpoint prevents the shared URL normalizer from dropping it.
    let endpoint = format!("{}/v1/messages", root.as_str().trim_end_matches('/'));
    let key = if let Some(value) = claude_file_env(config, "ANTHROPIC_API_KEY")? {
        (!value.trim().is_empty()).then(|| Credential::Literal(value.trim().to_owned()))
    } else {
        inherited("ANTHROPIC_API_KEY")
            .filter(|v| !v.trim().is_empty())
            .map(|_| Credential::Environment("ANTHROPIC_API_KEY".into()))
    };
    Ok((model, endpoint, key))
}

fn read_claude(root: &Path, inherited: &impl Fn(&str) -> Option<String>, found: &mut Discovery) {
    let path = root.join("settings.json");
    let Some(config) = json(&path) else {
        if path.exists() {
            found
                .notes
                .push("Claude Code: settings could not be read or parsed; skipped.".into());
        }
        return;
    };
    match claude_settings(&config, inherited) {
        Ok((model, endpoint, key)) => candidate(
            "Claude Code/user".into(),
            model,
            endpoint,
            "anthropic",
            key,
            true,
            found,
        ),
        Err(reason) => found.notes.push(format!("Claude Code: {reason}; skipped.")),
    }
}

fn read_codex(root: &Path, found: &mut Discovery) {
    let path = root.join("config.toml");
    let config: Value = match text(&path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|v| serde_json::to_value(v).ok())
    {
        Some(v) => v,
        None if !path.exists() => return,
        None => {
            found
                .notes
                .push("Codex: configuration could not be read or parsed; skipped.".into());
            return;
        }
    };
    let auth = json(&root.join("auth.json")).unwrap_or(Value::Null);
    if auth["auth_mode"] == "chatgpt" || auth.get("tokens").is_some_and(|v| !v.is_null()) {
        found.notes.push(
            "Codex: account sign-in is not an API key; subscription tokens are not imported."
                .into(),
        );
    }
    let mut variants = vec![("default".to_owned(), config.clone())];
    if let Some(profiles) = config["profiles"].as_object() {
        for (name, profile) in profiles {
            let mut combined = config.clone();
            merge(&mut combined, profile);
            variants.push((name.clone(), combined));
        }
    }
    if let Some(active) = string(&config, "profile") {
        variants.retain(|(name, _)| name != "default");
        variants.sort_by_key(|(name, _)| name != &active);
    }
    for (name, variant) in variants {
        let Some(model) =
            string(&variant, "model").or_else(|| string(&variant["models"]["new_thread"], "model"))
        else {
            continue;
        };
        let provider_id = string(&variant, "model_provider").unwrap_or_else(|| "openai".into());
        let provider = &variant["model_providers"][&provider_id];
        let (default_url, default_api) = match provider_id.as_str() {
            "openai" => ("https://api.openai.com/v1", "responses"),
            "ollama" => ("http://localhost:11434/v1", "chat"),
            "lmstudio" => ("http://localhost:1234/v1", "chat"),
            _ => ("", "responses"),
        };
        let url = string(provider, "base_url")
            .or_else(|| {
                if provider_id == "openai" {
                    string(&variant, "openai_base_url")
                } else {
                    None
                }
            })
            .unwrap_or_else(|| default_url.into());
        let protocol = string(provider, "wire_api").unwrap_or_else(|| default_api.into());
        let requires_auth = provider_id == "openai" || provider["requires_openai_auth"] == true;
        let key = string(provider, "env_key")
            .and_then(|name| env_key(&name))
            .or_else(|| literal_key(string(provider, "experimental_bearer_token")))
            .or_else(|| {
                if requires_auth {
                    env_key("OPENAI_API_KEY")
                        .or_else(|| string(&auth, "OPENAI_API_KEY").map(Credential::Literal))
                } else {
                    None
                }
            });
        if provider.get("auth").is_some()
            || provider.get("http_headers").is_some()
            || provider.get("env_http_headers").is_some()
            || provider.get("query_params").is_some()
        {
            found.notes.push(format!("Codex/{}: command authentication or extra headers/query parameters cannot be imported; skipped.", display(&name)));
            continue;
        }
        candidate(
            format!("Codex/{name}"),
            model,
            url,
            &protocol,
            key,
            false,
            found,
        );
    }
}

fn read_workbuddy(path: &Path, label: &str, found: &mut Discovery) {
    let doc = match json(path) {
        Some(doc) => doc,
        None if !path.exists() => return,
        None => {
            found.notes.push(format!(
                "{label}: models.json could not be read or parsed; skipped."
            ));
            return;
        }
    };
    let Some(models) = doc.as_array().or_else(|| doc["models"].as_array()) else {
        found
            .notes
            .push(format!("{label}: unsupported models.json format; skipped."));
        return;
    };
    for entry in models.iter().take(256) {
        let Some(model) = string(entry, "id").or_else(|| string(entry, "model")) else {
            continue;
        };
        let Some(url) = string(entry, "url").or_else(|| string(entry, "baseURL")) else {
            continue;
        };
        let protocol = string(entry, "api")
            .or_else(|| string(entry, "protocol"))
            .unwrap_or_else(|| "chat".into());
        let key = string(entry, "apiKeyEnv")
            .and_then(|name| env_key(&name))
            .or_else(|| literal_key(string(entry, "apiKey")));
        candidate(
            label.into(),
            model,
            url,
            &protocol,
            key,
            entry["useCustomProtocol"] == true,
            found,
        );
    }
}

fn dsh_key(provider: &Value, credentials: &Value) -> Option<Credential> {
    if let Some(name) = string(provider, "apiKeyEnv") {
        if let Some(value) = env_key(&name) {
            return Some(value);
        }
        if let Some(reference) = string(&credentials["refs"], &name) {
            if let Some(record) = credentials["records"].get(&reference) {
                // Never turn a browser session / account access token into an API key.
                if matches!(
                    record["kind"].as_str(),
                    Some("api-key" | "api_key" | "secret")
                ) {
                    return string(&record["payload"], "secret")
                        .or_else(|| string(&record["payload"], "apiKey"))
                        .map(Credential::Literal);
                }
                return None;
            }
            if !reference.contains("://") && !reference.starts_with("enc:") {
                return literal_key(Some(reference));
            }
        }
    }
    literal_key(string(provider, "apiKey"))
}

fn read_dsh(root: &Path, found: &mut Discovery) {
    let Ok(entries) = std::fs::read_dir(root.join("profiles")) else {
        return;
    };
    let mut profiles: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .take(64)
        .collect();
    profiles.sort_by_key(|e| e.file_name());
    let credentials = yaml(&root.join(".credentials.yaml")).unwrap_or(Value::Null);
    for profile in profiles {
        let label = format!("dsh/{}", profile.file_name().to_string_lossy());
        let mut plugins = BTreeMap::<String, Value>::new();
        for filename in ["cordis.yml", "cordis.patch.yml"] {
            let path = profile.path().join(filename);
            let Some(doc) = yaml(&path) else {
                if path.exists() {
                    found.notes.push(format!(
                        "{}: YAML could not be read or parsed; skipped file.",
                        display(&label)
                    ));
                }
                continue;
            };
            let Some(entries) = doc.as_array() else {
                found.notes.push(format!(
                    "{}: unsupported profile format; skipped file.",
                    display(&label)
                ));
                continue;
            };
            for (i, plugin) in entries.iter().take(256).enumerate() {
                let id = string(plugin, "id")
                    .or_else(|| string(plugin, "name"))
                    .unwrap_or_else(|| i.to_string());
                merge(plugins.entry(id).or_insert(Value::Null), plugin);
            }
        }
        for plugin in plugins.values() {
            let Some(providers) = plugin["config"]["providers"].as_object() else {
                continue;
            };
            for (id, provider) in providers {
                let Some(url) =
                    string(provider, "baseURL").or_else(|| string(provider, "base_url"))
                else {
                    continue;
                };
                let protocol =
                    string(provider, "api").unwrap_or_else(|| "openai-completions".into());
                let key = dsh_key(provider, &credentials);
                if let Some(models) = provider["models"].as_array() {
                    for model in models.iter().take(256) {
                        if let Some(model) = model
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| string(model, "id"))
                        {
                            candidate(
                                format!("{label}/{id}"),
                                model,
                                url.clone(),
                                &protocol,
                                key.clone(),
                                false,
                                found,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(root: &Path, path: &str, value: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }

    #[test]
    fn claude_user_settings_resolve_model_root_and_credential() {
        let shell = |name: &str| match name {
            "ANTHROPIC_MODEL" => Some("shell-model".into()),
            "ANTHROPIC_BASE_URL" => Some("https://shell.example".into()),
            "ANTHROPIC_API_KEY" => Some("inherited-secret".into()),
            _ => None,
        };
        let config = serde_json::json!({
            "model": "top-level-model",
            "env": {
                "ANTHROPIC_MODEL": "sonnet",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "pinned-sonnet",
                "ANTHROPIC_BASE_URL": "https://gateway.example/team/v1/",
                "ANTHROPIC_API_KEY": "${PLAIN_TEXT}"
            }
        });
        let (model, endpoint, key) = claude_settings(&config, &shell).unwrap();
        assert_eq!(model, "pinned-sonnet");
        assert_eq!(endpoint, "https://gateway.example/team/v1/v1/messages");
        assert!(matches!(key, Some(Credential::Literal(key)) if key == "${PLAIN_TEXT}"));

        let config = serde_json::json!({"model": "top-level-model"});
        let (model, endpoint, key) = claude_settings(&config, &shell).unwrap();
        assert_eq!(model, "shell-model");
        assert_eq!(endpoint, "https://shell.example/v1/messages");
        assert!(matches!(key, Some(Credential::Environment(name)) if name == "ANTHROPIC_API_KEY"));

        let config = serde_json::json!({
            "model": "top-level-model",
            "env": {"ANTHROPIC_MODEL": "", "ANTHROPIC_BASE_URL": "", "ANTHROPIC_API_KEY": ""}
        });
        let (model, endpoint, key) = claude_settings(&config, &shell).unwrap();
        assert_eq!(model, "top-level-model");
        assert_eq!(endpoint, "https://api.anthropic.com/v1/messages");
        assert!(key.is_none());
    }

    #[test]
    fn claude_user_settings_skip_unsupported_or_ambiguous_sources_safely() {
        let fixtures = [
            serde_json::json!({"model": "sonnet"}),
            serde_json::json!({"model": "best"}),
            serde_json::json!({"model": "claude-example[1m]"}),
            serde_json::json!({"model": "concrete", "apiKeyHelper": "PRIVATE_FIXTURE"}),
            serde_json::json!({"model": "concrete", "modelOverrides": {"concrete": "PRIVATE_FIXTURE"}}),
            serde_json::json!({"model": "concrete", "env": {"ANTHROPIC_AUTH_TOKEN": "PRIVATE_FIXTURE"}}),
            serde_json::json!({"model": "concrete", "env": {"CLAUDE_CODE_OAUTH_TOKEN": "PRIVATE_FIXTURE"}}),
            serde_json::json!({"model": "concrete", "env": {"CLAUDE_CODE_USE_BEDROCK": "1"}}),
            serde_json::json!({"model": "concrete", "env": {"CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST": "1"}}),
            serde_json::json!({"model": "concrete", "env": {"ANTHROPIC_CUSTOM_HEADERS": "PRIVATE_FIXTURE"}}),
            serde_json::json!({"model": "concrete", "env": {"HTTPS_PROXY": "PRIVATE_FIXTURE"}}),
            serde_json::json!({"model": "concrete", "env": {"ANTHROPIC_BASE_URL": "https://user:PRIVATE_FIXTURE@gateway.example"}}),
            serde_json::json!({"model": "concrete", "env": {"ANTHROPIC_BASE_URL": "https:"}}),
            serde_json::json!({"model": "concrete", "env": {"ANTHROPIC_API_KEY": false}}),
        ];
        let root = tempfile::tempdir().unwrap();
        for config in fixtures {
            write(root.path(), "settings.json", &config.to_string());
            let mut found = Discovery::default();
            read_claude(root.path(), &|_| None, &mut found);
            assert!(found.candidates.is_empty());
            assert_eq!(found.notes.len(), 1);
            assert!(!found.notes[0].contains("PRIVATE_FIXTURE"));
        }
    }

    #[test]
    fn codex_profiles_and_api_key_import_without_subscription_tokens() {
        let home = tempfile::tempdir().unwrap();
        write(
            home.path(),
            ".codex/config.toml",
            "model='default-model'\nmodel_provider='custom'\nprofile='fast'\n[model_providers.custom]\nbase_url='https://example.com/v1'\nwire_api='responses'\n[profiles.fast]\nmodel='fast-model'\n",
        );
        write(
            home.path(),
            ".codex/auth.json",
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"never-copy-this"}}"#,
        );
        let found = discover_at(home.path(), None, None);
        assert_eq!(found.candidates.len(), 1);
        assert_eq!(found.candidates[0].model, "fast-model");
        assert!(found.candidates[0].api_key.is_none());
        assert!(found.notes.iter().any(|n| n.contains("account sign-in")));
        write(home.path(), ".codex/config.toml", "model='api-model'\n");
        write(
            home.path(),
            ".codex/auth.json",
            r#"{"OPENAI_API_KEY":"dummy-api-key"}"#,
        );
        assert_eq!(
            discover_at(home.path(), None, None).candidates[0]
                .api_key
                .as_deref(),
            Some("dummy-api-key")
        );
    }
    #[test]
    fn workbuddy_full_urls_and_exact_custom_endpoint() {
        let home = tempfile::tempdir().unwrap();
        write(
            home.path(),
            ".workbuddy/models.json",
            r#"[{"id":"model-a","url":"https://example.com/plan/v3/chat/completions","apiKey":"dummy-key"},{"id":"model-b","url":"https://example.com/custom/infer/","useCustomProtocol":true}]"#,
        );
        let found = discover_at(home.path(), None, None);
        assert_eq!(found.candidates[0].base_url, "https://example.com/plan/v3");
        assert_eq!(found.candidates[0].api_key.as_deref(), Some("dummy-key"));
        assert_eq!(
            found.candidates[1].endpoint.as_deref(),
            Some("https://example.com/custom/infer/")
        );
    }
    #[test]
    fn dsh_patches_and_plain_credential_reference() {
        let home = tempfile::tempdir().unwrap();
        write(
            home.path(),
            ".dsh/profiles/desktop/cordis.yml",
            "- id: llm\n  config:\n    providers:\n      custom:\n        baseURL: https://example.com/anthropic\n        api: anthropic-messages\n        apiKeyEnv: ZISTRAL_TEST_NONEXISTENT_KEY\n        models: [{id: old-model}]\n",
        );
        write(
            home.path(),
            ".dsh/profiles/desktop/cordis.patch.yml",
            "- id: llm\n  config:\n    providers:\n      custom:\n        models: [{id: new-model}]\n",
        );
        write(
            home.path(),
            ".dsh/.credentials.yaml",
            "refs:\n  ZISTRAL_TEST_NONEXISTENT_KEY: dummy-provider-key\nrecords:\n  login:\n    kind: account\n    payload:\n      token: never-copy-login\n",
        );
        let found = discover_at(home.path(), None, None);
        assert_eq!(found.candidates.len(), 1);
        assert_eq!(found.candidates[0].model, "new-model");
        assert_eq!(found.candidates[0].api, "anthropic");
        assert_eq!(
            found.candidates[0].api_key.as_deref(),
            Some("dummy-provider-key")
        );
    }
    #[test]
    fn malformed_source_does_not_block_others_or_echo_secrets() {
        let home = tempfile::tempdir().unwrap();
        write(
            home.path(),
            ".codex/config.toml",
            "api_key='not-in-errors' malformed",
        );
        write(
            home.path(),
            ".workbuddy/models.json",
            r#"[{"id":"ok","url":"http://localhost:11434/v1"}]"#,
        );
        let found = discover_at(home.path(), None, None);
        assert_eq!(found.candidates.len(), 1);
        assert!(!found.notes.join(" ").contains("not-in-errors"));
        assert_eq!(display("a\u{1b}\nkey"), "akey");
    }
}
