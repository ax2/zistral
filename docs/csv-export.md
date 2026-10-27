# Export JSON Lines objects as CSV

Use v0.21.0 or later to export explicit records without a model or an external script:

```sh
zi data jsonl-to-csv -i records.jsonl -o table.csv
cat records.jsonl | zi data jsonl-to-csv
zi data --step json-select:/data --step array-to-jsonl --step jsonl-to-csv -i response.json
zi data --recipe docs/examples/api-csv.toml -i response.json -o table.csv
```

For these JSONL records (the second object's field order differs):

```jsonl
{"id":"0007","amount":123456789012345678901234567890,"note":"a,b"}
{"note":"one\nline","amount":1.2300E+400,"id":"0008"}
```

The output is UTF-8, comma-delimited CSV with a header, LF record terminators, no BOM,
and **every field quoted**, including headers:

```csv
"id","amount","note"
"0007","123456789012345678901234567890","a,b"
"0008","1.2300E+400","one
line"
```

## Columns and scalar values

- The first record must be a nonempty object. Its source key order defines the header.
  Names must be unique after JSON decoding, nonempty and at most 4096 UTF-8 bytes each.
  Names are case-sensitive, with no trimming or Unicode normalization. Maximum 1024 columns.
- Every later record must have exactly the same decoded column names, in any order.
  Values are aligned to the first header. Missing/extra/duplicate keys fail; no union,
  automatic column renaming, padding or dropped fields.
- JSON strings decode to cell text. Escaped quotes and line breaks become actual cell
  content; CSV doubles quotes inside the surrounding quotes. Whitespace stays unchanged.
- Numbers retain their original token spelling, including huge integers, exponent case,
  trailing fractional zeros and negative zero. They are never parsed as floating point.
  Booleans and null become the text `true`, `false` or `null`.
- Nested objects/arrays are rejected instead of stringified implicitly. Use existing
  JSON selection or reshape the data explicitly first. Root arrays can use `array-to-jsonl`.
- Empty input fails because there is no object from which to infer a header. Empty objects,
  blank JSONL records, nonobject records and invalid JSON also fail. JSONL accepts LF/CRLF
  record endings; a field's line break must be encoded within its JSON string.

CSV has no JSON type metadata. On reimport with `csv-to-jsonl`, **all cells become strings**:
JSON null becomes `"null"`, distinct from an empty cell, but indistinguishable from the original
JSON string `"null"`. String escapes and object order are not byte-round-tripped; cell text
and first-column/record order are preserved. Header-only CSV cannot be reconstructed from
empty JSONL because the column schema has been lost.

Always quoting fields also makes a one-column empty cell unambiguous, keeps commas/newlines
inside a cell, and prevents a BOM-valued first header name from becoming a file BOM.
It does **not** specify spreadsheet cell types or neutralize formulas. A spreadsheet may
reinterpret leading zeros, long numbers or formula-like text even when quoted. The converter
preserves data and does not prepend apostrophes/tabs. View untrusted exports as text or apply
consumer-specific import rules; see [OWASP's CSV injection discussion](https://community.owasp.org/attacks/CSV_Injection).

## Shared pipeline, diagnostics and limits

The operation works in single `zi data`, ordered steps, version-1 recipes and the existing
`transform_input` tool. It takes no pointer; `reverse: true` remains sort-only. For example:

```json
{"steps":[{"operation":"json-select","pointer":"/data"},{"operation":"array-to-jsonl"},{"operation":"jsonl-to-csv"}]}
```

Errors identify a 1-based JSONL record, without echoing keys or cells. A late error fails the
entire invocation: no partial stdout or published `-o` result, even with `--force`. Internally
the writer may have buffered a header or valid prefix; this is complete-result delivery, not
transaction rollback. The existing shell `>` truncation/concurrency/metadata limits still apply.

Limits are unchanged: 8 MiB initial input, 16 MiB per intermediate/CLI output, 100000 JSONL
records (plus the generated CSV header), depth 128, 1–16 steps, 64 KiB tool arguments/recipe
definitions and 64 KiB final tool JSON envelope **including escaping**. Quoted CSV may expand
again inside tool JSON; oversize results fail rather than truncate. Processing is fully buffered,
not unbounded streaming. Each new tool call starts from original input; only its internal
steps consume the previous intermediate. No recipe files are loaded by the tool.

The existing [csv Writer](https://docs.rs/csv/1.4.0/csv/struct.WriterBuilder.html) handles CSV
escaping, with explicit always-quote and LF settings. serde_json's borrowed
[RawValue](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html) keeps scalar
tokens exact after shared syntax/depth validation. No new dependencies, tool names, file/shell
permissions, network calls in offline commands, model runtime or memory are introduced.
