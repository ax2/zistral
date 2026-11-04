# Exact integer field filtering / 精确整数条件筛选（v0.33.0）

Since v0.33.0, `jsonl-where-int` addresses a concrete gap: `jsonl-count-by` produces integer
counts, while the existing `jsonl-where` compares strings only.

```sh
zi data jsonl-where-int --pointer /count --equals 2 -i counts.jsonl
zi data jsonl-where-int --pointer /id --equals 9007199254740993 -i records.jsonl
zi data jsonl-where-int --pointer /offset --equals=-2 -i records.jsonl
zi data jsonl-where-int --pointer /count --equals 2 --show-plan -o exactly-two.toml
zi data --recipe exactly-two.toml -i another-counts.jsonl
```

Use one explicit RFC 6901 pointer and a required `equals` JSON integer literal,
at most 4096 UTF-8 bytes. It has an optional minus followed by digits, with no
leading zeros except `0`/`-0`. Empty text, `+2`, `02`, quoted strings, decimal or
exponent forms fail during definition validation before input/output access.
For a negative CLI argument use `--equals=-2`. The empty pointer selects root
integer records. Whole matching records retain original number/string tokens,
key order and unrelated duplicate keys; output order stays unchanged, with LF
endings. `0` and `-0` match each other without rewriting either source spelling.
There is no machine-integer precision bound, floating-point conversion or coercion.

Every selected value must be an integer literal. A string `"2"`, `2.0`, `2e0`,
boolean, null or container is an error rather than a nonmatching record. Missing or
ambiguous paths and invalid array indexes also fail. The original JSON and Unicode
strings are checked even in discarded records. A malformed later record discards
the complete result and preserves an existing `-o` file, including with `--force`.
Empty input yields zero bytes after definition validation. Existing byte/depth and
100000-record caps apply to all records, including discarded ones.

For example, inspect groups occurring exactly twice:

```toml
version = 1

[[steps]]
operation = "jsonl-count-by"
pointer = "/level"

[[steps]]
operation = "jsonl-where-int"
pointer = "/count"
equals = "2"

[[steps]]
operation = "jsonl-to-tsv"
```

Save as a recipe, then run `zi data --recipe FILE -i events.jsonl`. The source
records need string `/level` values because counting retains its existing contract.
CSV/TSV import intentionally produces strings: importing a cell spelled `2` does
not make it an integer. Use existing string filtering for those original cells;
the counted results above have actual JSON integer fields.

This operation is shared by offline commands, version-1 recipes and the existing
`transform_input` tool, without adding a tool name, query language, runtime package,
permission or task memory. It starts from original input in each tool invocation;
only ordered steps within that invocation consume a previous intermediate. It
does not offer inequalities, ranges, expressions or implicit type conversion.
