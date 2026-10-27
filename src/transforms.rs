//! Deterministic transformations of explicit input. No model, filesystem or shell access.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize, Serialize,
    de::{IgnoredAny, MapAccess, Visitor},
};
use std::{
    collections::HashSet,
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
    /// Trim Unicode whitespace from each line; retain blank lines
    Trim,
    /// Keep the first occurrence of each exact line, preserving order
    Dedupe,
    /// Sort lines by case-sensitive Unicode order; retain duplicates
    Sort,
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
    /// Convert header-based comma CSV to JSONL objects; keep every cell as a string
    CsvToJsonl,
    /// Export flat scalar JSONL objects as quoted CSV; first object defines column order
    JsonlToCsv,
    /// Select and rename multiple values into one ordered JSON object per JSONL record
    JsonlProject,
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
        deserialize_with = "pointer_string"
    )]
    pub pointer: Option<String>,
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

fn pointer_string<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(de).map(Some)
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Step {
    pub fn validate(&self) -> Result<()> {
        if self.reverse && self.operation != Operation::Sort {
            bail!("--reverse is only valid for sort");
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
            (Some(pointer), Operation::JsonSelect | Operation::JsonlSelect) => {
                crate::selection::validate_pointer(pointer)
            }
            (None, Operation::JsonSelect | Operation::JsonlSelect) => bail!(
                "{} requires an explicit pointer; use an empty pointer to select the root",
                if self.operation == Operation::JsonSelect {
                    "json-select"
                } else {
                    "jsonl-select"
                }
            ),
            (Some(_), _) => bail!("pointer is only valid for json-select or jsonl-select"),
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
        step.validate()
            .with_context(|| format!("data pipeline step {} is invalid", index + 1))?;
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
        current = transform(source, step, input_limit, limit)
            .with_context(|| format!("data pipeline step {} failed", index + 1))?;
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
        Operation::Trim | Operation::Dedupe | Operation::Sort => {
            let mut lines: Vec<_> = input.lines().take(MAX_ITEMS + 1).collect();
            if lines.len() > MAX_ITEMS {
                bail!("line transformations accept at most 100000 lines");
            }
            match operation {
                Operation::Trim => lines.iter_mut().for_each(|line| *line = line.trim()),
                Operation::Dedupe => {
                    let mut seen = HashSet::new();
                    lines.retain(|line| seen.insert(*line));
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

pub fn execute(arguments: &str, input: &str) -> Result<String> {
    if arguments.len() > 64 * 1024 {
        bail!("transform tool arguments exceed 64 KiB");
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Steps {
        steps: Vec<Step>,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Args {
        Single(Step),
        Pipeline(Steps),
    }
    let args: Args = serde_json::from_str(arguments).map_err(|_| {
        anyhow::anyhow!(
            "transform_input requires either operation with optional reverse/pointer/fields, or 1-16 steps of the same shape; do not combine them"
        )
    })?;
    let output = match args {
        Args::Single(step) => transform(input, &step, MAX_INPUT, TOOL_OUTPUT)?,
        Args::Pipeline(args) => pipeline(input, &args.steps, TOOL_OUTPUT)?,
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
}
