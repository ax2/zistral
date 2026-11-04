//! Explicit literal names and JSON pointers, never expressions or captured input.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_FIELDS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub pointer: String,
    /// Explicit JSON literal text, used only for an absent path; null is not absence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "literal_default"
    )]
    pub default_json: Option<String>,
}

fn literal_default<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(de).map(Some)
}

pub fn parse(values: &[String], defaults: &[String]) -> Result<Option<Vec<Field>>> {
    if values.is_empty() {
        if !defaults.is_empty() {
            bail!("--field-default requires explicit --field mappings");
        }
        return Ok(None);
    }
    if values.len() > MAX_FIELDS {
        bail!("jsonl-project requires 1-64 fields");
    }
    let mut fields = values
        .iter()
        .map(|value| {
            let (name, pointer) = value.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("--field requires NAME=POINTER; use NAME= for the root")
            })?;
            Ok(Field {
                name: name.to_owned(),
                pointer: pointer.to_owned(),
                default_json: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if defaults.len() > fields.len() {
        bail!("--field-default accepts at most one default per field");
    }
    let mut seen = HashSet::new();
    for value in defaults {
        let (name, literal) = value
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--field-default requires NAME=JSON_LITERAL"))?;
        let field = fields
            .iter_mut()
            .find(|field| field.name == name)
            .context("--field-default must refer to an explicit output field name")?;
        if !seen.insert(name) {
            bail!("--field-default accepts at most one default per field");
        }
        field.default_json = Some(literal.to_owned());
    }
    Ok(Some(fields))
}

pub fn validate(fields: &[Field]) -> Result<()> {
    if fields.is_empty() || fields.len() > MAX_FIELDS {
        bail!("jsonl-project requires 1-64 fields");
    }
    let mut names = HashSet::new();
    for (index, field) in fields.iter().enumerate() {
        if field.name.is_empty()
            || field.name.len() > 256
            || field.name.chars().any(char::is_control)
        {
            bail!(
                "projection field {} name requires 1-256 UTF-8 bytes without control characters",
                index + 1
            );
        }
        if !names.insert(&field.name) {
            bail!("projection output names must be unique");
        }
        if let Some(literal) = &field.default_json {
            if literal.is_empty() || literal.len() > 4096 {
                bail!(
                    "projection field {} default_json requires 1-4096 UTF-8 bytes",
                    index + 1
                );
            }
            crate::selection::validated_record(literal).with_context(|| {
                format!("invalid default_json in projection field {}", index + 1)
            })?;
            // A projected value gains one object container, even before input is read.
            crate::transforms::validate_json(&format!("[{literal}]")).with_context(|| {
                format!(
                    "default_json in projection field {} exceeds supported nesting",
                    index + 1
                )
            })?;
        }
        crate::selection::validate_pointer(&field.pointer).map_err(|error| {
            anyhow::anyhow!("invalid pointer in projection field {}: {error}", index + 1)
        })?;
    }
    Ok(())
}
