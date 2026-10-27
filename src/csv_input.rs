//! One explicit CSV dialect, decoded in memory. No file, model or shell access.
use anyhow::{Context, Result, bail};
use serde::{Serializer as _, ser::SerializeMap};
use std::{collections::HashSet, io::Write};

const MAX_COLUMNS: usize = 1024;
const MAX_HEADER: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Unquoted,
    Quoted,
    Closed,
}

// csv deliberately accepts malformed quoting and skips empty lines. Check our narrower
// dialect first so decoding cannot silently repair a field or omit a logical record.
fn validate(input: &str, max_records: usize) -> Result<()> {
    if input.starts_with('\u{feff}') {
        bail!("unexpected BOM before CSV header; CLI input removes one leading UTF-8 BOM");
    }
    let bytes = input.as_bytes();
    let (mut state, mut record, mut columns, mut started, mut index) =
        (State::Start, 1usize, 1usize, false, 0usize);
    while index < bytes.len() {
        if record > max_records + 1 {
            bail!("CSV accepts at most {max_records} data records plus a header");
        }
        let byte = bytes[index];
        if state == State::Quoted {
            if byte == b'"' {
                state = State::Closed;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                if !matches!(state, State::Start | State::Closed) {
                    bail!("invalid quote in CSV record {record}");
                }
                state = State::Quoted;
            }
            b',' => {
                columns += 1;
                if columns > MAX_COLUMNS {
                    bail!("CSV record {record} exceeds 1024 columns");
                }
                state = State::Start;
            }
            b'\r' | b'\n' => {
                if byte == b'\r' {
                    if bytes.get(index + 1) != Some(&b'\n') {
                        bail!("bare CR outside quoted field in CSV record {record}");
                    }
                    index += 1;
                }
                if !started {
                    bail!("blank CSV record {record}; quote an empty single-column cell");
                }
                state = State::Start;
                columns = 1;
                record += 1;
                started = false;
                index += 1;
                continue;
            }
            _ => {
                if state == State::Closed {
                    bail!("unexpected text after closing quote in CSV record {record}");
                }
                state = State::Unquoted;
            }
        }
        started = true;
        index += 1;
    }
    if state == State::Quoted {
        bail!("unclosed quoted field in CSV record {record}");
    }
    Ok(())
}

pub fn to_jsonl(input: &str, out: &mut impl Write, max_records: usize) -> Result<()> {
    validate(input, max_records)?;
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b',')
        .has_headers(false)
        .flexible(true) // Check lengths ourselves to keep diagnostics free of cell values.
        .trim(csv::Trim::None)
        .from_reader(input.as_bytes());
    let mut headers = csv::StringRecord::new();
    if !reader
        .read_record(&mut headers)
        .map_err(|_| anyhow::anyhow!("cannot decode CSV header"))?
    {
        bail!("csv-to-jsonl requires a header record");
    }
    let mut seen = HashSet::new();
    for name in &headers {
        if name.is_empty() || name.len() > MAX_HEADER {
            bail!("CSV header names must contain 1-4096 UTF-8 bytes");
        }
        if !seen.insert(name) {
            bail!("CSV header names must be unique");
        }
    }
    let mut row = csv::StringRecord::new();
    let mut record = 2;
    while reader
        .read_record(&mut row)
        .map_err(|_| anyhow::anyhow!("cannot decode CSV record {record}"))?
    {
        if row.len() != headers.len() {
            bail!("CSV record {record} must have {} columns", headers.len());
        }
        let mut serializer = serde_json::Serializer::new(&mut *out);
        let mut object = serializer.serialize_map(Some(headers.len()))?;
        for (name, cell) in headers.iter().zip(row.iter()) {
            object.serialize_entry(name, cell).with_context(|| {
                format!("cannot encode CSV record {record} within output limit")
            })?;
        }
        object.end()?;
        out.write_all(b"\n")?;
        record += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input: &str, max_records: usize) -> Result<String> {
        let mut bytes = Vec::new();
        to_jsonl(input, &mut bytes, max_records)?;
        Ok(String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn csv_writer_generated_fields_keep_order_unicode_quotes_and_multiline_strings() {
        let cells = [
            "",
            "0007",
            "123456789012345678901234567890",
            "1.2300E+400",
            "-0",
            "null",
            "true",
            " 中 文 ",
            "a,b",
            "a\"b",
            "\"",
            "\"\"",
            "a\r\nb\nc\rd",
            "\\path",
            "\t",
            "\0",
            "{{input}}",
        ];
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(Vec::new());
        writer.write_record(["z", "a"]).unwrap();
        for cell in cells {
            writer.write_record(["id", cell]).unwrap();
        }
        let input = String::from_utf8(writer.into_inner().unwrap()).unwrap();
        let output = convert(&input, 100).unwrap();
        let records: Vec<_> = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect();
        assert_eq!(records.len(), cells.len());
        for (record, cell) in records.iter().zip(cells) {
            assert_eq!(record["z"], "id");
            assert_eq!(record["a"].as_str(), Some(cell));
        }
        assert!(output.starts_with("{\"z\":\"id\",\"a\":\"\"}\n"));
        assert_eq!(convert("h\n\"\"", 1).unwrap(), "{\"h\":\"\"}\n");
        assert_eq!(convert(" h \n v ", 1).unwrap(), "{\" h \":\" v \"}\n");
        assert_eq!(convert("id,name\n", 0).unwrap(), "");
    }

    #[test]
    fn csv_strict_dialect_rejects_ambiguous_headers_and_repairs_without_echoing_data() {
        for input in [
            "",
            "a,a\n1,2",
            "a,\n1,2",
            "a,b\nx\n",
            "a\n\"PRIVATE",
            "a\nx\"PRIVATE",
            "a\n\"x\"PRIVATE",
            "a\n\n",
            "a\nx\n\n",
            "a\rb",
            "\u{feff}a\nx",
        ] {
            let error = format!("{:#}", convert(input, 10).unwrap_err());
            assert!(!error.contains("PRIVATE"), "{error}");
        }
        let error = format!(
            "{:#}",
            convert("a,b\n\"one\ntwo\",ok\nx\n", 10).unwrap_err()
        );
        assert!(error.contains("record 3"), "{error}");
        let header = "x".repeat(MAX_HEADER);
        assert!(convert(&header, 0).is_ok());
        assert!(convert(&(header + "x"), 0).is_err());
    }

    #[test]
    fn csv_record_and_column_caps_include_quoted_records_but_not_header() {
        assert!(convert("h\n\"one\ntwo\"\nx\n", 2).is_ok());
        assert!(convert("h\n\"one\ntwo\"\nx\ny", 2).is_err());
        let headers = (0..MAX_COLUMNS)
            .map(|n| format!("c{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let row = vec!["x"; MAX_COLUMNS].join(",");
        assert!(convert(&format!("{headers}\n{row}"), 1).is_ok());
        assert!(convert(&format!("{headers},extra\n{row},x"), 1).is_err());
    }
}
