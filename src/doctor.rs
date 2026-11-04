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

const REPORT_LIMIT: usize = 64 * 1024;

fn diagnostic_error(kind: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"kind":kind,"http_status":null,"retries":null,"message":message})
}

fn probe_error(error: &anyhow::Error) -> serde_json::Value {
    if let Some(http) = error.downcast_ref::<crate::agent::HttpFailure>() {
        return serde_json::json!({"kind":"http","http_status":http.status,"retries":http.retries,"message":http.advice});
    }
    if let Some(network) = error.downcast_ref::<reqwest::Error>() {
        return if network.is_timeout() {
            diagnostic_error("timeout", "model probe timed out")
        } else if network.is_builder() {
            diagnostic_error(
                "request",
                "cannot prepare the model request; check service/authentication settings",
            )
        } else {
            diagnostic_error(
                "network",
                "model request failed; check service availability, TLS and network/proxy settings",
            )
        };
    }
    diagnostic_error(
        "response",
        "model probe did not produce a complete supported response; details hidden",
    )
}

fn encode_report(value: &serde_json::Value, limit: usize, terminal: bool) -> Option<String> {
    struct Limited {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("diagnostic report exceeds limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Limited {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).ok()?;
    let json = String::from_utf8(output.bytes).ok()?;
    if terminal {
        ui::json_display_with_limit(&json, limit)
    } else {
        Some(json)
    }
}

fn limit_report(requested: bool, status: &str) -> serde_json::Value {
    serde_json::json!({
        "version":1,"kind":"doctor-report","zi_version":env!("CARGO_PKG_VERSION"),"ok":false,
        "configuration":{"path":null,"status":"unavailable","details":null},
        "probe":{"requested":requested,"status":status,"max_requests":u8::from(requested),"max_output_tokens":if requested{Some(16)}else{None},"field":null,"elapsed_ms":null},
        "error":diagnostic_error("report_limit", "diagnostic report exceeds 64 KiB; settings omitted")
    })
}

pub fn run_json(cli: &Cli, probe: bool) -> Result<()> {
    use serde_json::json;
    use std::io::IsTerminal;
    let terminal = io::stdout().is_terminal();
    let settings = config::Settings::load(cli);
    let (details, error) = match &settings {
        Ok(settings) => {
            let credential = match &settings.api_key_source {
                config::ApiKeySource::Environment(name) => {
                    json!({"configured":true,"source":"environment","environment_variable":name})
                }
                config::ApiKeySource::SavedProfile => {
                    json!({"configured":true,"source":"profile","environment_variable":null})
                }
                config::ApiKeySource::NotConfigured => {
                    json!({"configured":false,"source":"none","environment_variable":null})
                }
            };
            let budget = settings.output_budget;
            let environment: Vec<_> = [
                "ZI_CONFIG",
                "ZI_PROFILE",
                "ZI_MODEL",
                "ZI_BASE_URL",
                "ZI_API",
            ]
            .into_iter()
            .filter(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
            .collect();
            (
                json!({
                    "profile":settings.profile_name,"model":settings.model,"protocol":settings.api,
                    "service_host":config::validate_url(&settings.base_url)?.host_str(),
                    "credential":credential,
                    "output_budget":{"tokens_per_turn":budget.tokens,"field":budget.tokens.map(|_|budget.field),"tokens_source":budget.tokens_source,"field_source":budget.tokens.map(|_|budget.field_source),"profile_preferences_ignored":cli.no_profile_budget},
                    "environment_settings_present":environment
                }),
                serde_json::Value::Null,
            )
        }
        Err(_) => (
            serde_json::Value::Null,
            diagnostic_error(
                "configuration",
                "configuration check failed; run zi config or human-readable zi doctor",
            ),
        ),
    };
    let mut report = json!({
        "version":1,"kind":"doctor-report","zi_version":env!("CARGO_PKG_VERSION"),"ok":settings.is_ok(),
        "configuration":{"path":config::path(cli).ok().map(|path|path.to_string_lossy().into_owned()),"status":if settings.is_ok(){"ok"}else{"failed"},"details":details},
        "probe":{"requested":probe,"status":if probe{"not_run"}else{"not_requested"},"max_requests":u8::from(probe),"max_output_tokens":if probe{Some(16)}else{None},"field":settings.as_ref().ok().filter(|_|probe).map(|s|s.output_budget.probe(&s.api).field),"elapsed_ms":null},
        "error":error
    });
    // Reserve room for bounded probe status/error fields, before any network IO.
    if encode_report(&report, REPORT_LIMIT - 2048, terminal).is_none() {
        report = limit_report(probe, if probe { "not_run" } else { "not_requested" });
    } else if let Ok(settings) = settings {
        if probe {
            let start = Instant::now();
            let result = crate::agent::probe(&settings, cli);
            report["probe"]["elapsed_ms"] =
                json!(start.elapsed().as_millis().min(u64::MAX as u128) as u64);
            match result {
                Ok(()) => report["probe"]["status"] = json!("ok"),
                Err(error) => {
                    report["ok"] = json!(false);
                    report["probe"]["status"] = json!("failed");
                    report["error"] = probe_error(&error);
                }
            }
        }
    }
    let json = match encode_report(&report, REPORT_LIMIT - 1, terminal) {
        Some(json) => json,
        None => {
            let status = report["probe"]["status"].as_str().unwrap_or("not_run");
            report = limit_report(probe, status);
            encode_report(&report, REPORT_LIMIT - 1, terminal)
                .context("cannot encode minimal diagnostic report")?
        }
    };
    let mut stdout = io::stdout().lock();
    stdout.write_all(json.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    if report["ok"] != true {
        anyhow::bail!("doctor checks failed; see JSON report");
    }
    Ok(())
}
