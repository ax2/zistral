//! Deterministic transformations of explicit input. No model, filesystem or shell access.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Deserialize)]
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

fn validate_json(input: &str) -> Result<()> {
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

pub fn run(input: &str, operation: Operation, reverse: bool, limit: usize) -> Result<String> {
    transform(input, operation, reverse, MAX_INPUT, limit)
}

pub fn validate_step(operation: Operation, reverse: bool) -> Result<()> {
    if reverse && operation != Operation::Sort {
        bail!("--reverse is only valid for sort");
    }
    Ok(())
}

/// Buffer only the current/next stage. Never emit an intermediate result.
pub fn run_pipeline(input: &str, steps: &[(Operation, bool)]) -> Result<String> {
    if steps.is_empty() || steps.len() > MAX_STEPS {
        bail!("a data pipeline requires 1-16 steps");
    }
    for &(operation, reverse) in steps {
        validate_step(operation, reverse)?;
    }
    let mut current = String::new();
    for (index, &(operation, reverse)) in steps.iter().enumerate() {
        let source = if index == 0 { input } else { &current };
        let input_limit = if index == 0 { MAX_INPUT } else { CLI_OUTPUT };
        current = transform(source, operation, reverse, input_limit, CLI_OUTPUT)
            .with_context(|| format!("data pipeline step {} failed", index + 1))?;
    }
    Ok(current)
}

fn transform(
    input: &str,
    operation: Operation,
    reverse: bool,
    input_limit: usize,
    limit: usize,
) -> Result<String> {
    if input.len() > input_limit {
        bail!("data transformation input exceeds {input_limit} bytes");
    }
    validate_step(operation, reverse)?;
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
                    if reverse {
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
    }
    Ok(out.finish())
}

pub fn execute(arguments: &str, input: &str) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        operation: Operation,
        #[serde(default)]
        reverse: bool,
    }
    let args: Args = serde_json::from_str(arguments).map_err(|_| {
        anyhow::anyhow!(
            "transform_input requires a supported operation and optional boolean reverse"
        )
    })?;
    let output = run(input, args.operation, args.reverse, TOOL_OUTPUT)?;
    let result = serde_json::to_string(&serde_json::json!({"output":output}))?;
    if result.len() > TOOL_OUTPUT {
        bail!("transform tool result exceeds 64 KiB; reduce input or use zi data locally");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
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
                (Operation::JsonFormat, false),
                (Operation::JsonMinify, false),
            ],
        )
        .unwrap();
        assert_eq!(reduced, source);
        assert!(run_pipeline(&"x".repeat(MAX_INPUT + 1), &[(Operation::Trim, false)]).is_err());
        assert!(run_pipeline("x", &[]).is_err());
        assert!(run_pipeline("x", &[(Operation::Trim, false); MAX_STEPS + 1]).is_err());
        assert!(run_pipeline("x", &[(Operation::Trim, true)]).is_err());
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
