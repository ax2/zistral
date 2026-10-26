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
struct PathSeed<'a>(&'a [String]);
impl<'de> DeserializeSeed<'de> for PathSeed<'_> {
    type Value = Found<'de>;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        if self.0.is_empty() {
            <&RawValue>::deserialize(deserializer).map(Ok)
        } else {
            deserializer.deserialize_any(PathVisitor(self.0))
        }
    }
}
struct PathVisitor<'a>(&'a [String]);
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
                    found = Some(map.next_value_seed(PathSeed(&self.0[1..]))?);
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
                match seq.next_element_seed(PathSeed(&self.0[1..]))? {
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
pub fn select(input: &str, pointer: &str, separator: Option<&str>, limit: usize) -> Result<String> {
    if input.len() > 64 * 1024 * 1024 {
        bail!("JSON selection input exceeds 64 MiB");
    }
    let tokens = tokens(pointer)?;
    crate::transforms::validate_json(input).context("input is not valid JSON")?;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    // Every container was counted by the validation pass before this bounded traversal.
    deserializer.disable_recursion_limit();
    let found = PathSeed(&tokens)
        .deserialize(&mut deserializer)
        .map_err(|_| anyhow::anyhow!("JSON pointer cannot be evaluated against this input"))?;
    let raw = match found {
        Ok(raw) => raw,
        Err(Problem::Ambiguous) => {
            bail!("JSON pointer references an ambiguous duplicate object key")
        }
        Err(Problem::Index) => bail!("JSON pointer requires an array index without leading zeros"),
        Err(Problem::Missing) => bail!("JSON pointer does not exist in the input"),
    };
    deserializer.end().map_err(|_| {
        anyhow::anyhow!("JSON pointer traversal did not consume the complete input")
    })?;
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
