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
    let operation = json!({"type":"string","enum":["trim","dedupe","sort","json-format","json-minify","json-keys","jsonl-to-array","array-to-jsonl","json-select","jsonl-select","csv-to-jsonl","jsonl-to-csv","jsonl-project"]});
    let projection_fields = json!({"type":"array","minItems":1,"maxItems":64,"description":"Only for jsonl-project: ordered output field definitions. Names unique, 1-256 UTF-8 bytes, no controls; pointers explicit (empty selects root), each max4096 UTF-8 bytes/128 reference tokens.","items":{"type":"object","properties":{"name":{"type":"string"},"pointer":{"type":"string"}},"required":["name","pointer"],"additionalProperties":false}});
    let step = json!({"type":"object","properties":{"operation":operation,"fields":projection_fields,"reverse":{"type":"boolean"},"pointer":{"type":"string","description":"Required for json-select and jsonl-select: plain RFC 6901, empty selects root, max4096 UTF-8 bytes/128 tokens. Selected strings stay quoted JSON."}},"required":["operation"],"additionalProperties":false});
    vec![
        json!({"type":"function","function":{"name":"count_input","description":"Count original input exactly. words means whitespace-delimited tokens; characters means Unicode scalars including whitespace; cjk_characters counts Han ideographs; graphemes counts visible clusters. Always use this for counting, never estimate.","parameters":{"type":"object","properties":{},"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"select_input","description":"Select original JSON using a plain RFC 6901 pointer (e.g. /abc/0). Preserve numeric tokens, object key order and repeated keys inside selected values; reject ambiguous referenced keys. Strings return decoded text; optional separator joins array display values. Pointer max 4096 UTF-8 bytes/128 tokens; input max 64 MiB, depth 128, output text max 64 KiB. No filesystem or numeric conversion; do not copy original input into arguments.","parameters":{"type":"object","properties":{"pointer":{"type":"string"},"separator":{"type":"string"}},"required":["pointer"],"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"transform_input","description":"Transform original input exactly. Supply either one operation with optional reverse/pointer/fields, or ordered steps (1-16 objects of the same shape); never both. Steps consume the previous intermediate within this invocation; separate invocations start from original input. Operations: trim Unicode whitespace per line, dedupe exact lines (keep first), sort case-sensitive Unicode lines, json-format, json-minify, json-keys (root keys in source order), jsonl-to-array (one valid value per line; no blank lines), array-to-jsonl (root array items to compact JSON lines), json-select (select one value), jsonl-select (select one value from each JSONL record in order; fail on any invalid/missing/ambiguous record with its 1-based position). The two JSON selection operations require an explicit pointer, even empty for root; selected strings stay quoted JSON. csv-to-jsonl imports UTF-8 comma CSV with a required unique nonempty header, strict double quoting and LF/CRLF records; all cells become JSON strings, with no trimming or number inference. Reject blank records, malformed quotes, wrong row widths, more than 1024 columns or header names above 4096 UTF-8 bytes. CSV record1 is the header; multiline quoted fields count as one record. Header-only CSV yields empty JSONL; no implicit repair or row skipping. jsonl-to-csv exports nonempty flat scalar JSONL objects: first record defines ordered unique nonempty columns, later records must have the same columns in any order. Strings decode, number tokens/boolean/null become their exact text; nested values and duplicate decoded keys are errors. All header/cell fields are quoted, inner quotes doubled, LF terminators, no BOM or spreadsheet type/formula sanitization. CSV export has the same 1024 column/4096-byte name caps; empty input has no schema and fails. jsonl-project requires explicit fields (1-64 ordered objects with name and pointer) and no top-level pointer. It selects/renames multiple values per JSONL record into one new object, preserving selected JSON tokens and internal key order/repeated keys. Output names unique, 1-256 UTF-8 bytes, no controls. Missing/ambiguous/invalid references fail the complete call; existing null is valid, no skip/defaults. Projection adds one container; nesting remains <=128. Fields are invalid on other operations, even empty. JSON formatting/selection preserve tokens/order/repeated keys; line output uses LF. reverse only applies to sort; pointer only applies to json-select or jsonl-select. JSON selection uses the preceding intermediate, not the original input; use select_input separately only when display text/join is desired. No filesystem, shell, includes or model calls. Do not copy input into arguments. Only final output returns as JSON with output; failure returns an error without intermediates. Limits: 64 KiB arguments, 8 MiB original input, 16 MiB intermediate, 100000 lines/keys/records, depth128, 64 KiB final JSON envelope.","parameters":{"type":"object","properties":{"operation":operation,"fields":projection_fields,"reverse":{"type":"boolean"},"pointer":{"type":"string","description":"Required for json-select and jsonl-select: plain RFC 6901, empty selects root, max4096 UTF-8 bytes/128 tokens. Selected strings stay quoted JSON."},"steps":{"type":"array","minItems":1,"maxItems":16,"items":step}},"oneOf":[{"required":["operation"],"not":{"required":["steps"]}},{"required":["steps"],"not":{"anyOf":[{"required":["operation"]},{"required":["reverse"]},{"required":["pointer"]},{"required":["fields"]}]}}],"additionalProperties":false}}}),
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
