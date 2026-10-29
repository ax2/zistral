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
    pub fn prepare(single: Option<Step>, steps: &[String], recipe: Option<&Path>) -> Result<Self> {
        let plan = if let Some(step) = single {
            vec![step]
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
                for (prefix, operation) in [("lines-head:", Operation::LinesHead), ("lines-tail:", Operation::LinesTail)] {
                    if let Some(count) = text.strip_prefix(prefix) {
                        let lines = count.parse::<usize>().map_err(|_| anyhow::anyhow!("line window count must be an integer from 1 to 100000"))?;
                        return Ok(Step { equals: None, operation, reverse: false, pointer: None, fields: None, contains: None, lines: Some(lines) });
                    }
                }
                for (prefix, operation) in [("json-select:", Operation::JsonSelect), ("jsonl-select:", Operation::JsonlSelect)] {
                    if let Some(pointer) = text.strip_prefix(prefix) {
                        return Ok(Step { equals: None, operation, reverse: false, pointer: Some(pointer.to_owned()), fields: None, contains: None, lines: None });
                    }
                }
                for (prefix, operation) in [("lines-include:", Operation::LinesInclude), ("lines-exclude:", Operation::LinesExclude)] {
                    if let Some(contains) = text.strip_prefix(prefix) {
                        return Ok(Step { equals: None, operation, reverse: false, pointer: None, fields: None, contains: Some(contains.to_owned()), lines: None });
                    }
                }
                let (name, reverse) = text.strip_suffix(":reverse")
                    .map_or((text.as_str(), false), |name| (name, true));
                let operation = Operation::from_str(name, false).map_err(|_| {
                    anyhow::anyhow!("unsupported --step; see zi data --help for native operations (sort:reverse is also supported)")
                })?;
                Ok(Step { equals: None, operation, reverse, pointer: None, fields: None, contains: None, lines: None })
            }).collect::<Result<Vec<_>>>()?
        };
        Self::from_steps(plan)
    }

    pub(crate) fn from_tool(arguments: &str) -> Result<Self> {
        let steps = match transforms::parse_arguments(arguments)? {
            transforms::ToolArguments::Single(step) => vec![step],
            transforms::ToolArguments::Pipeline(args) => args.steps,
        };
        Self::from_steps(steps)
    }

    fn from_steps(plan: Vec<Step>) -> Result<Self> {
        if plan.is_empty() || plan.len() > transforms::MAX_STEPS {
            bail!("a data pipeline requires 1-16 steps");
        }
        for (index, step) in plan.iter().enumerate() {
            step.validate()
                .with_context(|| format!("data pipeline step {} is invalid", index + 1))?;
        }
        let plan = Self(plan);
        // Exported plans must remain readable within the same recipe-size contract.
        // Escaping pointers can expand a compact source definition significantly.
        if plan.render()?.len() as u64 > MAX_RECIPE {
            bail!(
                "normalized recipe exceeds 64 KiB; shorten literals/pointers or reduce fields/steps"
            );
        }
        Ok(plan)
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

#[cfg(test)]
mod model_plan_tests {
    use super::*;
    #[test]
    fn model_plan_exports_canonical_version_one_definitions_with_literal_values() {
        for args in [
            r#"{"operation":"json-minify"}"#,
            r#"{"steps":[{"operation":"lines-include","contains":"ERROR:中文"},{"operation":"lines-tail","lines":2}]}"#,
        ] {
            let plan = Plan::from_tool(args).unwrap();
            let text = plan.render().unwrap();
            let recipe: Recipe = toml::from_str(&text).unwrap();
            assert_eq!(recipe.version, 1);
            assert_eq!(
                Plan::from_steps(recipe.steps).unwrap().render().unwrap(),
                text
            );
            assert!(text.ends_with('\n'));
        }
    }
    #[test]
    fn model_plan_uses_the_strict_native_argument_and_step_contracts() {
        for args in [
            r#"{"operation":"trim","steps":[]}"#,
            r#"{"operation":"trim","command":"PRIVATE_COMMAND"}"#,
            r#"{"operation":"lines-tail","lines":0}"#,
            r#"{"steps":[]}"#,
            r#"{"operation":"lines-tail","lines":null}"#,
            r#"{"operation":"lines-tail","lines":2,"lines":3}"#,
        ] {
            assert!(Plan::from_tool(args).is_err(), "{args}");
        }
        let args = serde_json::json!({"steps":vec![serde_json::json!({"operation":"trim"});17]})
            .to_string();
        assert!(Plan::from_tool(&args).is_err());
    }
    #[test]
    fn model_plan_enforces_argument_and_normalized_recipe_byte_limits() {
        assert!(
            Plan::from_tool(&format!("{}{{}}", " ".repeat(65537)))
                .err()
                .unwrap()
                .to_string()
                .contains("arguments exceed")
        );
        let step:Step=serde_json::from_value(serde_json::json!({"operation":"json-select","pointer":format!("/{}","\t".repeat(2047))})).unwrap();
        let error = Plan::from_steps(vec![step; 16]).err().unwrap();
        assert!(error.to_string().contains("normalized recipe exceeds"));
    }
}
