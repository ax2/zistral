# Import CSV as JSON Lines

Use v0.20.0 or later to turn a UTF-8 comma-separated table into JSONL records, without a
model or an external script:

```sh
zi data csv-to-jsonl -i table.csv
cat table.csv | zi data --step csv-to-jsonl --step jsonl-select:/id --step dedupe --step jsonl-to-array
zi data --recipe docs/examples/csv-ids.toml -i table.csv -o ids.json
```

For this input:

```csv
id,note
0007,"中,文"
0007,"line1
line2"
```

Import produces:

```jsonl
{"id":"0007","note":"中,文"}
{"id":"0007","note":"line1\nline2"}
```

The compound command selects IDs, keeps the first occurrence of each exact JSON line,
and collects them as `["0007"]`. Header and record order are preserved. Every cell is a
JSON string: leading zeros, large integers, exponent text, `null`, `false` and empty cells
are not guessed into other JSON types. Whitespace is preserved; there is no automatic trim.
CSV quoting is decoded into the cell value, then encoded as JSON, not copied byte-for-byte.

## One explicit dialect

- A nonempty header record is required. Decoded names must be unique, nonempty and at most
  4096 UTF-8 bytes each. Names are case-sensitive and not trimmed or Unicode-normalized.
- Comma is the delimiter. A double quote may open a field only at its beginning; a quoted
  field uses doubled quotes for literal quotes. Closing quotes may be followed only by a
  comma, record terminator or EOF. Spaces after a closing quote are rejected.
- LF and CRLF end records. Quoted cells may contain commas, quotes, LF, CRLF or bare CR;
  those remain cell contents. Bare CR outside quoted fields is rejected.
- Each data record must have exactly the header's column count. Blank logical records are
  rejected rather than skipped; use `""` to represent an empty cell in a single-column row.
  A header alone produces empty JSONL; an empty file fails because it has no header.
- CLI input retains existing normalization of one initial UTF-8 BOM. A remaining BOM
  before the header is rejected rather than stripped again. Output has no BOM.

This is a documented zi dialect inspired by common [CSV conventions](https://www.rfc-editor.org/rfc/rfc4180),
with Unicode and LF support. It does not promise to accept every spreadsheet export, TSV,
semicolon-separated CSV, comments, `sep=` directives, alternate encodings or compressed files.
This import operation does not evaluate spreadsheets/formulas. Since v0.21.0, the separate
[`jsonl-to-csv` operation](csv-export.md) exports scalar object records as CSV; reimport still
turns every cell into a string.

## Validation and failure behavior

The mature [Rust csv reader](https://docs.rs/csv/1.4.0/csv/struct.Reader.html#error-handling)
deliberately favors parsing over rejecting malformed quoting. zi therefore checks its
quoting, blank-record and record/column-count limits first, then uses that reader for actual
field decoding. Header and row-shape checks precede JSON object serialization. It never
silently repairs malformed quoting, renames duplicate headers, pads cells or drops rows.

Errors identify the header or a logical CSV record number. Record 1 is the header; embedded
line breaks in quoted fields do not increment this number. Diagnostics do not echo cell or
header values. A later error fails the entire invocation: no partial stdout or published
`-o` result, including with `--force`. Existing result-writer limitations for shell `>`
truncation, concurrent writers and file metadata still apply. CSV decoding may internally
prepare intermediate data; this is complete-result delivery, not task transaction rollback.

Avoid applying raw line operations before CSV import when quoted cells contain line breaks:
those physical lines belong to one cell. Apply field selection/deduplication after conversion
to JSONL, where embedded line breaks are escaped within each record.

## Recipes, tools and limits

The operation works in single `zi data`, ordered `--step`, explicit version-1 recipes and
the existing `transform_input` tool:

```json
{"steps":[{"operation":"csv-to-jsonl"},{"operation":"jsonl-select","pointer":"/id"},{"operation":"dedupe"},{"operation":"jsonl-to-array"}]}
```

It takes no `pointer`; `reverse: true` remains sort-only. Each new tool invocation starts
from original supplied input, while steps within a call consume the preceding intermediate.
Errors return normal tool observations, allowing correction of options or a different plan.
No recipe files are loaded by the model tool, and no file/shell permissions are added.

Limits: 8 MiB initial input, 16 MiB per intermediate/CLI output, 100000 data records plus
the header, 1024 columns per record, 1–16 steps and 64 KiB tool arguments/recipe definitions.
The final model-tool JSON envelope including escaping must fit 64 KiB, or the call fails.
Repeated long column names can expand CSV substantially; output limits still apply. This
is bounded, fully buffered processing. Increasing `--max-input` does not raise its initial cap.

The installed single executable includes the CSV parser. Two Rust packages (`csv`, `csv-core`)
are added; existing lower-level packages are reused. No Python/Node runtime, model weights,
new tool names, project rights, network calls in offline commands or memory are introduced.
