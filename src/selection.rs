//! Borrow selected JSON text; never convert its numeric tokens or build a value tree.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;

#[derive(Clone, Copy, Debug)]
enum Problem {
    Missing,
    Ambiguous,
    Index,
}
type Found<'de> = std::result::Result<&'de RawValue, Problem>;

fn tokens(pointer: &str) -> Result<Vec<String>> {
    if pointer.len() > 4096 {
        bail!("JSON pointer exceeds 4096 UTF-8 bytes");
    }
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    if !pointer.starts_with('/') {
        bail!("use a JSON pointer such as /abc/0 (empty string selects the root)");
    }
    let mut result = Vec::new();
    for token in pointer[1..].split('/') {
        if result.len() == 128 {
            bail!("JSON pointer exceeds 128 reference tokens");
        }
        let mut value = String::new();
        let mut chars = token.chars();
        while let Some(c) = chars.next() {
            if c == '~' {
                value.push(match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => bail!("invalid JSON pointer escape; use ~0 for '~' and ~1 for '/'"),
                });
            } else {
                value.push(c);
            }
        }
        result.push(value);
    }
    Ok(result)
}

pub(crate) fn validate_pointer(pointer: &str) -> Result<()> {
    tokens(pointer).map(|_| ())
}
struct PathSeed<'a>(&'a [String], bool);
impl<'de> DeserializeSeed<'de> for PathSeed<'_> {
    type Value = Found<'de>;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        if self.0.is_empty() {
            <&RawValue>::deserialize(deserializer).map(Ok)
        } else if self.1 {
            // Optional traversal borrows lexical values before visiting containers so an
            // absent child under 1e9999 is missing, not a floating-point parser failure.
            let raw = <&RawValue>::deserialize(deserializer)?;
            if !matches!(raw.get().as_bytes()[0], b'{' | b'[') {
                return Ok(Err(Problem::Missing));
            }
            let mut nested = serde_json::Deserializer::from_str(raw.get());
            nested.disable_recursion_limit();
            serde::Deserializer::deserialize_any(&mut nested, PathVisitor(self.0, true))
                .map_err(de::Error::custom)
        } else {
            deserializer.deserialize_any(PathVisitor(self.0, false))
        }
    }
}
struct PathVisitor<'a>(&'a [String], bool);
impl<'de> Visitor<'de> for PathVisitor<'_> {
    type Value = Found<'de>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON container for the reference token")
    }
    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let mut found = None;
        let mut ambiguous = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == self.0[0] {
                if found.is_some() {
                    ambiguous = true;
                    map.next_value::<IgnoredAny>()?;
                } else {
                    found = Some(map.next_value_seed(PathSeed(&self.0[1..], self.1))?);
                }
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(if ambiguous {
            Err(Problem::Ambiguous)
        } else {
            found.unwrap_or(Err(Problem::Missing))
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let token = &self.0[0];
        let index = if token == "0"
            || (!token.is_empty()
                && !token.starts_with('0')
                && token.bytes().all(|b| b.is_ascii_digit()))
        {
            token.parse::<usize>().ok()
        } else {
            None
        };
        let mut position = 0;
        let mut found = None;
        loop {
            if Some(position) == index {
                match seq.next_element_seed(PathSeed(&self.0[1..], self.1))? {
                    Some(value) => found = Some(value),
                    None => break,
                }
            } else if seq.next_element::<IgnoredAny>()?.is_none() {
                break;
            }
            position += 1;
        }
        Ok(if index.is_none() {
            Err(Problem::Index)
        } else {
            found.unwrap_or(Err(Problem::Missing))
        })
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(Err(Problem::Missing))
    }
}
fn display(raw: &RawValue, limit: usize) -> Result<String> {
    if raw.get().starts_with('"') {
        let text: String = serde_json::from_str(raw.get())
            .map_err(|_| anyhow::anyhow!("selected string cannot be decoded"))?;
        if text.len() > limit {
            bail!("selected output exceeds {limit} bytes");
        }
        Ok(text)
    } else {
        crate::transforms::compact_valid_json(raw.get(), limit)
    }
}
struct Join<'a> {
    separator: &'a str,
    limit: usize,
}
impl<'de> Visitor<'de> for Join<'_> {
    type Value = String;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array to join")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<String, A::Error> {
        let mut output = String::new();
        let mut first = true;
        while let Some(raw) = seq.next_element::<&RawValue>()? {
            if !first {
                if self.separator.len() > self.limit.saturating_sub(output.len()) {
                    return Err(de::Error::custom("joined output exceeds limit"));
                }
                output.push_str(self.separator);
            }
            let text =
                display(raw, self.limit.saturating_sub(output.len())).map_err(de::Error::custom)?;
            output.push_str(&text);
            first = false;
        }
        Ok(output)
    }
}
fn locate<'a>(input: &'a str, pointer: &str, if_present: bool) -> Result<Option<&'a RawValue>> {
    if input.len() > 64 * 1024 * 1024 {
        bail!("JSON selection input exceeds 64 MiB");
    }
    let tokens = tokens(pointer)?;
    crate::transforms::validate_json(input).context("input is not valid JSON")?;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    // Every container was counted by the validation pass before this bounded traversal.
    deserializer.disable_recursion_limit();
    let found = PathSeed(&tokens, if_present)
        .deserialize(&mut deserializer)
        .map_err(|_| anyhow::anyhow!("JSON pointer cannot be evaluated against this input"))?;
    let raw = match found {
        Ok(raw) => Some(raw),
        Err(Problem::Missing) if if_present => None,
        Err(Problem::Ambiguous) => {
            bail!("JSON pointer references an ambiguous duplicate object key")
        }
        Err(Problem::Index) => bail!("JSON pointer requires an array index without leading zeros"),
        Err(Problem::Missing) => bail!("JSON pointer does not exist in the input"),
    };
    deserializer.end().map_err(|_| {
        anyhow::anyhow!("JSON pointer traversal did not consume the complete input")
    })?;
    Ok(raw)
}

fn find<'a>(input: &'a str, pointer: &str) -> Result<&'a RawValue> {
    locate(input, pointer, false)?.context("JSON pointer does not exist in the input")
}

/// Validate the whole record and reference before exact decoded-string comparison.
pub(crate) fn string_equals(input: &str, pointer: &str, expected: &str) -> Result<bool> {
    let raw = find(input, pointer)?;
    if !raw.get().starts_with('"') {
        return Ok(false);
    }
    let text: String = serde_json::from_str(raw.get())
        .map_err(|_| anyhow::anyhow!("selected string cannot be decoded"))?;
    Ok(text == expected)
}

/// Shared key validation includes string scalar validity in unselected fields.
fn key_value<'a>(input: &'a str, pointer: &str) -> Result<&'a RawValue> {
    let raw = find(input, pointer)?;
    validate_strings(input)?;
    Ok(raw)
}

fn validate_strings(input: &str) -> Result<()> {
    // IgnoredAny checks syntax but skips surrogate pairing in unselected strings.
    // Inspect string tokens only: never parse the record's numeric payloads as floats.
    let (mut start, mut escaped) = (None, false);
    for (index, byte) in input.bytes().enumerate() {
        if let Some(begin) = start {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                let token = &input[begin..=index];
                if token.contains("\\u") {
                    serde_json::from_str::<String>(token)
                        .map_err(|_| anyhow::anyhow!("JSON contains an invalid Unicode string"))?;
                }
                start = None;
            }
        } else if byte == b'"' {
            start = Some(index);
        }
    }
    Ok(())
}

/// Validate syntax/depth and all Unicode strings, borrowing the whole lexical record.
pub(crate) fn validated_record(input: &str) -> Result<&RawValue> {
    key_value(input, "")
}

/// Strict decoded-string key; never coerce other JSON types.
pub(crate) fn string_key(input: &str, pointer: &str) -> Result<String> {
    let raw = key_value(input, pointer)?;
    serde_json::from_str::<String>(raw.get())
        .map_err(|_| anyhow::anyhow!("selected key must be a JSON string; no type coercion"))
}

/// Borrow exact integer digits directly from the validated record, without allocating a key.
pub(crate) fn integer_key<'a>(input: &'a str, pointer: &str) -> Result<crate::integer::Key<'a>> {
    crate::integer::Key::parse(key_value(input, pointer)?.get())
}

/// Replace exactly the borrowed value span, never an equal value elsewhere in the record.
/// Validation covers the complete original input before any value is hidden.
pub(crate) fn redact_json(
    input: &str,
    pointer: &str,
    if_present: bool,
    limit: usize,
) -> Result<String> {
    let raw = if if_present {
        let selected = locate(input, pointer, true)?;
        // Even a missing path must validate every original string before publishing.
        validate_strings(input)?;
        match selected {
            Some(raw) => raw.get(),
            None => return crate::transforms::compact_valid_json(input, limit),
        }
    } else {
        key_value(input, pointer)?.get()
    };
    // RawValue borrows a substring of this input. Address arithmetic locates that span
    // without unsafe pointer operations or a textual search that could match another value.
    let start = (raw.as_ptr() as usize)
        .checked_sub(input.as_ptr() as usize)
        .expect("selected RawValue borrows the input");
    let end = start + raw.len();
    let mut redacted = String::with_capacity(input.len() - raw.len() + 12);
    redacted.push_str(&input[..start]);
    redacted.push_str(r#""[REDACTED]""#);
    redacted.push_str(&input[end..]);
    crate::transforms::compact_valid_json(&redacted, limit)
}

/// Pipeline selection always returns a JSON value, including quoted string tokens.
pub(crate) fn select_json(input: &str, pointer: &str, limit: usize) -> Result<String> {
    crate::transforms::compact_valid_json(find(input, pointer)?.get(), limit)
}

pub fn select(input: &str, pointer: &str, separator: Option<&str>, limit: usize) -> Result<String> {
    let raw = find(input, pointer)?;
    if let Some(separator) = separator {
        if !raw.get().starts_with('[') {
            bail!("--join requires the selected value to be an array");
        }
        let mut de = serde_json::Deserializer::from_str(raw.get());
        de.disable_recursion_limit();
        serde::Deserializer::deserialize_seq(&mut de, Join { separator, limit }).map_err(|_| {
            anyhow::anyhow!("cannot join selected array within {limit} bytes or decode its strings")
        })
    } else {
        display(raw, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_pipeline_selection_retains_string_tokens_while_display_decodes() {
        let input = r#"{"x":"\u4e2d\ntext","n":1.2300E+400,"other":1e9999}"#;
        assert_eq!(select_json(input, "/x", 100).unwrap(), r#""\u4e2d\ntext""#);
        assert_eq!(select(input, "/x", None, 100).unwrap(), "中\ntext");
        assert_eq!(select_json(input, "/n", 100).unwrap(), "1.2300E+400");
        assert_eq!(
            select_json(r#" [ {"x":1,"x":2}, -0 ] "#, "", 100).unwrap(),
            r#"[{"x":1,"x":2},-0]"#
        );
        assert!(select_json(input, "/x", 5).is_err());
        assert!(select_json("{\"x\":1} trailing", "/x", 100).is_err());
        assert!(select_json("{\"x\":1,\"x\":2}", "/x", 100).is_err());
    }
    fn apply(input: &str, pointer: &str, separator: Option<&str>) -> String {
        select(input, pointer, separator, 1024 * 1024).unwrap()
    }
    #[test]
    fn lexical_values_and_join_keep_precision_order_duplicates_and_string_contract() {
        let source = r#" {"z":123456789012345678901234567890,"a":1.2300E+400,"zero":-0,"obj":{"z":1,"a":2,"z":3},"text":"\u4e2d\ntext"} "#;
        assert_eq!(apply(source, "/z", None), "123456789012345678901234567890");
        assert_eq!(apply(source, "/a", None), "1.2300E+400");
        assert_eq!(apply(source, "/zero", None), "-0");
        assert_eq!(apply(source, "/obj", None), r#"{"z":1,"a":2,"z":3}"#);
        assert_eq!(apply(source, "/text", None), "中\ntext");
        assert_eq!(
            apply(
                r#"["\u4e2d",123456789012345678901234567890,1.2300E+400,-0,{"z":1,"a":2,"z":3},null,true]"#,
                "",
                Some("|")
            ),
            "中|123456789012345678901234567890|1.2300E+400|-0|{\"z\":1,\"a\":2,\"z\":3}|null|true"
        );
        assert_eq!(
            apply(r#"{"ok":"yes","ignored":1e9999}"#, "/ok", None),
            "yes"
        );
        assert_eq!(apply("[]", "", Some(",")), "");
        assert_eq!(apply(r#""""#, "", None), "");
    }
    #[test]
    fn pointer_escaping_exact_unicode_and_duplicate_references_are_unambiguous() {
        let source =
            r#"{"":0,"a/b":{"~1":["value"]},"01":"key","é":1,"e\u0301":2,"a":1,"\u0061":2}"#;
        assert_eq!(apply(source, "/", None), "0");
        assert_eq!(apply(source, "/a~1b/~01/0", None), "value");
        assert_eq!(apply(source, "/01", None), "key");
        assert_eq!(apply(source, "/é", None), "1");
        assert_eq!(apply(source, "/e\u{301}", None), "2");
        assert!(
            select(source, "/a", None, 100)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
        for pointer in ["/a~", "/a~2", "#/a", "not-a-pointer"] {
            assert!(select(source, pointer, None, 100).is_err());
        }
        for pointer in ["/01", "/+1", "/-", "/999999999999999999999999999999"] {
            assert!(select("[1,2]", pointer, None, 100).is_err());
        }
        assert!(select(r#"{"a":{"x":1},"a":{"x":2}}"#, "/a/x", None, 100).is_err());
        assert_eq!(
            apply(
                r#"{"selected":{"x":1,"x":2},"other":1,"other":2}"#,
                "/selected",
                None
            ),
            r#"{"x":1,"x":2}"#
        );
    }
    #[test]
    fn full_input_validation_depth_pointer_and_output_limits_fail_without_values() {
        for source in ["{\"ok\":1} trailing", "{\"ok\":1,\"bad\":[1,]}", "{bad"] {
            assert!(select(source, "/ok", None, 100).is_err());
        }
        for (source, pointer) in [
            ("null", "/secret"),
            ("123", "/secret"),
            (r#""PRIVATE_VALUE""#, "/secret"),
            ("{}", "/secret"),
        ] {
            let error = select(source, pointer, None, 100).unwrap_err().to_string();
            assert!(!error.contains("PRIVATE_VALUE") && !error.contains("secret"));
        }
        let within = format!("{}1{}", "[".repeat(128), "]".repeat(128));
        assert_eq!(apply(&within, &"/0".repeat(128), None), "1");
        let deeper = format!("[{within}]");
        assert!(select(&deeper, "", None, 1000).is_err());
        assert!(select("{}", &format!("/{}", "a".repeat(4096)), None, 100).is_err());
        assert!(select(&within, &"/0".repeat(129), None, 100).is_err());
        for (source, sep) in [
            ("[\"a\",\"b\"]", Some("long separator")),
            ("\"abcdef\"", None),
            ("{\"n\":123}", None),
        ] {
            assert!(select(source, "", sep, 3).is_err());
        }
        let input = format!("{{\"x\":\"{}\"}}", "x".repeat(65537));
        assert!(crate::data::execute("select_input", r#"{"pointer":"/x"}"#, &input).is_err());
    }
    #[test]
    fn mixed_nested_reference_paths_match_expected_values_without_rounding() {
        for i in 0..200 {
            let source = format!(
                r#"{{"unused":1e9999,"a/b":[{{"~":{{"number":{i},"text":"值{i}"}}}},null]}}"#
            );
            assert_eq!(apply(&source, "/a~1b/0/~0/number", None), i.to_string());
            assert_eq!(apply(&source, "/a~1b/0/~0/text", None), format!("值{i}"));
            assert_eq!(apply(&source, "/a~1b/1", None), "null");
        }
    }
}

#[cfg(test)]
mod string_match_tests {
    use super::*;
    #[test]
    fn decoded_string_match_respects_unicode_pointers_and_complete_record_validation() {
        assert!(string_equals(r#"{"a/b":{"~":"\u0045RROR"}}"#, "/a~1b/~0", "ERROR").unwrap());
        assert!(string_equals(r#""\r\n\t\u0000""#, "", "\r\n\t\0").unwrap());
        assert!(string_equals(r#"["",1.2300E+400]"#, "/0", "").unwrap());
        assert!(!string_equals(r#""é""#, "", "e\u{301}").unwrap());
        for json in ["null", "true", "-0", "1.2300E+400", "[]", "{}"] {
            assert!(!string_equals(json, "", json).unwrap());
        }
        for (json, pointer) in [
            (r#"{"x":"ERROR"} trailing"#, "/x"),
            (r#"{"x":"ERROR","x":"ERROR"}"#, "/x"),
            (r#"["ERROR"]"#, "/00"),
            (r#"{"x":"ERROR"}"#, "/missing"),
            (r#"{"x":"\uD800"}"#, "/x"),
        ] {
            assert!(string_equals(json, pointer, "ERROR").is_err());
        }
    }
}

#[cfg(test)]
mod optional_redaction_tests {
    use super::*;
    #[test]
    fn optional_lookup_distinguishes_missing_from_ambiguous_and_invalid_indexes() {
        for input in [
            r#"{}"#,
            r#"{"a":null}"#,
            r#"{"a":1e9999}"#,
            r#"{"a":true}"#,
            r#"{"a":"s"}"#,
            "1e9999",
            "null",
        ] {
            assert!(locate(input, "/a/b", true).unwrap().is_none(), "{input}");
            assert_eq!(redact_json(input, "/a/b", true, 4096).unwrap(), input);
            assert!(redact_json(input, "/a/b", false, 4096).is_err());
        }
        assert_eq!(
            redact_json(r#"{"a":{"b":null}}"#, "/a/b", true, 4096).unwrap(),
            r#"{"a":{"b":"[REDACTED]"}}"#
        );
        assert_eq!(
            redact_json(r#"{"a":[]}"#, "/a/0/x", true, 4096).unwrap(),
            r#"{"a":[]}"#
        );
        assert_eq!(
            redact_json(r#"{"01":null}"#, "/01", true, 4096).unwrap(),
            r#"{"01":"[REDACTED]"}"#
        );
        for input in [
            r#"{"a":null,"a":{}}"#,
            r#"{"a":{},"\u0061":null}"#,
            r#"{"a":{"b":1,"b":2}}"#,
        ] {
            assert!(redact_json(input, "/a/b", true, 4096).is_err());
        }
        for pointer in [
            "/a/01",
            "/a/-",
            "/a/9999999999999999999999999999999999999999999999",
        ] {
            assert!(redact_json(r#"{"a":[]}"#, pointer, true, 4096).is_err());
        }
        assert_eq!(
            redact_json(r#"{"x":1,"x":2}"#, "/absent", true, 4096).unwrap(),
            r#"{"x":1,"x":2}"#
        );
    }
    #[test]
    fn skipped_records_still_validate_all_data_and_obey_output_limits() {
        for input in [
            r#"{"other":"\ud800"}"#,
            r#"{"a":null,"other":["\udc00"]}"#,
            "{} trailing",
            "",
        ] {
            assert!(redact_json(input, "/a/b", true, 4096).is_err());
        }
        let input = r#" { "n":1.2300E+400,"zero":-0,"s":"\u4e2d" } "#;
        assert_eq!(
            redact_json(input, "/absent", true, 1000).unwrap(),
            r#"{"n":1.2300E+400,"zero":-0,"s":"\u4e2d"}"#
        );
        assert!(redact_json(input, "/absent", true, 4).is_err());
        assert_eq!(redact_json(input, "", true, 12).unwrap(), r#""[REDACTED]""#);
        for depth in [128, 129] {
            let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(
                redact_json(&input, "/0/0/0", true, 4096).is_ok(),
                depth == 128
            );
        }
    }
}
