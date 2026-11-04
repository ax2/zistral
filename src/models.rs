use crate::{Cli, agent::HttpFailure, config::Settings, ui};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Read, Write};
use std::time::{Duration, Instant};

const PAGE_BYTES: u64 = 1_048_576;
const TOTAL_BYTES: u64 = 4_194_304;
const MAX_MODELS: usize = 4096;
const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct Page {
    data: Vec<Model>,
    object: Option<String>,
    has_more: Option<bool>,
    first_id: Option<String>,
    last_id: Option<String>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    object: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Serialize)]
struct Catalogue {
    version: u32,
    kind: &'static str,
    profile: String,
    protocol: String,
    service_host: String,
    // Complete for the supported endpoint's returned catalogue, not a claim of
    // inference permission, model compatibility, or service-wide completeness.
    complete: bool,
    pages: usize,
    models: Vec<String>,
}

pub fn run(cli: &Cli, json: bool) -> Result<()> {
    let settings = Settings::load_for_models(cli)?;
    let catalogue = fetch(&settings, cli.timeout)?;
    // Validate every page before writing anything, including for plain pipes.
    let output = if json {
        let mut raw = serde_json::to_string(&catalogue)?;
        if io::stdout().is_terminal() {
            raw = ui::json_display(&raw);
        }
        raw.push('\n');
        raw
    } else {
        catalogue
            .models
            .iter()
            .map(|id| format!("{id}\n"))
            .collect()
    };
    if ui::enabled(cli) {
        eprintln!(
            "{} model IDs reported by {} ({} page(s)); calling permissions and tool support are not verified.",
            catalogue.models.len(),
            catalogue.service_host,
            catalogue.pages
        );
    }
    io::stdout().lock().write_all(output.as_bytes())?;
    Ok(())
}

fn fetch(settings: &Settings, timeout: u64) -> Result<Catalogue> {
    let anthropic = settings.api == "anthropic";
    // The stored exact endpoint is a generation endpoint, never a models URL.
    // Retain reverse-proxy path prefixes and the existing Anthropic /v1 rule.
    let path = if anthropic && !settings.base_url.ends_with("/v1") {
        "v1/models"
    } else {
        "models"
    };
    let endpoint = crate::config::validate_url(&format!("{}/{path}", settings.base_url))?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(timeout.min(15)))
        .user_agent(concat!("zistral/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| anyhow::anyhow!("cannot initialize catalogue HTTP client"))?;
    let deadline = Instant::now() + Duration::from_secs(timeout.min(30));
    let mut catalogue = Catalogue {
        version: 1,
        kind: "model-catalogue",
        profile: settings.profile_name.clone(),
        protocol: settings.api.clone(),
        service_host: endpoint.host_str().unwrap_or_default().into(),
        complete: true,
        pages: 0,
        models: Vec::new(),
    };
    let mut seen = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;
    loop {
        if total == TOTAL_BYTES {
            bail!("model catalogue exceeds response size limit; nothing written");
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .context("model catalogue timed out; nothing written")?;
        let mut request = client.get(endpoint.clone()).timeout(remaining);
        if anthropic {
            request = request
                .query(&[("limit", "1000")])
                .header("anthropic-version", "2023-06-01");
            if let Some(cursor) = &cursor {
                request = request.query(&[("after_id", cursor)]);
            }
        }
        if let Some(key) = &settings.api_key {
            request = if anthropic {
                request.header("x-api-key", key)
            } else {
                request.bearer_auth(key)
            };
        }
        // Never render reqwest errors: URLs may contain private routing paths.
        let response = request.send().map_err(safe_transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(HttpFailure {
                status: status.as_u16(), retries: 0,
                advice: match status.as_u16() {
                    401 => "check the selected profile's API key",
                    403 => "catalogue access denied; check account permissions and network proxy",
                    404 => "models endpoint unavailable; check the API root or use zi models for saved profiles",
                    429 => "catalogue rate limited; try again later",
                    _ => "model catalogue request failed; redirects and retries are disabled",
                },
            }.into());
        }
        if response
            .content_length()
            .is_some_and(|n| n > PAGE_BYTES || n > TOTAL_BYTES - total)
        {
            bail!("model catalogue exceeds response size limit; nothing written");
        }
        let mut bytes = Vec::new();
        response
            .take(PAGE_BYTES.min(TOTAL_BYTES - total) + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                anyhow::anyhow!("cannot read complete model catalogue; nothing written")
            })?;
        total += bytes.len() as u64;
        if bytes.len() as u64 > PAGE_BYTES || total > TOTAL_BYTES {
            bail!("model catalogue exceeds response size limit; nothing written");
        }
        let page: Page = serde_json::from_slice(&bytes).map_err(|_| {
            anyhow::anyhow!(
                "invalid model catalogue response; expected a JSON data array of model IDs"
            )
        })?;
        if page.object.as_deref().is_some_and(|v| v != "list") {
            bail!("invalid model catalogue object type; nothing written");
        }
        if page
            .first_id
            .as_ref()
            .is_some_and(|id| page.data.first().map(|m| &m.id) != Some(id))
            || page
                .last_id
                .as_ref()
                .is_some_and(|id| page.data.last().map(|m| &m.id) != Some(id))
        {
            bail!("invalid model catalogue pagination cursor; nothing written");
        }
        let more = if anthropic {
            page.has_more
                .context("invalid model catalogue: missing has_more; nothing written")?
        } else {
            if page.has_more == Some(true) {
                bail!("model catalogue uses unsupported pagination; nothing written");
            }
            false
        };
        for model in page.data {
            if model.id.is_empty()
                || model.id.len() > 512
                || model
                    .id
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace())
                || model.object.as_deref().is_some_and(|v| v != "model")
                || model.kind.as_deref().is_some_and(|v| v != "model")
                || settings
                    .api_key
                    .as_ref()
                    .is_some_and(|key| model.id.contains(key))
            {
                bail!("invalid model ID in catalogue; nothing written");
            }
            if !seen.insert(model.id.clone()) {
                bail!("duplicate model ID in catalogue; nothing written");
            }
            if catalogue.models.len() == MAX_MODELS {
                bail!("model catalogue exceeds 4096 model limit; nothing written");
            }
            catalogue.models.push(model.id);
        }
        catalogue.pages += 1;
        if !more {
            break;
        }
        cursor = page.last_id;
        if cursor.is_none() || cursor.as_ref() != catalogue.models.last() {
            bail!("invalid model catalogue pagination cursor; nothing written");
        }
        if catalogue.pages == MAX_PAGES {
            bail!("model catalogue exceeds 10 page limit; nothing written");
        }
    }
    Ok(catalogue)
}

fn safe_transport_error(error: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(if error.is_timeout() {
        "model catalogue timed out; nothing written"
    } else if error.is_builder() {
        "cannot prepare model catalogue request; check service and credential settings"
    } else {
        "model catalogue connection failed; check service and proxy settings"
    })
}
