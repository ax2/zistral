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
    report!("Profile: {}", ui::clean(&config::active_name(cli)?));
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
    let overrides: Vec<_> = [
        "ZI_PROFILE",
        "ZI_MODEL",
        "ZI_BASE_URL",
        "ZI_API",
        "ZI_API_KEY",
        "OPENAI_API_KEY",
    ]
    .into_iter()
    .filter(|key| std::env::var(key).is_ok_and(|v| !v.trim().is_empty()))
    .collect();
    if !overrides.is_empty() {
        report!("Environment overrides: {}", overrides.join(", "));
    }
    report!("Configuration: OK");
    if probe {
        let mut test = cli.clone();
        test.prompt = vec!["Reply with OK.".into()];
        test.no_tools = true;
        test.no_stream = true;
        test.stream = false;
        test.plain = true;
        test.verbose = false;
        test.max_steps = Some(1);
        test.retries = 0;
        test.max_output_tokens = 16;
        test.timeout = cli.timeout.min(15);
        output.flush()?;
        let start = Instant::now();
        crate::agent::run(&settings, &test, "")
            .context("model probe failed (provider response bodies are hidden)")?;
        report!("Model probe: OK ({:.2}s)", start.elapsed().as_secs_f64());
    } else {
        report!(
            "Network: not tested. Use zi doctor --probe for a small model request (provider charges may apply)."
        );
    }
    Ok(())
}
