//! Deterministic transformations of explicit input. No model, filesystem or shell access.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize, Serialize,
    de::{IgnoredAny, MapAccess, Visitor},
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::{self, Write},
};

const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;
pub const CLI_OUTPUT: usize = 16 * 1024 * 1024;
pub const MAX_STEPS: usize = 16;
const TOOL_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    /// Keep the first explicit number of lines, preserving order and whitespace
    LinesHead,
    /// Keep the last explicit number of lines, preserving order and whitespace
    LinesTail,
    /// Trim Unicode whitespace from each line; retain blank lines
    Trim,
    /// Keep the first occurrence of each exact line, preserving order
    Dedupe,
    /// Sort lines by case-sensitive Unicode order; retain duplicates
    Sort,
    /// Keep lines containing an explicit case-sensitive literal substring
    LinesInclude,
    /// Keep lines not containing an explicit case-sensitive literal substring
    LinesExclude,
    /// Pretty-print JSON without changing number/string tokens or key order
    JsonFormat,
    /// Remove only insignificant JSON whitespace
    JsonMinify,
    /// List root object keys in source order, including repeated keys
    JsonKeys,
    /// Collect one JSON value per line into an array; reject blank/invalid records
    JsonlToArray,
    /// Emit each root array item as one compact JSON line, preserving numeric tokens
    ArrayToJsonl,
    /// Select a JSON value using an explicit RFC 6901 pointer; retain quoted strings
    JsonSelect,
    /// Select one value from every JSONL record; emit compact JSON lines in input order
    JsonlSelect,
    /// Keep complete JSONL records whose selected JSON string equals a literal
    JsonlWhere,
    /// Keep the first whole JSONL record for each decoded string at an explicit pointer
    JsonlDedupe,
    /// Stably sort whole JSONL records by a decoded string field; reverse changes key order
    JsonlSort,
    /// Count JSONL records per decoded string field, in first-seen key order
    JsonlCountBy,
    /// Convert header-based comma CSV to JSONL objects; keep every cell as a string
    CsvToJsonl,
    /// Export flat scalar JSONL objects as quoted CSV; first object defines column order
    JsonlToCsv,
    /// Select and rename multiple values into one ordered JSON object per JSONL record
    JsonlProject,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            clap::ValueEnum::to_possible_value(self)
                .expect("native operation has a CLI name")
                .get_name(),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_string"
    )]
    pub pointer: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_string"
    )]
    pub contains: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_string"
    )]
    pub equals: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_lines"
    )]
    pub lines: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "projection_fields"
    )]
    pub fields: Option<Vec<crate::projection::Field>>,
}

fn projection_fields<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<Vec<crate::projection::Field>>, D::Error> {
    Vec::<crate::projection::Field>::deserialize(de).map(Some)
}

fn optional_string<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(de).map(Some)
}

fn optional_lines<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<usize>, D::Error> {
    usize::deserialize(de).map(Some)
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Step {
    pub fn validate(&self) -> Result<()> {
        match (&self.equals, self.operation) {
            (Some(value), Operation::JsonlWhere) if value.len() <= 4096 => (),
            (Some(_), Operation::JsonlWhere) => {
                bail!("equals must contain at most 4096 UTF-8 bytes")
            }
            (None, Operation::JsonlWhere) => bail!(
                "jsonl-where requires explicit equals text; use --equals TEXT or recipe equals (empty is allowed)"
            ),
            (Some(_), _) => bail!("equals is only valid for jsonl-where"),
            (None, _) => (),
        }
        match (self.lines, self.operation) {
            (Some(n), Operation::LinesHead | Operation::LinesTail)
                if (1..=MAX_ITEMS).contains(&n) => {}
            (Some(_), Operation::LinesHead | Operation::LinesTail) => {
                bail!("lines must be an integer from 1 to 100000")
            }
            (None, Operation::LinesHead | Operation::LinesTail) => bail!(
                "line windows require explicit lines; use --lines N or step lines-head:N / lines-tail:N"
            ),
            (Some(_), _) => bail!("lines is only valid for lines-head or lines-tail"),
            (None, _) => (),
        }
        if self.reverse && !matches!(self.operation, Operation::Sort | Operation::JsonlSort) {
            bail!("--reverse is only valid for sort or jsonl-sort");
        }
        match (&self.contains, self.operation) {
            (Some(needle), Operation::LinesInclude | Operation::LinesExclude) => {
                if needle.is_empty() || needle.len() > 4096 || needle.contains(['\r', '\n']) {
                    bail!("contains requires 1-4096 UTF-8 bytes without CR or LF");
                }
            }
            (None, Operation::LinesInclude | Operation::LinesExclude) => {
                bail!(
                    "line filtering requires explicit contains text; use --contains TEXT or recipe contains"
                );
            }
            (Some(_), _) => bail!("contains is only valid for lines-include or lines-exclude"),
            (None, _) => (),
        }
        match (&self.fields, self.operation) {
            (Some(fields), Operation::JsonlProject) => crate::projection::validate(fields)?,
            (None, Operation::JsonlProject) => bail!(
                "jsonl-project requires explicit fields; use --field NAME=POINTER or recipe fields"
            ),
            (Some(_), _) => bail!("fields are only valid for jsonl-project"),
            (None, _) => (),
        }
        match (&self.pointer, self.operation) {
            (
                Some(pointer),
                Operation::JsonSelect
                | Operation::JsonlSelect
                | Operation::JsonlWhere
                | Operation::JsonlDedupe
                | Operation::JsonlSort
                | Operation::JsonlCountBy,
            ) => crate::selection::validate_pointer(pointer),
            (
                None,
                Operation::JsonSelect
                | Operation::JsonlSelect
                | Operation::JsonlWhere
                | Operation::JsonlDedupe
                | Operation::JsonlSort
                | Operation::JsonlCountBy,
            ) => {
                bail!(
                    "{} requires an explicit pointer; use an empty pointer to select the root",
                    if self.operation == Operation::JsonSelect {
                        "json-select"
                    } else if self.operation == Operation::JsonlSelect {
                        "jsonl-select"
                    } else if self.operation == Operation::JsonlWhere {
                        "jsonl-where"
                    } else if self.operation == Operation::JsonlDedupe {
                        "jsonl-dedupe"
                    } else if self.operation == Operation::JsonlSort {
                        "jsonl-sort"
                    } else {
                        "jsonl-count-by"
                    }
                )
            }
            (Some(_), _) => {
                bail!(
                    "pointer is only valid for JSON selection, jsonl-where, jsonl-dedupe, jsonl-sort or jsonl-count-by"
                )
            }
            (None, _) => Ok(()),
        }
    }
}

struct Buffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Buffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    fn newline(&mut self, depth: usize) -> io::Result<()> {
        self.write_all(b"\n")?;
        self.write_all(&[b' '; 256][..depth * 2])
    }
    fn finish(self) -> String {
        String::from_utf8(self.bytes).expect("UTF-8 input and ASCII formatting")
    }
}
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(format!(
                "transform output exceeds {} bytes",
                self.limit
            )));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn validate_json(input: &str) -> Result<()> {
    // IgnoredAny validates syntax without converting numbers or constructing a Value.
    // Check nesting first: its skip parser uses an explicit stack rather than Value's depth limit.
    let (mut string, mut escaped, mut depth) = (false, false, 0usize);
    for byte in input.bytes() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 128 {
                        bail!("JSON nesting exceeds 128 containers");
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => (),
            }
        }
    }
    let mut de = serde_json::Deserializer::from_str(input);
    IgnoredAny::deserialize(&mut de)
        .and_then(|_| de.end())
        .map_err(|e| {
            anyhow::anyhow!(
                "input is not valid JSON at line {}, column {}",
                e.line(),
                e.column()
            )
        })
}

pub(crate) fn compact_valid_json(input: &str, limit: usize) -> Result<String> {
    let mut output = Buffer::new(limit);
    format_json(input, false, &mut output)?;
    Ok(output.finish())
}

pub(crate) fn compact_json(input: &str, limit: usize) -> Result<String> {
    validate_json(input)?;
    compact_valid_json(input, limit)
}

fn format_json(input: &str, pretty: bool, out: &mut Buffer) -> Result<()> {
    let bytes = input.as_bytes();
    let (mut string, mut escaped, mut depth, mut previous) = (false, false, 0usize, 0u8);
    for (index, &byte) in bytes.iter().enumerate() {
        if string {
            out.write_all(&[byte])?;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
                previous = b'"';
            }
            continue;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        match byte {
            b'"' => {
                out.write_all(&[byte])?;
                string = true;
            }
            b'{' | b'[' => {
                out.write_all(&[byte])?;
                depth += 1;
                let closing = if byte == b'{' { b'}' } else { b']' };
                if pretty
                    && bytes[index + 1..].iter().find(|b| !b.is_ascii_whitespace())
                        != Some(&closing)
                {
                    out.newline(depth)?;
                }
            }
            b'}' | b']' => {
                depth -= 1; // Syntax and depth were validated before formatting.
                if pretty && !matches!(previous, b'{' | b'[') {
                    out.newline(depth)?;
                }
                out.write_all(&[byte])?;
            }
            b',' => {
                out.write_all(b",")?;
                if pretty {
                    out.newline(depth)?;
                }
            }
            b':' => {
                out.write_all(if pretty { b": " } else { b":" })?;
            }
            _ => out.write_all(&[byte])?,
        }
        previous = byte;
    }
    Ok(())
}

fn jsonl_to_array(input: &str, out: &mut Buffer) -> Result<()> {
    out.write_all(b"[")?;
    for (index, line) in input.lines().enumerate() {
        if index == MAX_ITEMS {
            bail!("JSONL conversion accepts at most 100000 records");
        }
        validate_json(line).with_context(|| format!("invalid JSONL record {}", index + 1))?;
        if index != 0 {
            out.write_all(b",")?;
        }
        format_json(line, false, out)?;
    }
    out.write_all(b"]")?;
    // Wrapping records adds one container. Never publish an array beyond the common depth cap.
    validate_json(std::str::from_utf8(&out.bytes).expect("UTF-8 formatting"))
        .context("collected JSON array exceeds supported nesting")
}

fn jsonl_where(input: &str, pointer: &str, equals: &str, out: &mut Buffer) -> Result<()> {
    for (index, line) in input.lines().enumerate() {
        if index == MAX_ITEMS {
            bail!("JSONL filtering accepts at most 100000 records");
        }
        let matched = crate::selection::string_equals(line, pointer, equals)
            .with_context(|| format!("cannot filter JSONL record {}", index + 1))?;
        if matched {
            format_json(line, false, out)?;
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

/// Keys borrow no JSON values other than decoded strings; record payloads remain lexical text.
fn jsonl_by_key(input: &str, step: &Step, out: &mut Buffer) -> Result<()> {
    let pointer = step.pointer.as_deref().expect("validated pointer");
    let records = input.lines().enumerate().map(|(index, line)| -> Result<_> {
        if index == MAX_ITEMS {
            bail!("JSONL key operations accept at most 100000 records");
        }
        let key = crate::selection::string_key(line, pointer).with_context(|| {
            format!(
                "cannot read JSONL key at record {} ({})",
                index + 1,
                step.operation
            )
        })?;
        Ok((key, line))
    });
    match step.operation {
        Operation::JsonlCountBy => {
            let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
            for (index, record) in records.enumerate() {
                let (key, _) = record?;
                let (count, _) = counts.entry(key).or_insert((0, index));
                *count += 1;
            }
            // Store each key once, then restore first-seen order after hash aggregation.
            let mut groups: Vec<_> = counts.into_iter().collect();
            groups.sort_unstable_by_key(|(_, (_, first))| *first);
            for (key, (count, _)) in groups {
                out.write_all(b"{\"value\":")?;
                serde_json::to_writer(&mut *out, &key)
                    .context("cannot encode JSONL group within output limit")?;
                writeln!(out, ",\"count\":{count}}}")?;
            }
        }
        Operation::JsonlSort => {
            let mut records = records.collect::<Result<Vec<_>>>()?;
            // Reverse the comparison, not the result, so ties retain input order in both directions.
            records.sort_by(|a, b| {
                if step.reverse {
                    b.0.cmp(&a.0)
                } else {
                    a.0.cmp(&b.0)
                }
            });
            for (_, line) in records {
                format_json(line, false, out)?;
                out.write_all(b"\n")?;
            }
        }
        Operation::JsonlDedupe => {
            let mut seen = HashSet::new();
            for record in records {
                let (key, line) = record?;
                if seen.insert(key) {
                    format_json(line, false, out)?;
                    out.write_all(b"\n")?;
                }
            }
        }
        _ => unreachable!("JSONL key operation"),
    }
    Ok(())
}

fn jsonl_select(input: &str, pointer: &str, out: &mut Buffer) -> Result<()> {
    for (index, line) in input.lines().enumerate() {
        if index == MAX_ITEMS {
            bail!("JSONL selection accepts at most 100000 records");
        }
        let remaining = out.limit.saturating_sub(out.bytes.len()).saturating_sub(1);
        let selected = crate::selection::select_json(line, pointer, remaining)
            .with_context(|| format!("cannot select JSONL record {}", index + 1))?;
        out.write_all(selected.as_bytes())?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn jsonl_project(input: &str, fields: &[crate::projection::Field], out: &mut Buffer) -> Result<()> {
    for (index, line) in input.lines().enumerate() {
        if index == MAX_ITEMS {
            bail!("JSONL projection accepts at most 100000 records");
        }
        let start = out.bytes.len();
        out.write_all(b"{")?;
        for (position, field) in fields.iter().enumerate() {
            if position != 0 {
                out.write_all(b",")?;
            }
            serde_json::to_writer(&mut *out, &field.name)
                .context("cannot encode projection name within output limit")?;
            out.write_all(b":")?;
            let selected = crate::selection::select_json(
                line,
                &field.pointer,
                out.limit.saturating_sub(out.bytes.len()),
            )
            .with_context(|| {
                format!(
                    "cannot project JSONL record {}, field {}",
                    index + 1,
                    position + 1
                )
            })?;
            out.write_all(selected.as_bytes())?;
        }
        out.write_all(b"}\n")?;
        // The new object adds one container even when a pointer selects the whole record.
        validate_json(std::str::from_utf8(&out.bytes[start..]).expect("UTF-8 JSON formatting"))
            .with_context(|| {
                format!(
                    "projected JSONL record {} exceeds supported nesting",
                    index + 1
                )
            })?;
    }
    Ok(())
}

fn array_to_jsonl(input: &str, out: &mut Buffer) -> Result<()> {
    validate_json(input)?;
    let input = input.trim();
    if !input.starts_with('[') {
        bail!("array-to-jsonl requires a root JSON array");
    }
    let inner = &input[1..input.len() - 1];
    if inner.trim().is_empty() {
        return Ok(());
    }
    let (mut string, mut escaped, mut depth, mut start, mut count) =
        (false, false, 0usize, 0usize, 0usize);
    // Syntax was checked first. Only commas outside strings/nested containers split items.
    for (index, byte) in inner.bytes().chain(std::iter::once(b',')).enumerate() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
            continue;
        }
        match byte {
            b'"' => string = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                if count == MAX_ITEMS {
                    bail!("JSONL conversion accepts at most 100000 records");
                }
                format_json(&inner[start..index], false, out)?;
                out.write_all(b"\n")?;
                count += 1;
                start = index + 1;
            }
            _ => (),
        }
    }
    Ok(())
}

struct Keys;
impl<'de> Visitor<'de> for Keys {
    type Value = Vec<String>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a root JSON object")
    }
    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let mut keys = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if keys.len() == MAX_ITEMS {
                return Err(serde::de::Error::custom("too many root keys"));
            }
            keys.push(key);
            map.next_value::<IgnoredAny>()?;
        }
        Ok(keys)
    }
}

#[cfg(test)]
fn run(input: &str, operation: Operation, reverse: bool, limit: usize) -> Result<String> {
    transform(
        input,
        &Step {
            operation,
            reverse,
            contains: None,
            lines: None,
            equals: None,
            pointer: None,
            fields: None,
        },
        MAX_INPUT,
        limit,
    )
}

/// Buffer only the current/next stage. Never emit an intermediate result.
pub fn run_pipeline(input: &str, steps: &[Step]) -> Result<String> {
    pipeline(input, steps, CLI_OUTPUT)
}

fn pipeline(input: &str, steps: &[Step], final_limit: usize) -> Result<String> {
    if steps.is_empty() || steps.len() > MAX_STEPS {
        bail!("a data pipeline requires 1-16 steps");
    }
    for (index, step) in steps.iter().enumerate() {
        step.validate().with_context(|| {
            format!(
                "data pipeline step {} is invalid ({})",
                index + 1,
                step.operation
            )
        })?;
    }
    let mut current = String::new();
    for (index, step) in steps.iter().enumerate() {
        let source = if index == 0 { input } else { &current };
        let input_limit = if index == 0 { MAX_INPUT } else { CLI_OUTPUT };
        let limit = if index + 1 == steps.len() {
            final_limit
        } else {
            CLI_OUTPUT
        };
        current = transform(source, step, input_limit, limit).with_context(|| {
            format!(
                "data pipeline step {} failed ({})",
                index + 1,
                step.operation
            )
        })?;
    }
    Ok(current)
}

fn transform(input: &str, step: &Step, input_limit: usize, limit: usize) -> Result<String> {
    if input.len() > input_limit {
        bail!("data transformation input exceeds {input_limit} bytes");
    }
    step.validate()?;
    let operation = step.operation;
    let mut out = Buffer::new(limit);
    match operation {
        Operation::LinesHead
        | Operation::LinesTail
        | Operation::Trim
        | Operation::Dedupe
        | Operation::Sort
        | Operation::LinesInclude
        | Operation::LinesExclude => {
            let mut lines: Vec<_> = input.lines().take(MAX_ITEMS + 1).collect();
            if lines.len() > MAX_ITEMS {
                bail!("line transformations accept at most 100000 lines");
            }
            match operation {
                Operation::LinesHead => lines.truncate(step.lines.expect("validated lines")),
                Operation::LinesTail => {
                    let first = lines
                        .len()
                        .saturating_sub(step.lines.expect("validated lines"));
                    lines.drain(..first);
                }
                Operation::Trim => lines.iter_mut().for_each(|line| *line = line.trim()),
                Operation::Dedupe => {
                    let mut seen = HashSet::new();
                    lines.retain(|line| seen.insert(*line));
                }
                Operation::LinesInclude | Operation::LinesExclude => {
                    let needle = step.contains.as_deref().expect("validated contains text");
                    lines.retain(|line| {
                        line.contains(needle) == (operation == Operation::LinesInclude)
                    });
                }
                Operation::Sort => {
                    lines.sort_unstable();
                    if step.reverse {
                        lines.reverse();
                    }
                }
                _ => unreachable!(),
            }
            for line in lines {
                out.write_all(line.as_bytes())?;
                out.write_all(b"\n")?;
            }
        }
        Operation::JsonFormat | Operation::JsonMinify | Operation::JsonKeys => {
            validate_json(input)?;
            if operation == Operation::JsonKeys {
                use serde::Deserializer;
                let keys = serde_json::Deserializer::from_str(input)
                    .deserialize_map(Keys)
                    .map_err(|_| {
                        anyhow::anyhow!("json-keys requires a root object with at most 100000 keys")
                    })?;
                serde_json::to_writer(&mut out, &keys)
                    .context("cannot serialize JSON keys within output limit")?;
            } else {
                format_json(input, operation == Operation::JsonFormat, &mut out)?;
            }
        }
        Operation::JsonlToArray => jsonl_to_array(input, &mut out)?,
        Operation::ArrayToJsonl => array_to_jsonl(input, &mut out)?,
        Operation::CsvToJsonl => crate::csv_input::to_jsonl(input, &mut out, MAX_ITEMS)?,
        Operation::JsonlToCsv => crate::csv_output::from_jsonl(input, &mut out, MAX_ITEMS)?,
        Operation::JsonlProject => jsonl_project(
            input,
            step.fields.as_deref().expect("validated fields"),
            &mut out,
        )?,
        Operation::JsonlWhere => jsonl_where(
            input,
            step.pointer.as_deref().expect("validated pointer"),
            step.equals.as_deref().expect("validated equals"),
            &mut out,
        )?,
        Operation::JsonlDedupe | Operation::JsonlSort | Operation::JsonlCountBy => {
            jsonl_by_key(input, step, &mut out)?
        }
        Operation::JsonlSelect => jsonl_select(
            input,
            step.pointer.as_deref().expect("validated pointer"),
            &mut out,
        )?,
        Operation::JsonSelect => {
            return crate::selection::select_json(
                input,
                step.pointer.as_deref().expect("validated pointer"),
                limit,
            );
        }
    }
    Ok(out.finish())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolSteps {
    pub steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolArguments {
    Single(Step),
    Pipeline(ToolSteps),
}

pub(crate) fn parse_arguments(arguments: &str) -> Result<ToolArguments> {
    if arguments.len() > 64 * 1024 {
        bail!("transform tool arguments exceed 64 KiB");
    }
    serde_json::from_str(arguments).map_err(|_| {
        anyhow::anyhow!(
            "transform_input requires either operation with optional reverse/pointer/fields/contains/lines/equals, or 1-16 steps of the same shape; do not combine them"
        )
    })
}

pub fn execute(arguments: &str, input: &str) -> Result<String> {
    let output = match parse_arguments(arguments)? {
        ToolArguments::Single(step) => transform(input, &step, MAX_INPUT, TOOL_OUTPUT)?,
        ToolArguments::Pipeline(args) => pipeline(input, &args.steps, TOOL_OUTPUT)?,
    };
    let result = serde_json::to_string(&serde_json::json!({"output":output}))?;
    if result.len() > TOOL_OUTPUT {
        bail!("transform tool result exceeds 64 KiB; reduce input or use zi data locally");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_preserves_selected_values_and_literal_ordered_names() {
        let args = r#"{"operation":"jsonl-project","fields":[{"name":"编号","pointer":"/payload/id"},{"name":"a\"\\","pointer":"/text"},{"name":"nested","pointer":"/obj"},{"name":"nullable","pointer":"/nil"}]}"#;
        let input = r#"{"obj":{"z":1,"a":2,"z":3},"payload":{"id":1.2300E+400},"text":"\u4e2d\n","nil":null}"#;
        let result: serde_json::Value =
            serde_json::from_str(&execute(args, input).unwrap()).unwrap();
        assert_eq!(
            result["output"],
            "{\"编号\":1.2300E+400,\"a\\\"\\\\\":\"\\u4e2d\\n\",\"nested\":{\"z\":1,\"a\":2,\"z\":3},\"nullable\":null}\n"
        );
        let root = r#"{"operation":"jsonl-project","fields":[{"name":"whole","pointer":""}]}"#;
        let values: serde_json::Value =
            serde_json::from_str(&execute(root, "-0\r\n\"\\u4e2d\"\nnull\n").unwrap()).unwrap();
        assert_eq!(
            values["output"],
            "{\"whole\":-0}\n{\"whole\":\"\\u4e2d\"}\n{\"whole\":null}\n"
        );
        let dependent = r#"{"operation":"jsonl-project","fields":[{"name":"id","pointer":"/payload/id"},{"name":"copy","pointer":"/id"}]}"#;
        let error = format!(
            "{:#}",
            execute(dependent, r#"{"payload":{"id":1}}"#).unwrap_err()
        );
        assert!(error.contains("record 1, field 2"));
        for bad in [
            r#"{"payload":{"id":1,"id":2},"text":"PRIVATE"}"#,
            r#"{"payload":{"id":1},"text":"PRIVATE","bad":[1,]}"#,
        ] {
            let error = format!("{:#}", execute(args, bad).unwrap_err());
            assert!(!error.contains("PRIVATE"));
            assert!(error.contains("record 1"));
        }
    }
    #[test]
    fn projection_enforces_definition_record_depth_and_output_bounds() {
        let field = crate::projection::Field {
            name: "x".into(),
            pointer: "".into(),
        };
        let step = Step {
            operation: Operation::JsonlProject,
            reverse: false,
            contains: None,
            lines: None,
            equals: None,
            pointer: None,
            fields: Some(vec![field.clone()]),
        };
        assert_eq!(transform("", &step, MAX_INPUT, CLI_OUTPUT).unwrap(), "");
        assert_eq!(
            transform(&"0\n".repeat(MAX_ITEMS), &step, MAX_INPUT, CLI_OUTPUT)
                .unwrap()
                .lines()
                .count(),
            MAX_ITEMS
        );
        assert!(transform(&"0\n".repeat(MAX_ITEMS + 1), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        assert!(transform("1", &step, MAX_INPUT, 4).is_err());
        let nested = |depth| format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert!(transform(&nested(127), &step, MAX_INPUT, CLI_OUTPUT).is_ok());
        assert!(transform(&nested(128), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        let long = serde_json::to_string(&"\\".repeat(20000)).unwrap();
        assert!(
            transform(&long, &step, MAX_INPUT, TOOL_OUTPUT)
                .unwrap()
                .len()
                < TOOL_OUTPUT
        );
        assert!(
            execute(
                r#"{"operation":"jsonl-project","fields":[{"name":"x","pointer":""}]}"#,
                &long
            )
            .is_err()
        );
        let mut many = step.clone();
        many.fields = Some(
            (0..64)
                .map(|i| crate::projection::Field {
                    name: format!("f{i}"),
                    ..field.clone()
                })
                .collect(),
        );
        assert!(many.validate().is_ok());
        many.fields
            .as_mut()
            .unwrap()
            .push(crate::projection::Field {
                name: "extra".into(),
                ..field.clone()
            });
        assert!(many.validate().is_err());
        for fields in [
            vec![],
            vec![field.clone(), field.clone()],
            vec![crate::projection::Field {
                name: "界".repeat(86),
                ..field.clone()
            }],
            vec![crate::projection::Field {
                name: "PRIVATE\n".into(),
                ..field.clone()
            }],
            vec![crate::projection::Field {
                pointer: "/PRIVATE~2".into(),
                ..field.clone()
            }],
        ] {
            let mut bad = step.clone();
            bad.fields = Some(fields);
            let error = format!("{:#}", bad.validate().unwrap_err());
            assert!(!error.contains("PRIVATE"));
        }
        let mut boundary = step.clone();
        boundary.fields.as_mut().unwrap()[0].name = "界".repeat(85) + "x";
        assert!(boundary.validate().is_ok());
        for args in [
            r#"{"operation":"trim","fields":[]}"#,
            r#"{"operation":"trim","fields":null}"#,
            r#"{"operation":"jsonl-project","fields":null}"#,
            r#"{"operation":"jsonl-project","fields":[{"name":"x"}]}"#,
            r#"{"operation":"jsonl-project","pointer":"","fields":[{"name":"x","pointer":""}]}"#,
            r#"{"steps":[{"operation":"trim"}],"fields":[]}"#,
        ] {
            assert!(execute(args, "").is_err());
        }
    }
    #[test]
    fn csv_export_enforces_record_depth_output_and_serialized_envelope_bounds() {
        let step = Step {
            operation: Operation::JsonlToCsv,
            reverse: false,
            contains: None,
            lines: None,
            equals: None,
            pointer: None,
            fields: None,
        };
        let input = "{\"x\":0}\n".repeat(MAX_ITEMS);
        let output = transform(&input, &step, MAX_INPUT, CLI_OUTPUT).unwrap();
        assert_eq!(output.lines().count(), MAX_ITEMS + 1);
        assert!(transform(&(input + "{\"x\":0}\n"), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        assert!(transform("{\"x\":1}", &step, MAX_INPUT, 3).is_err());
        let escaped = serde_json::json!({"x":"\"".repeat(20000)}).to_string();
        assert!(
            transform(&escaped, &step, MAX_INPUT, TOOL_OUTPUT)
                .unwrap()
                .len()
                < TOOL_OUTPUT
        );
        assert!(execute(r#"{"operation":"jsonl-to-csv"}"#, &escaped).is_err());
        let deep = format!("{{\"x\":{}{}}}", "[".repeat(128), "]".repeat(128));
        assert!(
            format!(
                "{:#}",
                transform(&deep, &step, MAX_INPUT, CLI_OUTPUT).unwrap_err()
            )
            .contains("128")
        );
    }
    #[test]
    fn csv_limits_keep_record_counts_and_serialized_tool_envelope_bounded() {
        let step = Step {
            operation: Operation::CsvToJsonl,
            reverse: false,
            contains: None,
            lines: None,
            equals: None,
            pointer: None,
            fields: None,
        };
        let input = format!("h\n{}", "x\n".repeat(MAX_ITEMS));
        assert_eq!(
            transform(&input, &step, MAX_INPUT, CLI_OUTPUT)
                .unwrap()
                .lines()
                .count(),
            MAX_ITEMS
        );
        assert!(transform(&(input + "x\n"), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        assert!(transform("h\nx", &step, MAX_INPUT, 5).is_err());
        let escaped = format!("h\n{}", "\\".repeat(20000));
        let raw = transform(&escaped, &step, MAX_INPUT, TOOL_OUTPUT).unwrap();
        assert!(raw.len() < TOOL_OUTPUT);
        assert!(execute(r#"{"operation":"csv-to-jsonl"}"#, &escaped).is_err());
    }
    #[test]
    fn jsonl_selection_preserves_values_and_enforces_shared_bounds() {
        let args = r#"{"operation":"jsonl-select","pointer":"/x"}"#;
        let input = "{\"x\":123456789012345678901234567890}\r\n{\"x\":1.2300E+400}\n{\"x\":-0}\n{\"x\":\"\\u4e2d\\n\"}\n{\"x\":{\"z\":1,\"a\":2,\"z\":3}}";
        let result: serde_json::Value =
            serde_json::from_str(&execute(args, input).unwrap()).unwrap();
        assert_eq!(
            result["output"],
            "123456789012345678901234567890\n1.2300E+400\n-0\n\"\\u4e2d\\n\"\n{\"z\":1,\"a\":2,\"z\":3}\n"
        );
        let step = Step {
            operation: Operation::JsonlSelect,
            reverse: false,
            contains: None,
            lines: None,
            equals: None,
            pointer: Some(String::new()),
            fields: None,
        };
        assert_eq!(transform("", &step, MAX_INPUT, CLI_OUTPUT).unwrap(), "");
        assert_eq!(
            transform("null\nfalse\n[1,2]\n", &step, MAX_INPUT, CLI_OUTPUT).unwrap(),
            "null\nfalse\n[1,2]\n"
        );
        let depth128 = format!("{}0{}", "[".repeat(128), "]".repeat(128));
        assert!(transform(&depth128, &step, MAX_INPUT, CLI_OUTPUT).is_ok());
        assert!(transform(&format!("[{depth128}]"), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        let records = "null\n".repeat(MAX_ITEMS);
        assert_eq!(
            transform(&records, &step, MAX_INPUT, CLI_OUTPUT).unwrap(),
            records
        );
        assert!(transform(&(records + "null"), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        assert!(transform("null\n", &step, MAX_INPUT, 4).is_err());
        assert_eq!(transform("null", &step, MAX_INPUT, 5).unwrap(), "null\n");
        let escaped = serde_json::json!({"x":"\\".repeat(17000)}).to_string();
        assert!(execute(args, &escaped).is_err()); // Serialized envelope also counts.
    }

    #[test]
    fn jsonl_selection_errors_identify_records_without_echoing_data_or_pointers() {
        for record in [
            "{}",
            "{\"x\":1,\"x\":2}",
            "PRIVATE_RECORD",
            "{\"x\":1} trailing",
            "",
            "\u{feff}{\"x\":1}",
        ] {
            let error = format!(
                "{:#}",
                execute(
                    r#"{"operation":"jsonl-select","pointer":"/x"}"#,
                    &format!("{{\"x\":\"PRIVATE_VALUE\"}}\n{record}\n")
                )
                .unwrap_err()
            );
            assert!(error.contains("record 2"), "{error}");
            assert!(!error.contains("PRIVATE"), "{error}");
        }
        for args in [
            r#"{"operation":"jsonl-select"}"#,
            r#"{"operation":"jsonl-select","pointer":null}"#,
            r#"{"operation":"jsonl-select","pointer":"PRIVATE_POINTER"}"#,
            r#"{"operation":"jsonl-select","pointer":"/x~2"}"#,
            r#"{"operation":"jsonl-select","pointer":"","reverse":true}"#,
        ] {
            assert!(
                !format!("{:#}", execute(args, "PRIVATE_INPUT").unwrap_err()).contains("PRIVATE")
            );
        }
    }

    #[test]
    fn selection_steps_validate_before_data_and_retain_tool_output_limits() {
        for args in [
            r#"{"operation":"json-select"}"#,
            r#"{"operation":"json-select","pointer":null}"#,
            r#"{"operation":"json-select","pointer":"PRIVATE_PATH"}"#,
            r#"{"operation":"json-select","pointer":"/x~2"}"#,
            r#"{"operation":"json-select","pointer":"","reverse":true}"#,
            r#"{"operation":"trim","pointer":""}"#,
            r#"{"steps":[{"operation":"trim"}],"pointer":""}"#,
        ] {
            let error = format!("{:#}", execute(args, "PRIVATE_INPUT").unwrap_err());
            assert!(!error.contains("PRIVATE"), "{error}");
        }
        for pointer in [format!("/{}", "x".repeat(4096)), "/x".repeat(129)] {
            assert!(
                execute(
                    &serde_json::json!({"operation":"json-select","pointer":pointer}).to_string(),
                    "{}"
                )
                .is_err()
            );
        }
        let input = serde_json::json!({"x":"\\".repeat(40000)}).to_string();
        assert!(execute(r#"{"operation":"json-select","pointer":"/x"}"#, &input).is_err());
        let input = format!(
            "{}{}-0{}",
            "[".repeat(99),
            "1.2300E+400,".repeat(50000),
            "]".repeat(99)
        );
        let pointer = "/0".repeat(98);
        let args = serde_json::json!({"steps":[{"operation":"json-format"},{"operation":"json-select","pointer":pointer},{"operation":"json-select","pointer":"/0"}]}).to_string();
        let value: serde_json::Value =
            serde_json::from_str(&execute(&args, &input).unwrap()).unwrap();
        assert_eq!(value["output"], "1.2300E+400");
    }
    #[test]
    fn tool_steps_preserve_order_and_old_single_operation_shape() {
        let input = " 乙 \r\n甲\n 乙\n\n";
        let args = r#"{"steps":[{"operation":"trim"},{"operation":"dedupe"},{"operation":"sort","reverse":true}]}"#;
        let result: serde_json::Value =
            serde_json::from_str(&execute(args, input).unwrap()).unwrap();
        assert_eq!(result["output"], "甲\n乙\n\n");
        let old: serde_json::Value = serde_json::from_str(
            &execute(r#"{"operation":"sort","reverse":true}"#, "a\nb").unwrap(),
        )
        .unwrap();
        assert_eq!(old["output"], "b\na\n");
        let source =
            r#" {"n":123456789012345678901234567890,"n":1.2300E+400,"z":-0,"s":"\u4e2d"} "#;
        let result: serde_json::Value = serde_json::from_str(
            &execute(
                r#"{"steps":[{"operation":"json-format"},{"operation":"json-minify"}]}"#,
                source,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result["output"], source.trim());
    }
    #[test]
    fn tool_plan_validation_is_exclusive_bounded_and_hides_values() {
        for args in [
            r#"{}"#,
            r#"{"steps":[]}"#,
            r#"{"steps":null}"#,
            r#"{"operation":"trim","steps":[{"operation":"dedupe"}]}"#,
            r#"{"steps":[{"operation":"trim"}],"reverse":false}"#,
            r#"{"steps":[{"operation":"trim","reverse":true}]}"#,
            r#"{"steps":[{"operation":"trim","path":"PRIVATE_PATH"}]}"#,
            r#"{"steps":[{"operation":"PRIVATE_OPERATION"}]}"#,
            r#"{"steps":[{"operation":"trim","reverse":null}]}"#,
            r#"{"steps":[{"operation":"trim","operation":"sort"}]}"#,
            r#"{"steps":[{"operation":"trim"}],"steps":[{"operation":"sort"}]}"#,
        ] {
            let error = format!("{:#}", execute(args, "PRIVATE_INPUT").unwrap_err());
            assert!(!error.contains("PRIVATE"), "{error}");
        }
        let excessive =
            serde_json::json!({"steps":vec![serde_json::json!({"operation":"trim"});17]})
                .to_string();
        assert!(execute(&excessive, "x").is_err());
        assert!(
            execute(
                &format!("{}{{\"operation\":\"trim\"}}", " ".repeat(65536)),
                "x"
            )
            .is_err()
        );
        let failure = execute(
            r#"{"steps":[{"operation":"trim"},{"operation":"json-minify"}]}"#,
            " PRIVATE_INPUT ",
        )
        .unwrap_err();
        let failure = format!("{failure:#}");
        assert!(failure.contains("step 2 failed") && !failure.contains("PRIVATE_INPUT"));
    }
    #[test]
    fn tool_pipeline_allows_large_intermediate_but_bounds_final_envelope() {
        let source = format!(
            "{}{}-0{}",
            "[".repeat(99),
            "1.2300E+400,".repeat(2000),
            "]".repeat(99)
        );
        assert!(
            run(&source, Operation::JsonFormat, false, CLI_OUTPUT)
                .unwrap()
                .len()
                > TOOL_OUTPUT
        );
        let result = execute(
            r#"{"steps":[{"operation":"json-format"},{"operation":"json-minify"}]}"#,
            &source,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap()["output"],
            source
        );
        assert!(result.len() <= TOOL_OUTPUT);
        let input = "line\n".repeat(20000);
        let result = execute(
            r#"{"steps":[{"operation":"sort"},{"operation":"dedupe"}]}"#,
            &input,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap()["output"],
            "line\n"
        );
        let quote_heavy = "\"".repeat(40000);
        assert!(
            execute(r#"{"steps":[{"operation":"trim"}]}"#, &quote_heavy)
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
        assert!(
            execute(
                r#"{"steps":[{"operation":"trim"}]}"#,
                &"x".repeat(MAX_INPUT + 1)
            )
            .is_err()
        );
        let sixteen = serde_json::json!({"steps":vec![serde_json::json!({"operation":"trim"});16]})
            .to_string();
        assert!(execute(&sixteen, "x").is_ok());
    }
    fn apply(text: &str, op: Operation) -> String {
        run(text, op, false, CLI_OUTPUT).unwrap()
    }
    #[test]
    fn jsonl_roundtrip_keeps_record_boundaries_and_original_lexical_values() {
        let lines = " {\"n\":123456789012345678901234567890,\"n\":1.2300E+400,\"s\":\"[,]} \\\" \\u4e2d\"} \r\n[1, {\"x\":true}]\n\"line\\ntext\"\n-0\nnull\nfalse";
        let expected = "[{\"n\":123456789012345678901234567890,\"n\":1.2300E+400,\"s\":\"[,]} \\\" \\u4e2d\"},[1,{\"x\":true}],\"line\\ntext\",-0,null,false]";
        assert_eq!(apply(lines, Operation::JsonlToArray), expected);
        let records = apply(expected, Operation::ArrayToJsonl);
        assert_eq!(apply(&records, Operation::JsonlToArray), expected);
        assert_eq!(records.lines().count(), 6);
        assert!(records.ends_with('\n'));
        assert_eq!(apply("", Operation::JsonlToArray), "[]");
        assert_eq!(apply("[ ]", Operation::ArrayToJsonl), "");
        for index in 0..200 {
            let value = serde_json::json!([index, [null, {}, []], {"text":format!("[,]}} \\ \" \n 世界 {index}")}, true]);
            let compact = value.to_string();
            let pretty = serde_json::to_string_pretty(&value).unwrap();
            let lines = apply(&pretty, Operation::ArrayToJsonl);
            assert_eq!(apply(&lines, Operation::JsonlToArray), compact);
        }
    }
    #[test]
    fn jsonl_rejects_bad_records_types_depth_counts_and_output_overflow() {
        for bad in [
            "\n",
            " \n",
            "{}\n\n",
            "{}\nnot-json",
            "{} true",
            "\u{feff}{}",
            "{}\n\u{feff}{}",
            "{\n\"x\":1\n}",
        ] {
            assert!(
                run(bad, Operation::JsonlToArray, false, CLI_OUTPUT).is_err(),
                "{bad:?}"
            );
        }
        for bad in ["{}", "null", "[1,]", "[1] true", "[\"unclosed]"] {
            assert!(run(bad, Operation::ArrayToJsonl, false, CLI_OUTPUT).is_err());
        }
        let within_depth = format!("{}0{}", "[".repeat(127), "]".repeat(127));
        assert_eq!(
            apply(
                &apply(&within_depth, Operation::JsonlToArray),
                Operation::ArrayToJsonl
            ),
            format!("{within_depth}\n")
        );
        let too_deep = format!("{}0{}", "[".repeat(128), "]".repeat(128));
        assert!(run(&too_deep, Operation::JsonlToArray, false, CLI_OUTPUT).is_err());
        let at_count = "0\n".repeat(MAX_ITEMS);
        let array = apply(&at_count, Operation::JsonlToArray);
        assert_eq!(apply(&array, Operation::ArrayToJsonl), at_count);
        assert!(
            run(
                &(at_count + "0\n"),
                Operation::JsonlToArray,
                false,
                CLI_OUTPUT
            )
            .is_err()
        );
        let too_many = format!("[{}0]", "0,".repeat(MAX_ITEMS));
        assert!(run(&too_many, Operation::ArrayToJsonl, false, CLI_OUTPUT).is_err());
        assert!(run("{}\n{}", Operation::JsonlToArray, false, 5).is_err());
        assert!(run("[{},{}]", Operation::ArrayToJsonl, false, 5).is_err());
    }
    #[test]
    fn pipeline_allows_bounded_expansion_then_reduction_and_retains_json_tokens() {
        let step = |operation, reverse| Step {
            operation,
            reverse,
            contains: None,
            lines: None,
            equals: None,
            pointer: None,
            fields: None,
        };
        let source = format!(
            "{}{}-0{}",
            "[".repeat(99),
            "1.2300E+400,".repeat(50000),
            "]".repeat(99)
        );
        let pretty = run(&source, Operation::JsonFormat, false, CLI_OUTPUT).unwrap();
        assert!(pretty.len() > MAX_INPUT && pretty.len() <= CLI_OUTPUT);
        let reduced = run_pipeline(
            &source,
            &[
                step(Operation::JsonFormat, false),
                step(Operation::JsonMinify, false),
            ],
        )
        .unwrap();
        assert_eq!(reduced, source);
        assert!(run_pipeline(&"x".repeat(MAX_INPUT + 1), &[step(Operation::Trim, false)]).is_err());
        assert!(run_pipeline("x", &[]).is_err());
        assert!(run_pipeline("x", &vec![step(Operation::Trim, false); MAX_STEPS + 1]).is_err());
        assert!(run_pipeline("x", &[step(Operation::Trim, true)]).is_err());
    }
    #[test]
    fn line_records_are_exact_and_cross_platform() {
        assert_eq!(
            apply("\u{3000}你好 \r\n\t\r\n x", Operation::Trim),
            "你好\n\nx\n"
        );
        assert_eq!(
            apply("b\r\na\nb\n\n\nA\na \n", Operation::Dedupe),
            "b\na\n\nA\na \n"
        );
        assert_eq!(apply("b\na\nA\na\n", Operation::Sort), "A\na\na\nb\n");
        assert_eq!(
            run("b\na", Operation::Sort, true, CLI_OUTPUT).unwrap(),
            "b\na\n"
        );
        for op in [Operation::Trim, Operation::Dedupe, Operation::Sort] {
            assert_eq!(apply("", op), "");
            assert_eq!(apply("\n", op), "\n");
        }
    }
    #[test]
    fn json_tokens_precision_order_duplicates_and_escapes_survive() {
        let input = r#" { "z": 123456789012345678901234567890, "a":1.2300e+400, "z": -0, "s":" {\"x\": [1,2]} \u4e2d \\" } "#;
        let compact = apply(input, Operation::JsonMinify);
        assert!(
            compact
                .starts_with(r#"{"z":123456789012345678901234567890,"a":1.2300e+400,"z":-0,"s":"#)
        );
        assert_eq!(
            apply(&apply(input, Operation::JsonFormat), Operation::JsonMinify),
            compact
        );
        assert_eq!(apply(input, Operation::JsonKeys), r#"["z","a","z","s"]"#);
        assert_eq!(
            apply("[{}, [], [true, null]]", Operation::JsonFormat),
            "[\n  {},\n  [],\n  [\n    true,\n    null\n  ]\n]"
        );
        assert_eq!(
            apply(r#""hello world""#, Operation::JsonFormat),
            r#""hello world""#
        );
    }
    #[test]
    fn invalid_input_and_limits_fail_without_partial_results() {
        for text in [
            "",
            "{",
            "[1,]",
            "01",
            "1e+",
            "true false",
            "{\"x\":\"\u{0001}\"}",
        ] {
            assert!(run(text, Operation::JsonMinify, false, CLI_OUTPUT).is_err());
        }
        assert!(run("[]", Operation::JsonKeys, false, CLI_OUTPUT).is_err());
        assert!(run("a", Operation::Dedupe, true, CLI_OUTPUT).is_err());
        assert!(
            run(
                &"\n".repeat(MAX_ITEMS + 1),
                Operation::Sort,
                false,
                CLI_OUTPUT
            )
            .is_err()
        );
        assert!(
            run(
                &"x".repeat(MAX_INPUT + 1),
                Operation::Trim,
                false,
                CLI_OUTPUT
            )
            .is_err()
        );
        assert!(
            run(
                &format!("{}0{}", "[".repeat(129), "]".repeat(129)),
                Operation::JsonMinify,
                false,
                CLI_OUTPUT
            )
            .is_err()
        );
        assert!(run("{\"x\":1}", Operation::JsonFormat, false, 5).is_err());
        assert!(execute(r#"{"operation":"dedupe","path":"secret"}"#, "a").is_err());
        assert!(execute(r#"{"operation":"sort","reverse":"yes"}"#, "a").is_err());
        assert!(execute(r#"{"operation":"trim"}"#, &"x".repeat(TOOL_OUTPUT)).is_err());
    }
    #[test]
    fn json_formatter_roundtrips_mixed_values_and_bounds_indentation_growth() {
        for index in 0..250 {
            let value = serde_json::json!({"quote\\\"": [index, -index, null, true, false, {}, [], {"text": format!("[] , : \\ \" \n\t\r 世界 e\u{301} {index}")} ]});
            let compact = serde_json::to_string(&value).unwrap();
            let pretty = apply(&compact, Operation::JsonFormat);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
                value
            );
            assert_eq!(apply(&pretty, Operation::JsonMinify), compact);
        }
        let at_limit = format!("{}0{}", "[".repeat(128), "]".repeat(128));
        assert_eq!(apply(&at_limit, Operation::JsonMinify), at_limit);
        let growing = format!(
            "{}{}0{}",
            "[".repeat(127),
            "0,".repeat(70000),
            "]".repeat(127)
        );
        assert!(run(&growing, Operation::JsonFormat, false, CLI_OUTPUT).is_err());
    }

    #[test]
    fn line_filters_are_literal_ordered_and_preserve_line_contents() {
        let run_filter = |operation: &str, needle: &str, input: &str| {
            let args = serde_json::json!({"operation":operation,"contains":needle}).to_string();
            let result: serde_json::Value =
                serde_json::from_str(&execute(&args, input).unwrap()).unwrap();
            result["output"].as_str().unwrap().to_owned()
        };
        let input =
            "  ERROR a.* 中\r\nerror a.*\n  ERROR a.* 中\n\nERROR e\u{301}\nERROR é\nlast\r";
        assert_eq!(
            run_filter("lines-include", "ERROR", input),
            "  ERROR a.* 中\n  ERROR a.* 中\nERROR e\u{301}\nERROR é\n"
        );
        assert_eq!(
            run_filter("lines-exclude", "ERROR", input),
            "error a.*\n\nlast\r\n"
        );
        assert_eq!(
            run_filter("lines-include", "a.*", input),
            "  ERROR a.* 中\nerror a.*\n  ERROR a.* 中\n"
        );
        assert_eq!(run_filter("lines-include", "é", input), "ERROR é\n");
        assert_eq!(run_filter("lines-include", "\t", "a\tb\n a b\n"), "a\tb\n");
        for operation in ["lines-include", "lines-exclude"] {
            assert_eq!(run_filter(operation, "x", ""), "");
        }
        assert_eq!(run_filter("lines-include", "missing", input), "");
    }

    #[test]
    fn line_filter_limits_apply_before_discarding_and_to_serialized_tool_output() {
        let mut step = Step {
            operation: Operation::LinesInclude,
            reverse: false,
            pointer: None,
            fields: None,
            contains: Some("x".into()),
            lines: None,
            equals: None,
        };
        let input = "x\n".repeat(MAX_ITEMS);
        assert_eq!(
            transform(&input, &step, MAX_INPUT, CLI_OUTPUT)
                .unwrap()
                .lines()
                .count(),
            MAX_ITEMS
        );
        step.contains = Some("absent".into());
        assert!(transform(&(input + "x\n"), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        assert!(transform(&"x".repeat(MAX_INPUT + 1), &step, MAX_INPUT, CLI_OUTPUT).is_err());
        step.contains = Some("界".repeat(1365) + "x");
        assert!(step.validate().is_ok());
        step.contains.as_mut().unwrap().push('x');
        assert!(step.validate().is_err());
        step.contains = Some("x".into());
        assert!(transform("x", &step, MAX_INPUT, 1).is_err());
        let escaped = "\"".repeat(40000);
        step.contains = Some("\"".into());
        assert!(transform(&escaped, &step, MAX_INPUT, TOOL_OUTPUT).is_ok());
        assert!(execute(r#"{"operation":"lines-include","contains":"\""}"#, &escaped).is_err());
        for args in [
            r#"{"operation":"lines-include"}"#,
            r#"{"operation":"lines-exclude","contains":""}"#,
            r#"{"operation":"lines-include","contains":null}"#,
            r#"{"operation":"lines-include","contains":true}"#,
            r#"{"operation":"lines-include","contains":"PRIVATE\n"}"#,
            r#"{"operation":"lines-exclude","contains":"PRIVATE\r"}"#,
            r#"{"operation":"trim","contains":""}"#,
            r#"{"operation":"trim","contains":null}"#,
            r#"{"operation":"lines-include","contains":"x","reverse":true}"#,
            r#"{"operation":"lines-include","contains":"x","pointer":""}"#,
            r#"{"operation":"lines-include","contains":"x","fields":[]}"#,
            r#"{"steps":[{"operation":"trim"}],"contains":"x"}"#,
        ] {
            let error = format!("{:#}", execute(args, "PRIVATE_INPUT").unwrap_err());
            assert!(!error.contains("PRIVATE"), "{error}");
        }
    }

    #[test]
    fn line_windows_preserve_order_spaces_duplicates_and_blank_lines() {
        for (operation, count, expected) in [
            ("lines-head", 1, " a \n"),
            ("lines-head", 3, " a \n\n a \n"),
            ("lines-tail", 2, " a \n尾\n"),
            ("lines-tail", 1, "尾\n"),
            ("lines-head", 20, " a \n\n a \n尾\n"),
            ("lines-tail", 20, " a \n\n a \n尾\n"),
        ] {
            let args = serde_json::json!({"operation":operation,"lines":count}).to_string();
            let result: serde_json::Value =
                serde_json::from_str(&execute(&args, " a \r\n\r\n a \n尾").unwrap()).unwrap();
            assert_eq!(result["output"], expected);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&execute(&args, "").unwrap()).unwrap()["output"],
                ""
            );
        }
        let args = r#"{"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-tail","lines":2}]}"#;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &execute(args, "ERROR old\nINFO skip\nERROR repeat\nERROR repeat\n").unwrap()
            )
            .unwrap()["output"],
            "ERROR repeat\nERROR repeat\n"
        );
        let tail_first = r#"{"steps":[{"operation":"lines-tail","lines":2},{"operation":"lines-include","contains":"ERROR"}]}"#;
        let filter_first = r#"{"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-tail","lines":2}]}"#;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &execute(tail_first, "ERROR old\nERROR mid\nINFO end\n").unwrap()
            )
            .unwrap()["output"],
            "ERROR mid\n"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &execute(filter_first, "ERROR old\nERROR mid\nINFO end\n").unwrap()
            )
            .unwrap()["output"],
            "ERROR old\nERROR mid\n"
        );
    }

    #[test]
    fn line_window_definitions_are_explicit_strict_and_match_tool_schema() {
        for args in [
            r#"{"operation":"lines-head"}"#,
            r#"{"operation":"lines-tail","lines":0}"#,
            r#"{"operation":"lines-head","lines":100001}"#,
            r#"{"operation":"lines-head","lines":null}"#,
            r#"{"operation":"lines-head","lines":-1}"#,
            r#"{"operation":"lines-head","lines":1.5}"#,
            r#"{"operation":"lines-head","lines":"2"}"#,
            r#"{"operation":"lines-head","lines":true}"#,
            r#"{"operation":"lines-head","lines":2,"lines":3}"#,
            r#"{"operation":"trim","lines":2}"#,
            r#"{"operation":"lines-tail","lines":2,"contains":"x"}"#,
            r#"{"operation":"lines-tail","lines":2,"reverse":true}"#,
            r#"{"operation":"lines-head","lines":2,"pointer":""}"#,
            r#"{"steps":[{"operation":"lines-tail","lines":2}],"lines":2}"#,
        ] {
            assert!(execute(args, "PRIVATE_INPUT").is_err(), "{args}");
        }
        let definitions = crate::data::tool_definitions();
        let params = &definitions[2]["function"]["parameters"];
        assert_eq!(params["properties"]["lines"]["minimum"], 1);
        assert_eq!(
            params["properties"]["steps"]["items"]["properties"]["lines"]["maximum"],
            100000
        );
        assert!(
            params["properties"]["operation"]["enum"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("lines-tail"))
        );
    }

    #[test]
    fn line_windows_count_discarded_bytes_lines_and_final_envelope_limits() {
        let args = r#"{"operation":"lines-head","lines":1}"#;
        assert!(execute(args, &format!("ok\n{}", "x".repeat(MAX_INPUT))).is_err());
        assert!(execute(args, &"x\n".repeat(MAX_ITEMS + 1)).is_err());
        let tail = r#"{"operation":"lines-tail","lines":1}"#;
        assert!(execute(tail, &"x\n".repeat(MAX_ITEMS + 1)).is_err());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &execute(tail, &format!("{}end", "x\n".repeat(MAX_ITEMS - 1))).unwrap()
            )
            .unwrap()["output"],
            "end\n"
        );
        assert!(execute(tail, &"x".repeat(TOOL_OUTPUT)).is_err());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &execute(tail, &format!("{}\nend", "x".repeat(TOOL_OUTPUT))).unwrap()
            )
            .unwrap()["output"],
            "end\n"
        );
    }
}

#[cfg(test)]
mod jsonl_where_tests {
    use super::*;
    fn output(args: &str, input: &str) -> Result<String> {
        let value: serde_json::Value = serde_json::from_str(&execute(args, input)?)?;
        Ok(value["output"].as_str().unwrap().to_owned())
    }
    const FILTER: &str = r#"{"operation":"jsonl-where","pointer":"/level","equals":"ERROR"}"#;
    #[test]
    fn structured_filter_decodes_strings_and_preserves_whole_json_tokens() {
        let input = " {\"level\":\"INFO\",\"message\":\"ERROR\"}\r\n{\"n\":1.2300E+400,\"level\":\"\\u0045RROR\",\"x\":1,\"x\":-0}\n{\"level\":null}\n{\"level\":true}\n{\"level\":7}\n{\"level\":{}}\n{\"level\":[]}\n";
        assert_eq!(
            output(FILTER, input).unwrap(),
            "{\"n\":1.2300E+400,\"level\":\"\\u0045RROR\",\"x\":1,\"x\":-0}\n"
        );
        assert_eq!(output(FILTER, "").unwrap(), "");
        assert_eq!(output(FILTER, "{\"level\":\"error\"}").unwrap(), "");
        assert_eq!(
            output(
                r#"{"operation":"jsonl-where","pointer":"","equals":""}"#,
                "\"\"\nnull\n\"x\""
            )
            .unwrap(),
            "\"\"\n"
        );
    }
    #[test]
    fn structured_filter_rejects_incomplete_null_duplicate_and_unrelated_definitions() {
        for args in [
            r#"{"operation":"jsonl-where","pointer":"/level"}"#,
            r#"{"operation":"jsonl-where","equals":"ERROR"}"#,
            r#"{"operation":"jsonl-where","pointer":"/level","equals":null}"#,
            r#"{"operation":"jsonl-where","pointer":"/level","equals":1}"#,
            r#"{"operation":"jsonl-where","pointer":"/level","equals":false}"#,
            r#"{"operation":"jsonl-where","pointer":"/level","equals":"x","equals":"y"}"#,
            r#"{"operation":"jsonl-where","pointer":"bad","equals":"x"}"#,
            r#"{"operation":"jsonl-where","pointer":"/~2","equals":"x"}"#,
            r#"{"operation":"jsonl-where","pointer":"","equals":"x","contains":"x"}"#,
            r#"{"operation":"jsonl-where","pointer":"","equals":"x","reverse":true}"#,
            r#"{"operation":"jsonl-where","pointer":"","equals":"x","lines":1}"#,
            r#"{"operation":"jsonl-where","pointer":"","equals":"x","fields":[]}"#,
            r#"{"operation":"trim","equals":""}"#,
            r#"{"operation":"jsonl-select","pointer":"","equals":"x"}"#,
            r#"{"steps":[{"operation":"jsonl-where","pointer":"","equals":"x"}],"equals":"x"}"#,
        ] {
            assert!(execute(args, "").is_err(), "{args}");
        }
        let args =
            serde_json::json!({"operation":"jsonl-where","pointer":"","equals":"界".repeat(1366)})
                .to_string();
        assert!(execute(&args, "").is_err());
        let schema = crate::data::tool_definitions();
        let p = &schema[2]["function"]["parameters"];
        assert_eq!(p["properties"]["equals"]["type"], "string");
        assert_eq!(
            p["properties"]["equals"],
            p["properties"]["steps"]["items"]["properties"]["equals"]
        );
        assert!(
            p["properties"]["operation"]["enum"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("jsonl-where"))
        );
    }
    #[test]
    fn structured_filter_checks_later_discarded_records_and_hides_private_literals() {
        for line in [
            r#"{"PRIVATE":1}"#,
            r#"{"level":"INFO","level":"ERROR"}"#,
            r#"{"level":"ERROR","le\u0076el":"INFO"}"#,
            r#"{"level":"INFO"} PRIVATE"#,
            r#"{"level":"\uD800"}"#,
            "",
        ] {
            let input = format!("{{\"level\":\"ERROR\"}}\n{line}\n");
            let error = format!("{:#}", execute(FILTER, &input).unwrap_err());
            assert!(error.contains("record 2"), "{error}");
            assert!(!error.contains("PRIVATE"), "{error}");
        }
        let pointer =
            serde_json::json!({"operation":"jsonl-where","pointer":"/PRIVATE","equals":"PRIVATE"})
                .to_string();
        assert!(!format!("{:#}", execute(&pointer, "{}").unwrap_err()).contains("PRIVATE"));
    }
    #[test]
    fn structured_filter_counts_discarded_records_bytes_and_final_envelope() {
        let rows = "{\"level\":\"INFO\"}\n".repeat(MAX_ITEMS);
        assert_eq!(output(FILTER, &rows).unwrap(), "");
        assert!(execute(FILTER, &(rows + "{\"level\":\"INFO\"}\n")).is_err());
        assert!(execute(FILTER, &" ".repeat(MAX_INPUT + 1)).is_err());
        let large = format!(
            "{{\"level\":\"ERROR\",\"x\":\"{}\"}}",
            "x".repeat(TOOL_OUTPUT)
        );
        assert!(execute(FILTER, &large).is_err());
        let discard = large.replace("ERROR", "INFO");
        assert_eq!(output(FILTER, &discard).unwrap(), "");
        let step: Step = serde_json::from_str(FILTER).unwrap();
        assert!(transform("{\"level\":\"ERROR\"}", &step, MAX_INPUT, 4).is_err());
    }
    #[test]
    fn structured_filter_composes_in_order_with_selection_projection_and_csv() {
        let args = r#"{"steps":[{"operation":"json-select","pointer":"/items"},{"operation":"array-to-jsonl"},{"operation":"jsonl-where","pointer":"/level","equals":"ERROR"},{"operation":"jsonl-project","fields":[{"name":"id","pointer":"/id"}]},{"operation":"jsonl-to-csv"}]}"#;
        assert_eq!(
            output(
                args,
                r#"{"items":[{"level":"INFO","id":1},{"id":1.2300E+400,"level":"ERROR"}]}"#
            )
            .unwrap(),
            "\"id\"\n\"1.2300E+400\"\n"
        );
    }
    #[test]
    fn structured_filter_later_stage_accepts_large_intermediate() {
        let args = r#"{"steps":[{"operation":"csv-to-jsonl"},{"operation":"jsonl-where","pointer":"/level","equals":"ERROR"}]}"#;
        // Quoted JSON escaping expands CSV backslashes beyond the initial8MiB cap.
        let input = format!("level,value\nINFO,{}", "\\".repeat(5 * 1024 * 1024));
        assert_eq!(output(args, &input).unwrap(), "");
    }
}

#[cfg(test)]
mod jsonl_key_tests {
    use super::*;
    fn step(operation: Operation, pointer: &str, reverse: bool) -> Step {
        Step {
            operation,
            pointer: Some(pointer.into()),
            reverse,
            fields: None,
            contains: None,
            equals: None,
            lines: None,
        }
    }
    const OPS: [Operation; 3] = [
        Operation::JsonlDedupe,
        Operation::JsonlSort,
        Operation::JsonlCountBy,
    ];

    #[test]
    fn key_dedupe_preserves_first_complete_record_and_decodes_equivalent_keys() {
        let input = "{ \"k\":\"中\",\"n\":1.2300E+400,\"x\":1,\"x\":2 }\r\n{\"k\":\"\\u4e2d\",\"n\":-0}\n{\"k\":\"\",\"n\":9007199254740993}\n";
        assert_eq!(
            transform(
                input,
                &step(Operation::JsonlDedupe, "/k", false),
                MAX_INPUT,
                CLI_OUTPUT
            )
            .unwrap(),
            "{\"k\":\"中\",\"n\":1.2300E+400,\"x\":1,\"x\":2}\n{\"k\":\"\",\"n\":9007199254740993}\n"
        );
        assert_eq!(
            transform(
                "\"a\"\n\"\\u0061\"\n\"A\"",
                &step(Operation::JsonlDedupe, "", false),
                MAX_INPUT,
                CLI_OUTPUT
            )
            .unwrap(),
            "\"a\"\n\"A\"\n"
        );
    }
    #[test]
    fn key_sort_is_lexical_stable_in_both_directions_without_normalization() {
        let input =
            "{\"k\":\"2\",\"id\":1}\n{\"k\":\"10\",\"id\":2}\n{\"k\":\"\\u0032\",\"id\":3}\n";
        for (reverse, ids) in [(false, vec![2, 1, 3]), (true, vec![1, 3, 2])] {
            let result = transform(
                input,
                &step(Operation::JsonlSort, "/k", reverse),
                MAX_INPUT,
                CLI_OUTPUT,
            )
            .unwrap();
            let actual: Vec<_> = result
                .lines()
                .map(|line| {
                    serde_json::from_str::<serde_json::Value>(line).unwrap()["id"]
                        .as_u64()
                        .unwrap()
                })
                .collect();
            assert_eq!(actual, ids);
            assert!(result.contains("\\u0032"));
        }
        assert_eq!(
            transform(
                "\"é\"\n\"e\\u0301\"\n\"A\"\n\"a\"",
                &step(Operation::JsonlSort, "", false),
                MAX_INPUT,
                CLI_OUTPUT
            )
            .unwrap(),
            "\"A\"\n\"a\"\n\"e\\u0301\"\n\"é\"\n"
        );
    }
    #[test]
    fn key_counts_use_first_seen_decoded_keys_and_escape_new_output_safely() {
        let input = "{\"a/b\":{\"~\":[\"中\"]}}\n{\"a/b\":{\"~\":[\"\\u4e2d\"]}}\n{\"a/b\":{\"~\":[\"\\n\\u0000\\\"\"]}}\n{\"a/b\":{\"~\":[\"\"]}}";
        assert_eq!(
            transform(
                input,
                &step(Operation::JsonlCountBy, "/a~1b/~0/0", false),
                MAX_INPUT,
                CLI_OUTPUT
            )
            .unwrap(),
            "{\"value\":\"中\",\"count\":2}\n{\"value\":\"\\n\\u0000\\\"\",\"count\":1}\n{\"value\":\"\",\"count\":1}\n"
        );
    }
    #[test]
    fn every_key_record_is_checked_without_echoing_private_data() {
        for operation in OPS {
            let valid = r#"{"k":"a","literal":"\\uD800","emoji":"\uD83D\uDE00","n":1.2300E+400}"#;
            assert!(transform(valid, &step(operation, "/k", false), MAX_INPUT, CLI_OUTPUT).is_ok());
            for bad in [
                r#"{"k":1,"secret":"PRIVATE"}"#,
                r#"{"k":null,"secret":"PRIVATE"}"#,
                r#"{"k":true,"secret":"PRIVATE"}"#,
                r#"{"k":[],"secret":"PRIVATE"}"#,
                r#"{"k":{},"secret":"PRIVATE"}"#,
                r#"{"k":"a","k":"a","secret":"PRIVATE"}"#,
                r#"{"k":"a","\u006b":"a","secret":"PRIVATE"}"#,
                r#"{"missing":"PRIVATE"}"#,
                r#"{"k":"a","secret":"PRIVATE","bad":[1,]}"#,
                "",
                r#"{"k":"a","bad":"\uD800","secret":"PRIVATE"}"#,
            ] {
                let error = format!(
                    "{:#}",
                    transform(
                        &format!("{{\"k\":\"a\"}}\n{bad}\n"),
                        &step(operation, "/k", false),
                        MAX_INPUT,
                        CLI_OUTPUT
                    )
                    .unwrap_err()
                );
                assert!(error.contains("record 2"), "{error}");
                assert!(!error.contains("PRIVATE"), "{error}");
            }
            let s = step(operation, "/a/01", false);
            assert!(transform(r#"{"a":["ok","PRIVATE"]}"#, &s, MAX_INPUT, CLI_OUTPUT).is_err());
        }
    }
    #[test]
    fn key_operations_enforce_input_record_depth_and_complete_output_limits() {
        for operation in OPS {
            let s = step(operation, "/k", false);
            assert_eq!(transform("", &s, MAX_INPUT, CLI_OUTPUT).unwrap(), "");
            let many = "{\"k\":\"a\"}\n".repeat(MAX_ITEMS);
            let out = transform(&many, &s, MAX_INPUT, CLI_OUTPUT).unwrap();
            if operation == Operation::JsonlCountBy {
                assert_eq!(out, "{\"value\":\"a\",\"count\":100000}\n");
            }
            assert!(transform(&(many + "{\"k\":\"a\"}\n"), &s, MAX_INPUT, CLI_OUTPUT).is_err());
            assert!(transform("{\"k\":\"long\"}", &s, MAX_INPUT, 4).is_err());
            assert!(transform(&" ".repeat(MAX_INPUT + 1), &s, MAX_INPUT, CLI_OUTPUT).is_err());
            let deep = |n| {
                format!(
                    "{{\"k\":\"ok\",\"data\":{}0{}}}",
                    "[".repeat(n),
                    "]".repeat(n)
                )
            };
            assert!(transform(&deep(127), &s, MAX_INPUT, CLI_OUTPUT).is_ok());
            assert!(transform(&deep(128), &s, MAX_INPUT, CLI_OUTPUT).is_err());
        }
        let input = "\"\\\\\"\n".repeat(15000);
        let args = r#"{"operation":"jsonl-sort","pointer":""}"#;
        let error = format!("{:#}", execute(args, &input).unwrap_err());
        assert!(error.contains("64 KiB") || error.contains("65536"));
    }
    #[test]
    fn key_definitions_and_schema_share_strict_pointer_and_option_contracts() {
        for operation in OPS {
            let mut s = step(operation, "", false);
            s.pointer = None;
            assert!(s.validate().is_err());
            s.pointer = Some("/~2".into());
            assert!(s.validate().is_err());
            s.pointer = Some(format!("/{}", "x".repeat(4096)));
            assert!(s.validate().is_err());
            s.pointer = Some("/x".repeat(129));
            assert!(s.validate().is_err());
            s = step(operation, "", false);
            s.equals = Some(String::new());
            assert!(s.validate().is_err());
            s = step(operation, "", false);
            s.contains = Some("x".into());
            assert!(s.validate().is_err());
            s = step(operation, "", false);
            s.lines = Some(1);
            assert!(s.validate().is_err());
            s = step(operation, "", false);
            s.fields = Some(vec![]);
            assert!(s.validate().is_err());
            s = step(operation, "", true);
            assert_eq!(s.validate().is_ok(), operation == Operation::JsonlSort);
        }
        let tools = crate::data::tool_definitions();
        let schema = &tools[2]["function"]["parameters"];
        for properties in [
            &schema["properties"],
            &schema["properties"]["steps"]["items"]["properties"],
        ] {
            for op in ["jsonl-dedupe", "jsonl-sort", "jsonl-count-by"] {
                assert!(
                    properties["operation"]["enum"]
                        .as_array()
                        .unwrap()
                        .contains(&serde_json::json!(op))
                );
                for bad in [
                    serde_json::json!({"operation":op,"pointer":null}),
                    serde_json::json!({"operation":op,"pointer":"","equals":null}),
                    serde_json::json!({"operation":op,"pointer":"","reverse":null}),
                ] {
                    assert!(execute(&bad.to_string(), "\"a\"").is_err());
                }
            }
        }
    }
    #[test]
    fn key_analysis_can_reduce_large_intermediates_and_compose_with_csv() {
        let args = r#"{"steps":[{"operation":"csv-to-jsonl"},{"operation":"jsonl-dedupe","pointer":"/id"},{"operation":"jsonl-count-by","pointer":"/level"},{"operation":"jsonl-to-csv"}]}"#;
        let out = execute(args, "id,level\nb,WARN\na,ERROR\nb,WARN\nc,ERROR\n").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).unwrap()["output"],
            "\"value\",\"count\"\n\"WARN\",\"1\"\n\"ERROR\",\"2\"\n"
        );
        let args = r#"{"steps":[{"operation":"csv-to-jsonl"},{"operation":"jsonl-count-by","pointer":"/id"}]}"#;
        // CSV backslashes expand past8MiB after decoding to JSON, then a keyed count reduces output.
        let input = format!("id,payload\na,{}", "\\".repeat(5 * 1024 * 1024));
        assert!(execute(args, &input).is_ok());
    }
}
