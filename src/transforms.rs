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
    if input.len() > MAX_INPUT {
        bail!("data transformations accept at most 8 MiB of UTF-8 input");
    }
    if reverse && operation != Operation::Sort {
        bail!("--reverse is only valid for sort");
    }
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
