//! Explicit literal names and JSON pointers, never expressions or captured input.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_FIELDS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub pointer: String,
}

pub fn parse(values: &[String]) -> Result<Option<Vec<Field>>> {
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() > MAX_FIELDS {
        bail!("jsonl-project requires 1-64 fields");
    }
    values
        .iter()
        .map(|value| {
            let (name, pointer) = value.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("--field requires NAME=POINTER; use NAME= for the root")
            })?;
            Ok(Field {
                name: name.to_owned(),
                pointer: pointer.to_owned(),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
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
        crate::selection::validate_pointer(&field.pointer).map_err(|error| {
            anyhow::anyhow!("invalid pointer in projection field {}: {error}", index + 1)
        })?;
    }
    Ok(())
}
