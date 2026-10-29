# Structured JSONL string filtering / 按字段字符串筛选记录

```sh
zi data jsonl-where --pointer /level --equals ERROR -i events.jsonl
zi data jsonl-where --pointer /payload/status --equals failed -i events.jsonl
cat events.jsonl | zi data jsonl-where --pointer /level --equals ERROR
zi data jsonl-where --pointer /level --equals ERROR --show-plan -o errors.toml
zi data --recipe errors.toml -i another.jsonl
```

`jsonl-where` keeps **complete records** when the selected JSON value is a string whose
decoded contents exactly equal the explicit `equals` literal. `"ERROR"` and
`"\u0045RROR"` both match `--equals ERROR`; text elsewhere in the record does not.
Output records are compact JSON, retain original number/string tokens and key order,
including unrelated duplicate members, and each end with LF. Matching never converts
large numbers or rewrites the selected string's escape spelling.

The explicit plain RFC 6901 pointer is required, even if empty to select the root.
Existing pointer limits apply: 4096 UTF-8 bytes and 128 reference tokens, `~1` for `/`,
`~0` for `~`, canonical array indices without leading zeros. An existing JSON null,
boolean, number, array or object does **not match**, even when its displayed text equals
the literal. This is a string-only equality operation, not a general predicate engine.

`equals` is required and may be empty, up to **4096 UTF-8 bytes**. It is literal text,
not a JSON literal: `--equals ERROR` matches the JSON string `"ERROR"`; including quotes
in the argument would match a string containing those quotes. Comparison is case-sensitive
and does not trim, normalize Unicode, infer numbers, interpret regex or match substrings.
CR/LF/tab/NUL inside a decoded JSON string retain their normal literal meaning; a record
must still fit on one physical JSONL line with JSON escapes for such characters.

For values starting with `-`, use `--equals=-value` or a recipe. For an empty literal
use your shell's empty-argument syntax, or `equals = ""` in a recipe. `--pointer ""`
selects a whole JSONL value and can match root string records.

## Failure is complete, including discarded records

Missing pointers, ambiguous referenced duplicate keys, invalid pointer traversal,
malformed/blank records or selected strings that cannot be decoded fail the entire
operation. They are not silently treated as nonmatches. The error identifies the
1-based record position, not raw input, pointer or match literals. Every source record
is checked, including records that would not match. An invalid later record never
publishes a prefix or replaces an existing output file, even with explicit `--force`.

Empty input and a fully valid input with no matches succeed with **zero bytes**.
Record order and duplicates remain unless another explicit step changes them.
CRLF becomes LF; JSON whitespace outside strings is compacted. A single initial UTF-8
BOM follows the existing input-reader rule. An internal BOM is not silently removed.

The existing limits remain: 8 MiB initial input, 16 MiB intermediate/final CLI output,
100000 source records per JSONL operation, nesting128 and 1–16 pipeline steps. Discarded
records count toward input/record limits. `transform_input` additionally has 64 KiB
arguments and a 64 KiB final JSON envelope; a large filtered result can therefore fail
in a model task while succeeding locally. No partial model-tool output is returned.

## Reusable recipes and native model tasks

```toml
version = 1

[[steps]]
operation = "jsonl-where"
pointer = "/level"
equals = "ERROR"

[[steps]]
operation = "lines-tail"
lines = 20
```

Select the recipe explicitly with `zi data --recipe errors.toml`. An array response can
first use `json-select` then `array-to-jsonl`; matching records can then feed
`jsonl-project`, `jsonl-select`, `jsonl-to-array` or `jsonl-to-csv`. Unlike raw line
filtering, a field match does not mistake an unrelated message for a level.
For multi-step workflows use recipes, or let a model propose one with
`--exact --export-plan`. There is no multi-parameter `--step jsonl-where:...` shorthand;
single-operation `--pointer` and `--equals` cannot be mixed with `--step` or `--recipe`.

The existing `transform_input` accepts the same definition:

```json
{"steps":[{"operation":"jsonl-where","pointer":"/level","equals":"ERROR"},{"operation":"lines-tail","lines":20}]}
```

Ordinary tasks retain their normal final-model behavior. `--exact` returns one successful
native result directly. `--exact --export-plan` validates and exports TOML without
executing records, as documented in [model plan export](model-plan-export.md).
Every separate tool call still starts from the original task input. Definition errors
may be corrected within the existing budget. `equals` is invalid on other operations,
even empty or null; other operation-specific options retain their restrictions.

Offline `--show-plan` validates without reading data or loading model configuration.
Definition validity does not prove the selected pointer exists in actual records.
The version-1 recipe and argument-size caps are unchanged. There are no expressions,
shell commands, includes, hidden file reads, new tool names, project permissions,
dependencies, Agent-loop branches or task memory.

Filtering inside a model task cannot retract original input already sent to that
provider. To reduce supplied model context, filter offline first and pipe the result
explicitly. This operation is not automatic redaction; retained records can still
contain sensitive fields. A model may choose the wrong pointer/literal, so inspect a
proposed plan before reuse or sharing.
