//! Flat JSONL objects to CSV text. No file access or spreadsheet type/formula inference.
use crate::csv_input::{MAX_COLUMNS, MAX_HEADER};
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize,
    de::{self, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{borrow::Cow, collections::HashMap, fmt, io::Write};

struct Row<'a> {
    names: Vec<String>,
    cells: HashMap<String, Cow<'a, str>>,
}

impl<'de> Deserialize<'de> for Row<'de> {
    fn deserialize<D: de::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Row<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a nonempty object with unique scalar-valued columns")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Row<'de>, A::Error> {
                let mut row = Row {
                    names: Vec::new(),
                    cells: HashMap::new(),
                };
                while let Some(name) = map.next_key::<String>()? {
                    if row.names.len() == MAX_COLUMNS {
                        return Err(de::Error::custom("object exceeds 1024 columns"));
                    }
                    if name.is_empty() || name.len() > MAX_HEADER {
                        return Err(de::Error::custom(
                            "column names must contain 1-4096 UTF-8 bytes",
                        ));
                    }
                    if row.cells.contains_key(&name) {
                        return Err(de::Error::custom("duplicate decoded column name"));
                    }
                    let raw = map.next_value::<&RawValue>()?.get();
                    let cell = match raw.as_bytes()[0] {
                        b'"' => Cow::Owned(
                            serde_json::from_str::<String>(raw).map_err(de::Error::custom)?,
                        ),
                        b'{' | b'[' => {
                            return Err(de::Error::custom(
                                "CSV cells require scalar values; nested objects/arrays are unsupported",
                            ));
                        }
                        _ => Cow::Borrowed(raw), // Validated JSON number, boolean or null, with exact token spelling.
                    };
                    row.cells.insert(name.clone(), cell);
                    row.names.push(name);
                }
                if row.names.is_empty() {
                    return Err(de::Error::custom(
                        "a nonempty object is required to define CSV columns",
                    ));
                }
                Ok(row)
            }
        }
        de.deserialize_map(Fields)
    }
}

pub fn from_jsonl(input: &str, out: &mut impl Write, max_records: usize) -> Result<()> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b',')
        .has_headers(false)
        .quote_style(csv::QuoteStyle::Always)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(out);
    let mut headers = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if index == max_records {
            bail!("CSV export accepts at most {max_records} JSONL records");
        }
        let record = index + 1;
        crate::transforms::validate_json(line)
            .with_context(|| format!("cannot export JSONL record {record}"))?;
        if !line.trim_start().starts_with('{') {
            bail!("JSONL record {record} must be a nonempty object of scalar-valued columns");
        }
        // Root type and complete syntax were checked first; only our constant semantic errors
        // (plus line/column positions) reach diagnostics, never source keys or cell values.
        let row: Row<'_> = serde_json::from_str(line)
            .with_context(|| format!("cannot export JSONL record {record}"))?;
        if index == 0 {
            headers = row.names;
            writer
                .write_record(&headers)
                .context("cannot encode CSV header within output limit")?;
        }
        if row.cells.len() != headers.len()
            || headers.iter().any(|name| !row.cells.contains_key(name))
        {
            bail!("JSONL record {record} must have the same columns as record 1");
        }
        writer
            .write_record(headers.iter().map(|name| row.cells[name].as_ref()))
            .with_context(|| {
                format!("cannot encode JSONL record {record} within CSV output limit")
            })?;
    }
    if headers.is_empty() {
        bail!("jsonl-to-csv requires at least one nonempty JSON object to define columns");
    }
    writer
        .flush()
        .context("cannot flush CSV within output limit")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(input: &str, max: usize) -> Result<String> {
        let mut bytes = Vec::new();
        from_jsonl(input, &mut bytes, max)?;
        Ok(String::from_utf8(bytes).unwrap())
    }
    #[test]
    fn scalar_text_and_first_column_order_survive_csv_reader_and_import() {
        let input = concat!(
            "{\"id\":\"0007\",\"amount\":123456789012345678901234567890,\"note\":\"中,\\\"文\\\"\\r\\nline\",\"flag\":true,\"empty\":null}\n",
            "{\"empty\":\"\",\"flag\":false,\"note\":\" \",\"amount\":1.2300E+400,\"id\":-0}\n"
        );
        let csv = convert(input, 2).unwrap();
        assert!(csv.starts_with("\"id\",\"amount\",\"note\",\"flag\",\"empty\"\n"));
        let rows = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows[0].iter().collect::<Vec<_>>(),
            [
                "0007",
                "123456789012345678901234567890",
                "中,\"文\"\r\nline",
                "true",
                "null"
            ]
        );
        assert_eq!(
            rows[1].iter().collect::<Vec<_>>(),
            ["-0", "1.2300E+400", " ", "false", ""]
        );
        let mut jsonl = Vec::new();
        crate::csv_input::to_jsonl(&csv, &mut jsonl, 2).unwrap();
        let values = String::from_utf8(jsonl)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values[0]["amount"], "123456789012345678901234567890");
        assert_eq!(values[0]["empty"], "null");
        assert_eq!(values[1]["empty"], "");
    }
    #[test]
    fn quoted_headers_and_cells_keep_bom_empty_controls_and_formula_text() {
        let input =
            r#"{"\ufeffid":"=1+2","\r\n,\"":"\u0000\t\r\n\\","space":" ","empty":""}"#.to_string();
        let csv = convert(&input, 1).unwrap();
        assert!(csv.starts_with('"'));
        assert!(csv.starts_with("\"\u{feff}id\""));
        assert!(!csv.starts_with('\u{feff}'));
        let mut result = Vec::new();
        crate::csv_input::to_jsonl(&csv, &mut result, 1).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&result).unwrap(),
            serde_json::from_str::<serde_json::Value>(&input).unwrap()
        );
        assert_eq!(convert("{\"x\":\"\"}\n", 1).unwrap(), "\"x\"\n\"\"\n");
    }
    #[test]
    fn shape_name_and_record_limits_fail_without_source_values() {
        for input in [
            "",
            "\n",
            "{}",
            "[]",
            "\"PRIVATE_VALUE\"",
            "{\"PRIVATE_KEY\":{}}",
            "{\"PRIVATE_KEY\":[]}",
            "{\"\":1}",
            "{\"id\":1,\"i\\u0064\":2}",
            "{\"id\":1}\n{\"PRIVATE_KEY\":2}\n",
            "{\"id\":1}\n{\"id\":1,\"x\":\"PRIVATE_VALUE\"}\n",
            "{\"id\":1}\n{\"id\":\"PRIVATE_VALUE\"",
        ] {
            let message = format!("{:#}", convert(input, 2).unwrap_err());
            assert!(!message.contains("PRIVATE"), "{message}");
        }
        assert!(
            format!(
                "{:#}",
                convert("{\"id\":1}\n{\"id\":2}\n{\"x\":3}", 3).unwrap_err()
            )
            .contains("record 3")
        );
        assert!(convert("{\"id\":1}\n{\"id\":2}\n", 1).is_err());
        let columns = |count| {
            format!(
                "{{{}}}",
                (0..count)
                    .map(|i| format!("\"c{i}\":0"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        assert!(convert(&columns(1024), 1).is_ok());
        assert!(convert(&columns(1025), 1).is_err());
        assert!(convert(&format!("{{\"{}\":0}}", "界".repeat(1365) + "x"), 1).is_ok());
        assert!(convert(&format!("{{\"{}\":0}}", "界".repeat(1365) + "xx"), 1).is_err());
    }
}
