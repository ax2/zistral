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
    let operation = json!({"type":"string","enum":["trim","dedupe","sort","json-format","json-minify","json-keys","json-profile","jsonl-profile","jsonl-to-array","array-to-jsonl","json-select","jsonl-select","json-redact","jsonl-redact","csv-to-jsonl","jsonl-to-csv","jsonl-project","lines-include","lines-exclude","lines-head","lines-tail","jsonl-where","jsonl-dedupe","jsonl-sort","jsonl-sort-int","jsonl-count-by"]});
    let projection_fields = json!({"type":"array","minItems":1,"maxItems":64,"description":"Only for jsonl-project: ordered output field definitions. Names unique, 1-256 UTF-8 bytes, no controls; pointers explicit (empty selects root), each max4096 UTF-8 bytes/128 reference tokens.","items":{"type":"object","properties":{"name":{"type":"string"},"pointer":{"type":"string"}},"required":["name","pointer"],"additionalProperties":false}});
    let contains = json!({"type":"string","description":"Only for lines-include/lines-exclude: required literal substring, 1-4096 UTF-8 bytes, no CR/LF. Case-sensitive; no regex, glob, normalization or structured-field predicate."});
    let lines = json!({"type":"integer","minimum":1,"maximum":100000,"description":"Required only for lines-head/lines-tail: number of first/last lines. Keep original order, whitespace and duplicates; normalize CRLF to LF. All input bytes and discarded lines still count toward limits."});
    let equals = json!({"type":"string","description":"Required only for jsonl-where: exact decoded JSON string, 0-4096 UTF-8 bytes. Empty is allowed. Nonstring selected values do not match; no numeric inference, substring, regex or Unicode normalization. Missing/ambiguous pointers and malformed records fail the complete call."});
    let step = json!({"type":"object","properties":{"operation":operation,"contains":contains,"equals":equals,"lines":lines,"fields":projection_fields,"reverse":{"type":"boolean"},"pointer":{"type":"string","description":"Required for json-select, jsonl-select, json-redact, jsonl-redact, jsonl-where, jsonl-dedupe, jsonl-sort, jsonl-sort-int and jsonl-count-by: plain RFC 6901, empty selects root, max4096 UTF-8 bytes/128 tokens. Selected strings stay quoted JSON."}},"required":["operation"],"additionalProperties":false});
    vec![
        json!({"type":"function","function":{"name":"count_input","description":"Count original input exactly. words means whitespace-delimited tokens; characters means Unicode scalars including whitespace; cjk_characters counts Han ideographs; graphemes counts visible clusters. Always use this for counting, never estimate.","parameters":{"type":"object","properties":{},"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"select_input","description":"Select original JSON using a plain RFC 6901 pointer (e.g. /abc/0). Preserve numeric tokens, object key order and repeated keys inside selected values; reject ambiguous referenced keys. Strings return decoded text; optional separator joins array display values. Pointer max 4096 UTF-8 bytes/128 tokens; input max 64 MiB, depth 128, output text max 64 KiB. No filesystem or numeric conversion; do not copy original input into arguments.","parameters":{"type":"object","properties":{"pointer":{"type":"string"},"separator":{"type":"string"}},"required":["pointer"],"additionalProperties":false}}}),
        json!({"type":"function","function":{"name":"transform_input","description":"Transform original input exactly. Supply either one operation with optional reverse/pointer/fields/contains/lines/equals, or ordered steps (1-16 objects of the same shape); never both. Steps consume the previous intermediate within this invocation; separate invocations start from original input. Operations: json-profile (one JSON document) and jsonl-profile (JSONL records) report {records,paths:[{pointer,present,missing,types}]} without sample values. Visit object members recursively; arrays are opaque leaves, with no index/wildcard paths. Types count array/boolean/null/number/object/string, without float conversion or schema inference. Root pointer is empty; paths are sorted by encoded RFC6901 pointer text. Missing counts are relative to all records, including absent/nonobject ancestors. At most1024 distinct paths including root, each4096UTF-8 bytes/128tokens; visited objects require unique decoded keys. Validate whole original JSON/Unicode even inside opaque arrays. Empty JSONL reports0records and no paths; blank/invalid records fail. Keys/path names are exposed, so this is not sanitization. Model-side profiling cannot undo original-input disclosure; profile offline before sharing only the report when needed. No operation-specific arguments. Operations: lines-head/lines-tail keep the first/last explicit lines count (integer 1-100000), in original order without trimming or deduplication; CRLF becomes LF and selected lines end in LF. Count all input bytes and discarded lines toward existing limits. trim Unicode whitespace per line, dedupe exact lines (keep first), sort case-sensitive Unicode lines, json-format, json-minify, json-keys (root keys in source order), jsonl-to-array (one valid value per line; no blank lines), array-to-jsonl (root array items to compact JSON lines), json-select (select one value), jsonl-select (select one value from each JSONL record in order; fail on any invalid/missing/ambiguous record with its 1-based position). json-redact (one JSON document) and jsonl-redact (each JSONL record) replace exactly one explicit pointer value of any JSON type with the fixed JSON string [REDACTED], preserving other lexical values/key order and compacting insignificant whitespace. Empty pointer masks the entire document/record. No secret detection, wildcards, type coercion or custom replacement. Missing/ambiguous pointers, invalid Unicode/JSON, depth/record/output bounds fail the complete result. Multiple explicit fields use ordered steps; an earlier parent replacement makes a later child pointer fail. Original input was already supplied to this model: offline pre-redaction is required before disclosure. The two JSON selection operations require an explicit pointer, even empty for root; selected strings stay quoted JSON. csv-to-jsonl imports UTF-8 comma CSV with a required unique nonempty header, strict double quoting and LF/CRLF records; all cells become JSON strings, with no trimming or number inference. Reject blank records, malformed quotes, wrong row widths, more than 1024 columns or header names above 4096 UTF-8 bytes. CSV record1 is the header; multiline quoted fields count as one record. Header-only CSV yields empty JSONL; no implicit repair or row skipping. jsonl-to-csv exports nonempty flat scalar JSONL objects: first record defines ordered unique nonempty columns, later records must have the same columns in any order. Strings decode, number tokens/boolean/null become their exact text; nested values and duplicate decoded keys are errors. All header/cell fields are quoted, inner quotes doubled, LF terminators, no BOM or spreadsheet type/formula sanitization. CSV export has the same 1024 column/4096-byte name caps; empty input has no schema and fails. jsonl-project requires explicit fields (1-64 ordered objects with name and pointer) and no top-level pointer. It selects/renames multiple values per JSONL record into one new object, preserving selected JSON tokens and internal key order/repeated keys. Output names unique, 1-256 UTF-8 bytes, no controls. Missing/ambiguous/invalid references fail the complete call; existing null is valid, no skip/defaults. Projection adds one container; nesting remains <=128. Fields are invalid on other operations, even empty. lines-include and lines-exclude require contains (1-4096 UTF-8 bytes, no CR/LF) and keep whole lines with/without that exact case-sensitive substring; no regex/glob/Unicode normalization. Preserve line order, duplicates and whitespace; count every source line against 100000, even discarded lines. No matches yields empty output successfully. contains is invalid on any other operation, even empty/null. Filtering examines raw lines, not JSON values, CSV records or log severity. JSON formatting/selection preserve tokens/order/repeated keys; line output uses LF. reverse only applies to sort, jsonl-sort or jsonl-sort-int; pointer only applies to JSON selection/filtering/key operations. jsonl-where requires equals as literal decoded string (0-4096 UTF-8 bytes, empty allowed) and explicit pointer. Keep each entire matching record, compact JSON with original number/string tokens and key order/repeats, LF after each. Exact case-sensitive decoded-string equality, no substring/regex/normalization or type coercion: nonstring values do not match. Missing/ambiguous pointers, invalid records/strings or more than100000 total records (including discarded ones) fail the whole call with record position; no partial output. equals is invalid for other operations, even null. To filter numeric/boolean/null values, do not use this string-only operation. jsonl-dedupe, jsonl-sort and jsonl-count-by require an explicit pointer selecting a JSON string in every record (empty selects root). No numeric/date inference, case folding or Unicode normalization. Validate all records, including duplicates: missing/ambiguous/nonstring keys, invalid JSON and more than100000 records fail the complete result with record position. jsonl-dedupe keeps the first complete record per decoded key, in input order. jsonl-sort stably sorts complete records by decoded Unicode string key; reverse=true reverses key order while equal keys keep input order. This is lexical string sorting, not numeric or chronological parsing; timestamps require a common normalized format/timezone for chronological order. Both preserve retained numeric/string tokens, internal key order/repeated unrelated keys, compact JSON and LF endings. jsonl-sort-int requires an explicit pointer to a JSON integer literal in every record: exact signed digit ordering with no float conversion or machine-integer limit, decimal/exponent syntax and strings are rejected; -0 and 0 tie, equal values stay stable in both directions. Use it after counting to rank counts, then a line window for top N. jsonl-count-by emits one JSONL object {value: decoded string, count: integer}, in first-seen key order; equivalent escape spellings count together, emitted key strings are encoded afresh. Empty input produces zero bytes for all three. Sort and group operations buffer bounded records/keys, not a streaming or external sort. JSON selection uses the preceding intermediate, not the original input; use select_input separately only when display text/join is desired. No filesystem, shell, includes or model calls. Do not copy input into arguments. Only final output returns as JSON with output; failure returns an error without intermediates. Limits: 64 KiB arguments, 8 MiB original input, 16 MiB intermediate, 100000 lines/keys/records, depth128, 64 KiB final JSON envelope.","parameters":{"type":"object","properties":{"operation":operation,"contains":contains,"equals":equals,"lines":lines,"fields":projection_fields,"reverse":{"type":"boolean"},"pointer":{"type":"string","description":"Required for json-select, jsonl-select, json-redact, jsonl-redact, jsonl-where, jsonl-dedupe, jsonl-sort, jsonl-sort-int and jsonl-count-by: plain RFC 6901, empty selects root, max4096 UTF-8 bytes/128 tokens. Selected strings stay quoted JSON."},"steps":{"type":"array","minItems":1,"maxItems":16,"items":step}},"oneOf":[{"required":["operation"],"not":{"required":["steps"]}},{"required":["steps"],"not":{"anyOf":[{"required":["operation"]},{"required":["reverse"]},{"required":["pointer"]},{"required":["fields"]},{"required":["contains"]},{"required":["lines"]},{"required":["equals"]}]}}],"additionalProperties":false}}}),
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
