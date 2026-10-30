//! Value-free structural reports for explicit JSON/JSONL input. Arrays are opaque leaves.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    io::Write,
};

const MAX_PATHS: usize = 1024;
const TYPES: [&str; 6] = ["array", "boolean", "null", "number", "object", "string"];

struct Fields<'a>(Vec<(String, &'a RawValue)>);
impl<'de> Deserialize<'de> for Fields<'de> {
    fn deserialize<D: de::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Fields<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object with bounded unique decoded keys")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut fields = Vec::new();
                let mut names = HashSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if fields.len() == MAX_PATHS || !names.insert(name.clone()) {
                        return Err(de::Error::custom(
                            "profile requires bounded unique decoded object keys",
                        ));
                    }
                    fields.push((name, map.next_value::<&RawValue>()?));
                }
                Ok(Fields(fields))
            }
        }
        de.deserialize_map(Object)
    }
}

#[derive(Default)]
struct Profile {
    records: usize,
    paths: BTreeMap<String, [usize; 6]>,
}
impl Profile {
    fn register(&mut self, path: &str) -> Result<()> {
        crate::selection::validate_pointer(path)
            .context("profile path exceeds supported JSON pointer bounds")?;
        if !self.paths.contains_key(path) {
            if self.paths.len() == MAX_PATHS {
                bail!("profile exceeds 1024 distinct paths including the root");
            }
            self.paths.insert(path.to_owned(), [0; 6]);
        }
        Ok(())
    }

    fn observe(&mut self, input: &str) -> Result<()> {
        let root = crate::selection::validated_record(input)?;
        self.register("")?;
        let mut pending = vec![(String::new(), root)];
        while let Some((path, raw)) = pending.pop() {
            let kind = match raw.get().as_bytes()[0] {
                b'[' => 0,
                b't' | b'f' => 1,
                b'n' => 2,
                b'{' => 4,
                b'"' => 5,
                _ => 3,
            };
            self.paths
                .get_mut(&path)
                .expect("path registered before queueing")[kind] += 1;
            if kind != 4 {
                continue;
            }
            let mut de = serde_json::Deserializer::from_str(raw.get());
            de.disable_recursion_limit();
            let Fields(fields) = Fields::deserialize(&mut de).map_err(|_| {
                anyhow::anyhow!(
                    "profile requires unique decoded object keys and at most 1024 fields per object"
                )
            })?;
            for (name, value) in fields.into_iter().rev() {
                let child = format!("{path}/{}", name.replace('~', "~0").replace('/', "~1"));
                // Register before enqueueing so wide/deep objects cannot build an unbounded
                // pending stack of owned paths. Arrays never produce index paths.
                self.register(&child)?;
                pending.push((child, value));
            }
        }
        self.records += 1;
        Ok(())
    }

    fn write(&self, out: &mut impl Write) -> Result<()> {
        #[derive(Serialize)]
        struct Path<'a> {
            pointer: &'a str,
            present: usize,
            missing: usize,
            types: BTreeMap<&'static str, usize>,
        }
        #[derive(Serialize)]
        struct Report<'a> {
            records: usize,
            paths: Vec<Path<'a>>,
        }
        let paths = self
            .paths
            .iter()
            .map(|(pointer, counts)| {
                let present = counts.iter().sum();
                Path {
                    pointer,
                    present,
                    missing: self.records - present,
                    types: TYPES
                        .into_iter()
                        .zip(counts.iter().copied())
                        .filter(|(_, n)| *n != 0)
                        .collect(),
                }
            })
            .collect();
        serde_json::to_writer(
            out,
            &Report {
                records: self.records,
                paths,
            },
        )
        .context("cannot encode JSON profile within output limit")
    }
}

pub fn json(input: &str, out: &mut impl Write) -> Result<()> {
    let mut profile = Profile::default();
    profile
        .observe(input)
        .context("cannot profile JSON document")?;
    profile.write(out)
}

pub fn jsonl(input: &str, out: &mut impl Write, max_records: usize) -> Result<()> {
    let mut profile = Profile::default();
    for (index, line) in input.lines().enumerate() {
        if index == max_records {
            bail!("JSONL profiling accepts at most {max_records} records");
        }
        profile
            .observe(line)
            .with_context(|| format!("cannot profile JSONL record {}", index + 1))?;
    }
    profile.write(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn report(input: &str) -> Value {
        let mut out = Vec::new();
        jsonl(input, &mut out, 100000).unwrap();
        serde_json::from_slice(&out).unwrap()
    }
    #[test]
    fn object_paths_count_presence_and_types_without_values_or_array_indexes() {
        let value = report(concat!(
            "{\"user\":{\"email\":\"PRIVATE_SAMPLE\"},\"a/b\":{\"~\":null},\"arr\":[{\"hidden\":9}],\"n\":1e9999}\n",
            "{\"user\":null,\"a/b\":{\"~\":true},\"arr\":[],\"n\":-0}\n",
            "{\"user\":{\"email\":null},\"n\":\"text\"}\n",
            "null\n"
        ));
        assert_eq!(
            value,
            json!({"records":4,"paths":[
                {"pointer":"","present":4,"missing":0,"types":{"object":3,"null":1}},
                {"pointer":"/arr","present":2,"missing":2,"types":{"array":2}},
                {"pointer":"/a~1b","present":2,"missing":2,"types":{"object":2}},
                {"pointer":"/a~1b/~0","present":2,"missing":2,"types":{"null":1,"boolean":1}},
                {"pointer":"/n","present":3,"missing":1,"types":{"number":2,"string":1}},
                {"pointer":"/user","present":3,"missing":1,"types":{"object":2,"null":1}},
                {"pointer":"/user/email","present":2,"missing":2,"types":{"string":1,"null":1}}
            ]})
        );
        let text = value.to_string();
        assert!(!text.contains("PRIVATE_SAMPLE") && !text.contains("hidden"));
        assert_eq!(report(""), json!({"records":0,"paths":[]}));
        let mixed = report("[]\nfalse\nnull\n900719925474099312345678901234567890\n{}\n\"s\"\n");
        assert_eq!(
            mixed["paths"][0]["types"],
            json!({"array":1,"boolean":1,"null":1,"number":1,"object":1,"string":1})
        );
        assert_eq!(
            report("{\"\":{\"é\":0,\"e\\u0301\":null},\"\\u4e2d\":0}\n{\"中\":true}\n")["paths"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
    }
    #[test]
    fn malformed_unicode_duplicates_and_bounds_fail_before_any_report_is_written() {
        for invalid in [
            "",
            "{",
            "{} trailing",
            r#"{"x":1,"x":2}"#,
            r#"{"x":1,"\u0078":2}"#,
            r#"{"parent":{"x":1,"x":2}}"#,
            r#"{"arr":["\ud800"]}"#,
        ] {
            let mut out = Vec::new();
            assert!(
                jsonl(&format!("{{}}\n{invalid}\n"), &mut out, 100000).is_err(),
                "{invalid}"
            );
            assert!(out.is_empty());
        }
        // Opaque arrays do not expose or structurally inspect their contained object keys.
        assert_eq!(
            report(r#"{"arr":[{"x":1,"x":2}]}"#)["paths"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let wide = |n| {
            format!(
                "{{{}}}",
                (0..n)
                    .map(|i| format!("\"k{i}\":0"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        assert_eq!(report(&wide(1023))["paths"].as_array().unwrap().len(), 1024);
        for input in [
            wide(1024),
            format!("{}\n{{\"new\":0}}", wide(1023)),
            format!("{{\"{}\":0}}", "x".repeat(4096)),
            format!("{{\"{}\":0}}", "/".repeat(2048)),
        ] {
            let mut out = Vec::new();
            assert!(jsonl(&input, &mut out, 100000).is_err());
            assert!(out.is_empty());
        }
        assert_eq!(
            report(&format!("{{\"{}\":0}}", "x".repeat(4095)))["paths"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for depth in [128, 129] {
            let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(json(&input, &mut Vec::new()).is_ok(), depth == 128);
        }
    }
}
