use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

pub const RESULT_LIMIT: usize = 64 * 1024 * 1024;

pub fn counts(input: &str) -> Value {
    json!({
        "bytes": input.len(),
        "characters": input.chars().count(),
        "graphemes": input.graphemes(true).count(),
        "non_whitespace_characters": input.chars().filter(|c| !c.is_whitespace()).count(),
        "words": input.split_whitespace().count(),
        "lines": input.lines().count(),
        "cjk_characters": input.chars().filter(|c| matches!(*c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F | 0x30000..=0x323AF)).count()
    })
}

pub fn select(input: &str, pointer: &str, separator: Option<&str>) -> Result<String> {
    crate::selection::select(input, pointer, separator, RESULT_LIMIT)
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{"name":"count_input","description":"Count original input exactly. words means whitespace-delimited tokens; characters means Unicode scalars including whitespace; cjk_characters counts Han ideographs; graphemes counts visible clusters. Always use this for counting, never estimate.","parameters":{"type":"object","properties":{},"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"select_input","description":"Select original JSON using a plain RFC 6901 pointer (e.g. /abc/0). Preserve numeric tokens, object key order and repeated keys inside selected values; reject ambiguous referenced keys. Strings return decoded text; optional separator joins array display values. Pointer max 4096 UTF-8 bytes/128 tokens; input max 64 MiB, depth 128, output text max 64 KiB. No filesystem or numeric conversion; do not copy original input into arguments.","parameters":{"type":"object","properties":{"pointer":{"type":"string"},"separator":{"type":"string"}},"required":["pointer"],"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"transform_input","description":"Transform original input exactly: trim Unicode whitespace per line, dedupe exact lines (keep first), sort lines by case-sensitive Unicode order, json-format, json-minify, json-keys (root keys in source order), jsonl-to-array (one valid JSON value per line; no blank lines), or array-to-jsonl (one compact JSON line per root array item). JSON formatting and JSONL conversion preserve original number/string tokens, key order and duplicate keys. Line output uses LF. No filesystem or shell. Do not copy input into arguments. Returns JSON with output; max 8 MiB input, 100000 lines/keys/JSONL records, 128 JSON containers, 64 KiB result. reverse only applies to sort.","parameters":{"type":"object","properties":{"operation":{"type":"string","enum":["trim","dedupe","sort","json-format","json-minify","json-keys","jsonl-to-array","array-to-jsonl"]},"reverse":{"type":"boolean"}},"required":["operation"],"additionalProperties":false}}}),
    ]
}

pub fn execute(name: &str, arguments: &str, input: &str) -> Result<String> {
    if name == "transform_input" {
        return crate::transforms::execute(arguments, input);
    }
    let args: Value =
        serde_json::from_str(arguments).context("tool arguments must be valid JSON")?;
    let object = args
        .as_object()
        .context("tool arguments must be an object")?;
    match name {
        "count_input" => {
            if !object.is_empty() {
                bail!("count_input takes no arguments");
            }
            Ok(counts(input).to_string())
        }
        "select_input" => {
            if object.keys().any(|k| k != "pointer" && k != "separator") {
                bail!("unknown select_input argument");
            }
            let pointer = args
                .get("pointer")
                .and_then(Value::as_str)
                .context("pointer must be a string")?;
            let separator = args
                .get("separator")
                .map(|v| v.as_str().context("separator must be a string"))
                .transpose()?;
            crate::selection::select(input, pointer, separator, 64 * 1024)
        }
        _ => bail!("unknown tool: {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_counts_are_explicit() {
        let c = counts("你好 e\u{301}\n");
        assert_eq!(c["characters"], 6);
        assert_eq!(c["graphemes"], 5);
        assert_eq!(c["cjk_characters"], 2);
        assert_eq!(c["words"], 2);
        assert_eq!(counts("")["lines"], 0);
        assert_eq!(counts("a\r\nb\n")["lines"], 2);
    }
    #[test]
    fn json_selection_and_join() {
        assert_eq!(
            select(r#"{"abc":[["a","b",3]]}"#, "/abc/0", Some("-")).unwrap(),
            "a-b-3"
        );
        assert_eq!(
            select(r#"{"a/b":{"~":null}}"#, "/a~1b/~0", None).unwrap(),
            "null"
        );
        assert!(select("{}", "/abc", None).is_err());
        assert!(select("123", "", Some(",")).is_err());
    }
    #[test]
    fn malformed_tools_are_rejected() {
        assert!(execute("count_input", "[]", "a").is_err());
        assert!(execute("shell", "{}", "a").is_err());
        assert!(execute("select_input", r#"{"pointer":"","separator":5}"#, "[]").is_err());
    }
}
