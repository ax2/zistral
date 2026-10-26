# Nested JSON selection in reusable data pipelines

For API responses that wrap records in an envelope, keep extraction and reshaping in one
reviewable definition. No model configuration is needed for these commands.

```sh
zi data --step json-select:/data/items --step array-to-jsonl -i response.json
zi data --show-plan --step json-select:/data/items --step array-to-jsonl -o records.toml
zi data --recipe records.toml -i response.json -o records.jsonl
```

Given `{"data":{"items":[{"id":123456789012345678901234567890},-0]}}`, the result is:

```jsonl
{"id":123456789012345678901234567890}
-0
```

The exported version-1 TOML includes the explicit pointer, never input/output data or paths:

```toml
version = 1

[[steps]]
operation = "json-select"
pointer = "/data/items"

[[steps]]
operation = "array-to-jsonl"
```

Single-operation form: `zi data json-select --pointer /data/items`. A JSON pointer is a
literal RFC 6901 reference, not a query expression: `/` starts a token, `~1` represents
`/` in a key, `~0` represents `~`. Array indices have no leading zeros. An explicit
empty string selects the root: `--step 'json-select:'` or `--pointer ''` in a shell that
preserves empty arguments. The inline form is useful on older Windows shells. `/` selects
the empty object key, not the root. `json-select:/a:reverse` selects the literal key
`a:reverse`; only a step beginning exactly `json-select:` interprets the remainder as a
pointer, and all its colons belong to that pointer. Quote the whole step when necessary.
It does not evaluate JSONPath, wildcards, filters, URI fragments, variables or shell text.

`pointer` is required only for `json-select`; even `pointer = ""` on another operation is
rejected. `reverse = true` remains sort-only. JSON tool `pointer: null` is rejected.
Existing version-1 recipes without pointers remain valid. Older zi versions cannot run
the new operation; use v0.16.0 or later. No format-version migration is required.

## JSON values versus display text

Each step consumes the previous step's complete output. `json-select` returns compact JSON,
including quotes and original escapes for selected strings. Selecting `"\u4e2d"` keeps
that JSON token; it does not yield bare `中`. JSON numbers never pass through floating-point
conversion; object key order and repeated keys inside the selected subtree remain.
An ambiguous duplicate key along the reference path is rejected. Unrelated duplicates do
not invalidate selection. The whole source must be valid JSON even when only one field is
selected. This does not make the result canonical JSON or validate application schemas.

Existing CLI `--select` and model `select_input` continue to display decoded strings and
can join array display values. Use them for final text extraction/joining; use `json-select`
inside a compound transformation when the next operation should receive a JSON value.

The current model tool accepts the same step objects:

```json
{"steps":[{"operation":"json-select","pointer":"/data/items"},{"operation":"array-to-jsonl"}]}
```

Single operation: `{"operation":"json-select","pointer":"/data/items"}`. Separate tool
invocations still begin with original input; this does not implicitly chain past results,
load local recipe files, or grant project/file/shell access. The Agent loop is unchanged.

## Validation, limits and output

Pointer syntax, 4096 UTF-8 byte/128 reference-token caps and step option validity are
checked for the entire plan before input/output access. `--show-plan` never reads data:
it proves definition validity, not that a path exists or contains an array. Missing paths,
ambiguous referenced keys, invalid array indices, full-source syntax or later type failures
are runtime errors, identify the failing step and emit no intermediate stdout/result file.
Existing `-o` no-clobber and explicit `--force` rules apply, including in-place processing;
the shell's own `>` truncation remains outside this guarantee.

Existing limits remain: 64 KiB recipe/tool arguments, 1–16 steps, 8 MiB original data,
16 MiB per intermediate/CLI final result, 128 JSON containers and 64 KiB tool final JSON
envelope (including escaping). Selection from an expanded intermediate may use the full
16 MiB allowance. Pointer validation does not turn these bounds into a process memory cap.
No new dependencies, tool names, permissions, runtime, network calls or memory files.

Since v0.16.1, the normalized TOML definition must also fit 64 KiB, checked before input/output for both
execution and preview. Pointer escaping can expand a short literal source; such plans fail
early rather than exporting a recipe that the same program cannot read back.
