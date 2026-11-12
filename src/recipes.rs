//! Explicit local definitions, never implicit project discovery or executable scripts.
use crate::transforms::{self, BoundField, Operation, Step};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

const MAX_RECIPE: u64 = 64 * 1024;
const MAX_PARAMETERS: usize = 16;

// Embed reviewed source examples so installed binaries need no companion recipe files.
const BUILTINS: [(&str, &str, &str); 8] = [
    (
        "cleanup",
        "Trim lines, keep unique lines, sort descending",
        include_str!("../docs/examples/cleanup.toml"),
    ),
    (
        "severity-counts",
        "Count all JSONL records by string /level, emit CSV",
        include_str!("../docs/examples/severity-counts.toml"),
    ),
    (
        "unique-severity-counts",
        "Keep first string /id, count retained /level, emit CSV",
        include_str!("../docs/examples/unique-severity-counts.toml"),
    ),
    (
        "top-severity-counts",
        "Count string /level, rank counts descending, take 20 groups, emit CSV",
        include_str!("../docs/examples/top-severity-counts.toml"),
    ),
    (
        "top-error-codes",
        "Rank string error codes from ERROR records as JSONL (20 groups by default)",
        include_str!("../docs/examples/top-error-codes.toml"),
    ),
    (
        "top-http-5xx-codes",
        "Rank error codes from HTTP 5xx records as JSONL (20 groups by default)",
        include_str!("../docs/examples/top-http-5xx-codes.toml"),
    ),
    (
        "recent-unique-events",
        "Keep first string /id, sort /time descending, take 20 JSONL records",
        include_str!("../docs/examples/recent-unique-events.toml"),
    ),
    (
        "log-context",
        "Keep nearby lines around a literal match, then take the last lines; requires --var needle=VALUE",
        include_str!("../docs/examples/log-context.toml"),
    ),
];

pub fn list() -> String {
    let mut output = String::from("Built-in offline recipes:\n");
    for (name, description, _) in BUILTINS {
        output.push_str(&format!("builtin:{name} — {description}\n"));
    }
    output.push_str("\nParameters: zi data --list-parameters --recipe builtin:NAME\nCopy definition: zi data --export-recipe --recipe builtin:NAME [-o recipe.toml]\nPreview a run: zi data --show-plan --recipe builtin:NAME [--var NAME=VALUE]\nRun: zi data --recipe builtin:NAME -i INPUT [-o OUTPUT]\n");
    output
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe<T = Step> {
    version: u32,
    steps: Vec<T>,
}

#[derive(Deserialize)]
struct RecipeVersion {
    version: u32,
}

#[derive(Deserialize)]
struct TemplateStep {
    #[serde(default)]
    bind: BTreeMap<BoundField, String>,
    #[serde(flatten)]
    arguments: toml::Table,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ParameterType {
    Text,
    LineCount,
}

#[derive(Serialize)]
struct ParameterBinding {
    step: usize,
    field: &'static str,
}

#[derive(Serialize)]
struct Parameter {
    #[serde(rename = "type")]
    value_type: ParameterType,
    required: bool,
    bindings: Vec<ParameterBinding>,
}

impl BoundField {
    fn name(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Contains => "contains",
            Self::Equals => "equals",
            Self::AtLeast => "at_least",
            Self::AtMost => "at_most",
            Self::Context => "context",
            Self::Lines => "lines",
        }
    }

    fn value_type(self) -> ParameterType {
        match self {
            Self::Context | Self::Lines => ParameterType::LineCount,
            _ => ParameterType::Text,
        }
    }

    fn parse_value(self, name: &str, value: &str) -> Result<toml::Value> {
        if self.value_type() == ParameterType::Text {
            return Ok(toml::Value::String(value.to_owned()));
        }
        let count = value
            .parse::<u32>()
            .ok()
            .filter(|_| value.bytes().all(|b| b.is_ascii_digit()))
            .with_context(|| {
                format!("recipe parameter {name} must be an unsigned decimal line count")
            })?;
        Ok(toml::Value::Integer(i64::from(count)))
    }
}

fn parse_recipe<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T> {
    toml::from_str(text).map_err(|error| crate::definition::toml_error(&error, text, "recipe"))
}

fn check_step_count(count: usize) -> Result<()> {
    if !(1..=transforms::MAX_STEPS).contains(&count) {
        bail!("a data pipeline requires 1-16 steps");
    }
    Ok(())
}

fn parameters(steps: &[TemplateStep]) -> Result<BTreeMap<String, Parameter>> {
    use crate::definition::valid_variable;

    check_step_count(steps.len())?;
    let mut names = BTreeMap::new();
    for (index, step) in steps.iter().enumerate() {
        for (&field, name) in &step.bind {
            if !valid_variable(name) {
                bail!(
                    "invalid recipe parameter name in bind; use 1-32 ASCII identifier characters"
                );
            }
            let value_type = field.value_type();
            let parameter = names.entry(name.clone()).or_insert_with(|| Parameter {
                value_type,
                required: false,
                bindings: Vec::new(),
            });
            if parameter.value_type != value_type {
                bail!("recipe parameter {name} cannot bind both text and line counts");
            }
            let fallback = step.arguments.get(field.name());
            if fallback.is_some_and(|value| match value_type {
                ParameterType::Text => !value.is_str(),
                ParameterType::LineCount => !value.is_integer(),
            }) {
                bail!("recipe parameter {name} has an incompatible fallback type");
            }
            parameter.required |= fallback.is_none();
            parameter.bindings.push(ParameterBinding {
                step: index + 1,
                field: field.name(),
            });
        }
    }
    if names.len() > MAX_PARAMETERS {
        bail!("a recipe supports at most {MAX_PARAMETERS} parameters");
    }
    Ok(names)
}

fn decode_step(arguments: toml::Table, index: usize) -> Result<Step> {
    // Converted TOML may contain arbitrary literals. Keep deserialization diagnostics structural.
    toml::Value::Table(arguments)
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid recipe fields or types in step {}", index + 1))
}

fn bind_recipe(steps: Vec<TemplateStep>, variables: &[String]) -> Result<Vec<Step>> {
    use crate::definition::valid_variable;

    let names = parameters(&steps)?;
    if variables.len() > MAX_PARAMETERS {
        bail!("a recipe supports at most {MAX_PARAMETERS} parameters");
    }
    let mut values = BTreeMap::new();
    for argument in variables {
        let (name, value) = argument
            .split_once('=')
            .context("--var requires NAME=VALUE")?;
        if !valid_variable(name) {
            bail!("invalid --var name; use 1-32 ASCII identifier characters");
        }
        if !names.contains_key(name) {
            bail!("unknown recipe parameter {name}");
        }
        if value.len() > 4096 {
            bail!("recipe parameter {name} exceeds 4096 UTF-8 bytes");
        }
        if values.insert(name, value).is_some() {
            bail!("duplicate recipe parameter {name}");
        }
    }
    steps
        .into_iter()
        .enumerate()
        .map(|(index, step)| {
            let mut arguments = step.arguments;
            for (field, name) in step.bind {
                if let Some(&value) = values.get(name.as_str()) {
                    arguments.insert(field.name().to_owned(), field.parse_value(&name, value)?);
                } else if !arguments.contains_key(field.name()) {
                    bail!("missing recipe parameter {name}; use --var {name}=VALUE");
                }
            }
            decode_step(arguments, index)
        })
        .collect()
}

enum RecipeDefinition {
    Literal(Vec<Step>),
    Template(Vec<TemplateStep>),
}

fn read_recipe_source(path: &Path) -> Result<String> {
    let bytes = if let Some(name) = path.to_str().and_then(|text| text.strip_prefix("builtin:")) {
        let (_, _, definition) = BUILTINS
            .iter()
            .find(|(candidate, _, _)| *candidate == name)
            .context("unknown builtin recipe; use zi data --list-recipes for exact names")?;
        definition.as_bytes().to_vec()
    } else {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .with_context(|| format!("cannot open recipe {}", path.display()))?
            .take(MAX_RECIPE + 1)
            .read_to_end(&mut bytes)
            .context("cannot read recipe")?;
        bytes
    };
    if bytes.len() as u64 > MAX_RECIPE {
        bail!("recipe exceeds 64 KiB");
    }
    String::from_utf8(bytes).context("recipe must be UTF-8")
}

fn parse_definition(text: &str) -> Result<RecipeDefinition> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match parse_recipe::<RecipeVersion>(text)?.version {
        1 => Ok(RecipeDefinition::Literal(
            parse_recipe::<Recipe>(text)?.steps,
        )),
        2 => Ok(RecipeDefinition::Template(
            parse_recipe::<Recipe<TemplateStep>>(text)?.steps,
        )),
        _ => bail!("unsupported recipe version; expected version = 1 or 2"),
    }
}

fn declarations(definition: RecipeDefinition) -> Result<(u32, BTreeMap<String, Parameter>)> {
    match definition {
        RecipeDefinition::Literal(steps) => {
            check_step_count(steps.len())?;
            for (index, step) in steps.iter().enumerate() {
                step.validate()
                    .with_context(|| format!("invalid recipe step {}", index + 1))?;
            }
            Ok((1, BTreeMap::new()))
        }
        RecipeDefinition::Template(steps) => {
            let parameters = parameters(&steps)?;
            for (index, step) in steps.into_iter().enumerate() {
                decode_step(step.arguments, index)?
                    .validate_with_bindings(|field| step.bind.contains_key(&field))
                    .with_context(|| format!("invalid recipe step {}", index + 1))?;
            }
            Ok((2, parameters))
        }
    }
}

pub fn export_recipe(path: &Path) -> Result<String> {
    let source = read_recipe_source(path)?;
    declarations(parse_definition(&source)?)?;
    Ok(source)
}

pub fn list_parameters(path: &Path) -> Result<String> {
    let (recipe_version, parameters) = declarations(parse_definition(&read_recipe_source(path)?)?)?;
    let mut output = serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "recipe_version": recipe_version,
        "parameters": parameters,
    }))?;
    output.push('\n');
    Ok(output)
}

pub struct Plan(Vec<Step>);

impl Plan {
    pub fn prepare(
        single: Option<Step>,
        steps: &[String],
        recipe: Option<&Path>,
        variables: &[String],
    ) -> Result<Self> {
        let plan = if let Some(step) = single {
            vec![step]
        } else if let Some(path) = recipe {
            match parse_definition(&read_recipe_source(path)?)? {
                RecipeDefinition::Literal(_) if !variables.is_empty() => {
                    bail!("--var requires a version-2 data recipe")
                }
                RecipeDefinition::Literal(steps) => steps,
                RecipeDefinition::Template(steps) => bind_recipe(steps, variables)?,
            }
        } else {
            if steps.len() > transforms::MAX_STEPS {
                bail!("a data pipeline requires 1-16 steps");
            }
            steps.iter().map(|text| {
                for (prefix, operation) in [("lines-head:", Operation::LinesHead), ("lines-tail:", Operation::LinesTail)] {
                    if let Some(count) = text.strip_prefix(prefix) {
                        let lines = count.parse::<usize>().map_err(|_| anyhow::anyhow!("line window count must be an integer from 1 to 100000"))?;
                        return Ok(Step { context: None, at_least: None, at_most: None, if_present: None, equals: None, operation, reverse: false, pointer: None, fields: None, contains: None, lines: Some(lines) });
                    }
                }
                for (prefix, operation) in [("json-redact:", Operation::JsonRedact), ("jsonl-redact:", Operation::JsonlRedact), ("json-select:", Operation::JsonSelect), ("jsonl-select:", Operation::JsonlSelect), ("jsonl-dedupe:", Operation::JsonlDedupe), ("jsonl-sort:", Operation::JsonlSort), ("jsonl-sort-int:", Operation::JsonlSortInt), ("jsonl-count-by:", Operation::JsonlCountBy)] {
                    if let Some(pointer) = text.strip_prefix(prefix) {
                        return Ok(Step { context: None, at_least: None, at_most: None, if_present: None, equals: None, operation, reverse: false, pointer: Some(pointer.to_owned()), fields: None, contains: None, lines: None });
                    }
                }
                for (prefix, operation) in [("lines-include:", Operation::LinesInclude), ("lines-exclude:", Operation::LinesExclude)] {
                    if let Some(contains) = text.strip_prefix(prefix) {
                        return Ok(Step { context: None, at_least: None, at_most: None, if_present: None, equals: None, operation, reverse: false, pointer: None, fields: None, contains: Some(contains.to_owned()), lines: None });
                    }
                }
                let (name, reverse) = text.strip_suffix(":reverse")
                    .map_or((text.as_str(), false), |name| (name, true));
                let operation = Operation::from_str(name, false).map_err(|_| {
                    anyhow::anyhow!("unsupported --step; see zi data --help for native operations (sort:reverse is also supported)")
                })?;
                Ok(Step { context: None, at_least: None, at_most: None, if_present: None, equals: None, operation, reverse, pointer: None, fields: None, contains: None, lines: None })
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
        check_step_count(plan.len())?;
        for (index, step) in plan.iter().enumerate() {
            step.validate().with_context(|| {
                format!(
                    "data pipeline step {} is invalid ({})",
                    index + 1,
                    step.operation
                )
            })?;
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
