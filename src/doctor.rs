use crate::{Cli, config, ui};
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::time::Instant;

pub fn run(cli: &Cli, probe: bool) -> Result<()> {
    let mut output = io::stdout().lock();
    macro_rules! report { ($($args:tt)*) => { writeln!(output, $($args)*)? }; }
    report!("zi doctor {}", env!("CARGO_PKG_VERSION"));
    report!(
        "Config: {}",
        ui::clean(&config::path(cli)?.display().to_string())
    );
    let settings =
        config::Settings::load(cli).context("configuration check failed; run zi config")?;
    report!("Profile: {}", ui::clean(&settings.profile_name));
    report!("Model: {}", ui::clean(&settings.model));
    report!("Protocol: {}", settings.api);
    let url = config::validate_url(&settings.base_url)?;
    report!("Service host: {}", url.host_str().unwrap_or("(unset)"));
    report!(
        "API Key: {}",
        if settings.api_key.is_some() {
            "configured (hidden)"
        } else {
            "not set; local/custom services may not require one"
        }
    );
    match &settings.api_key_source {
        config::ApiKeySource::Environment(name) => report!(
            "API Key source: environment variable {} (value hidden)",
            ui::json_display(&serde_json::to_string(name)?)
        ),
        config::ApiKeySource::SavedProfile => {
            report!("API Key source: saved profile (value hidden)")
        }
        config::ApiKeySource::NotConfigured => report!("API Key source: not configured"),
    }
    let budget = settings.output_budget;
    if let Some(tokens) = budget.tokens {
        report!(
            "Output cap: {tokens} tokens per turn (source: {})",
            budget.tokens_source
        );
        report!(
            "Output field: {} (source: {})",
            budget.field,
            budget.field_source
        );
    } else {
        report!("Output cap: omitted (source: default)");
    }
    let environment: Vec<_> = [
        "ZI_CONFIG",
        "ZI_PROFILE",
        "ZI_MODEL",
        "ZI_BASE_URL",
        "ZI_API",
    ]
    .into_iter()
    .filter(|key| std::env::var(key).is_ok_and(|v| !v.trim().is_empty()))
    .collect();
    if !environment.is_empty() {
        report!(
            "Environment settings present (CLI flags may override): {}",
            environment.join(", ")
        );
    }
    report!("Configuration: OK");
    if probe {
        report!(
            "Probe cap: 16 tokens per turn (field: {})",
            budget.probe(&settings.api).field
        );
        output.flush()?;
        let start = Instant::now();
        crate::agent::probe(&settings, cli)
            .context("model probe failed (provider response bodies are hidden)")?;
        report!("Model probe: OK ({:.2}s)", start.elapsed().as_secs_f64());
    } else {
        report!(
            "Network: not tested. Use zi doctor --probe for a small model request (provider charges may apply)."
        );
    }
    Ok(())
}
