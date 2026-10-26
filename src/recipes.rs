//! Explicit local definitions, never implicit project discovery or executable scripts.
use crate::transforms::{self, Operation, Step};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

const MAX_RECIPE: u64 = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    version: u32,
    steps: Vec<Step>,
}

pub struct Plan(Vec<Step>);

impl Plan {
    pub fn prepare(
        operation: Option<Operation>,
        reverse: bool,
        pointer: Option<&str>,
        steps: &[String],
        recipe: Option<&Path>,
    ) -> Result<Self> {
        if (reverse || pointer.is_some()) && operation.is_none() {
            bail!(
                "--reverse/--pointer require a single operation; put options in individual steps"
            );
        }
        let plan = if let Some(operation) = operation {
            vec![Step {
                operation,
                reverse,
                pointer: pointer.map(str::to_owned),
            }]
        } else if let Some(path) = recipe {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .with_context(|| format!("cannot open recipe {}", path.display()))?
                .take(MAX_RECIPE + 1)
                .read_to_end(&mut bytes)
                .context("cannot read recipe")?;
            if bytes.len() as u64 > MAX_RECIPE {
                bail!("recipe exceeds 64 KiB");
            }
            let text = std::str::from_utf8(&bytes).context("recipe must be UTF-8")?;
            let file: Recipe = toml::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))
                .map_err(|_| anyhow::anyhow!("invalid recipe: expected version and native operation steps; see zi data --help"))?;
            if file.version != 1 {
                bail!("unsupported recipe version; expected version = 1");
            }
            file.steps
        } else {
            if steps.len() > transforms::MAX_STEPS {
                bail!("a data pipeline requires 1-16 steps");
            }
            steps.iter().map(|text| {
                if let Some(pointer) = text.strip_prefix("json-select:") {
                    return Ok(Step { operation: Operation::JsonSelect, reverse: false, pointer: Some(pointer.to_owned()) });
                }
                let (name, reverse) = text.strip_suffix(":reverse")
                    .map_or((text.as_str(), false), |name| (name, true));
                let operation = Operation::from_str(name, false).map_err(|_| {
                    anyhow::anyhow!("unsupported --step; see zi data --help for native operations (sort:reverse is also supported)")
                })?;
                Ok(Step { operation, reverse, pointer: None })
            }).collect::<Result<Vec<_>>>()?
        };
        if plan.is_empty() || plan.len() > transforms::MAX_STEPS {
            bail!("a data pipeline requires 1-16 steps");
        }
        for (index, step) in plan.iter().enumerate() {
            step.validate()
                .with_context(|| format!("data pipeline step {} is invalid", index + 1))?;
        }
        Ok(Self(plan))
    }

    pub fn run(&self, input: &str) -> Result<String> {
        transforms::run_pipeline(input, &self.0)
    }

    pub fn render(&self) -> Result<String> {
        let recipe = Recipe {
            version: 1,
            steps: self.0.clone(),
        };
        toml::to_string_pretty(&recipe).context("cannot render data plan")
    }
}
