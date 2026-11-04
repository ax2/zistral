# Explicit TSV exchange / 显式 TSV 表格交换（未发布）

The next development batch adds `tsv-to-jsonl` and `jsonl-to-tsv` for tab-separated
tables. Installed v0.32.0 does not contain these operations yet. This is one
interoperability improvement, accumulated with other useful work before release.

```sh
zi data tsv-to-jsonl -i table.tsv -o records.jsonl
zi data --step tsv-to-jsonl --step jsonl-count-by:/level --step jsonl-to-csv -i events.tsv
zi data jsonl-to-tsv -i records.jsonl -o table.tsv
zi data --step csv-to-jsonl --step jsonl-to-tsv -i table.csv -o table.tsv
zi data --step tsv-to-jsonl --step jsonl-to-csv --show-plan -o exchange.toml
zi data --recipe exchange.toml -i table.tsv -o table.csv
```

TSV uses a literal tab separator and the existing strict CSV-style double quoting.
Quote a field containing tabs/newlines/double quotes; double an embedded quote.
Commas are ordinary text. Backslash sequences such as `\t` are literal characters,
not escapes. This is an explicit quoted TSV dialect; it does not claim compatibility
with every producer's escaping convention. No delimiter detection or extension-based
format guessing occurs. `csv-to-jsonl` continues to treat tabs as cell content.

Import requires one header record with unique nonempty UTF-8 column names and the
same number of cells in each later record. Cells stay strings: `0007`, `true`, `null`
and large number spellings are not inferred as JSON types. LF/CRLF records and
quoted multiline cells are supported. No trimming, skipped blank records, quote
repair or wrong-width row recovery occurs. A header-only table yields zero bytes.
The CLI removes one initial UTF-8 BOM; a BOM inside a quoted name/value remains data.

Export requires at least one nonempty flat scalar JSON object. The first object's
key order defines header order; later objects must have exactly the same unique
decoded column names, in any order. Strings decode; number/boolean/null tokens
become their original text. Nested values, duplicate decoded keys and blank records
fail. Every header and cell is double quoted, quotes are doubled, rows end with LF,
and no BOM is emitted. Reimport produces strings, so this is not JSON-type roundtrip.
Quoting does not sanitize spreadsheet formulas or control a spreadsheet's type guesses.

Use the existing `jsonl-project` step first to choose/rename scalar columns in a
nested record. Existing `--show-plan` exports a reusable version-1 definition without
reading input; recipes and the existing `transform_input` share the same operations.
They add no tool names, executable runtime, provider settings, task memory or permissions.

The existing caps apply: 8 MiB initial input, 16 MiB intermediate/CLI result,
100000 data records plus an import header, 1024 columns and 4096 UTF-8 bytes per
header name. Model tools additionally cap the serialized final JSON envelope at
64 KiB; quotes/backslashes/control characters count after JSON escaping. Processing
is bounded in memory, not a streaming table engine. Invalid later rows fail the
complete task without stdout intermediates or replacement of an existing `-o` file.
Shell redirection has the shell's own truncation behavior; prefer `-o FILE`.


The same development batch adds [exact integer field filtering](integer-filtering.md)
and [explicit optional projection values](jsonl-projection.md#explicit-defaults-for-absent-fields--显式缺失值未发布).
For a heterogeneous nested table, inspect/edit
[`optional-event-table.toml`](examples/optional-event-table.toml) and then choose it
explicitly with `--recipe`. These are accumulated related outcomes, not separate
TSV/import/export patch releases. The example is a user-maintained definition;
installed zi still needs no companion recipe files for ordinary operations.
