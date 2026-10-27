# JSON Lines and JSON arrays / JSONL 与数组互转

```sh
cat events.jsonl | zi data jsonl-to-array
zi data array-to-jsonl -i payload.json -o events.jsonl
zi data --step jsonl-to-array --step json-format -i events.jsonl -o events.json
zi data --recipe docs/examples/unique-records.toml -i payload.json -o unique.json
```

`jsonl-to-array` accepts one complete JSON value per physical LF/CRLF-delimited line:
objects, arrays, strings, numbers, booleans and null. It produces a compact root array.
An empty file becomes `[]`; blank/whitespace-only lines, multiline records, extra values
on a line and malformed JSON fail with a one-based record number. The final line need
not end in LF. No bad records are silently skipped and no record contents appear in errors.

`array-to-jsonl` requires a root JSON array, validates the complete input, then writes one
compact JSON value and LF per root item. `[]` produces zero bytes. Nested arrays/objects
remain one record; commas/brackets/escaped newlines inside strings never split records.
Scalar or object roots are rejected. Both conversions retain record/item order, original
number/string tokens, object key order and repeated keys. They are not numeric conversion,
canonical JSON, a duplicate-key check or semantic object deduplication.

两种转换不构建 serde_json::Value 数值树。JSONL 聚合逐行校验后复用词法压缩器；数组拆分先
校验整个 JSON，再跟踪字符串、转义和嵌套层级，只把根层级逗号当作分界。因此超大整数、
`1.2300E+400` 和 `-0` 不被转成浮点数，字符串中的 `\n` 保持转义文本。

Limits: original UTF-8 input 8 MiB (even if `--max-input` is increased), local output or
each pipeline intermediate 16 MiB, 100000 JSONL records/root array items, 128 nested JSON
containers. Collection adds one array layer: a record with 128 containers cannot be wrapped
into another supported array. These commands buffer complete results; the format's line
structure does not imply unbounded/streaming processing or gzip decompression support.

CLI input retains zi's existing normalization: one initial UTF-8 BOM is removed before
operations. A BOM inside a later record is invalid. Output never includes a BOM. This is
a deliberate input compatibility tolerance; the [JSON Lines format](https://jsonlines.org/)
itself prohibits a BOM, requires valid JSON on each line and permits CRLF. Do not use this
converter as a strict raw-file BOM validator.

Both operations work in positional `zi data`, ordered `--step`, and explicit version-1
TOML recipes. `--reverse` is still only for sort. A later failure produces no intermediate
stdout and does not publish an `-o` result; shell `>` can independently truncate its target.
The existing explicit overwrite, concurrency and file-metadata limitations still apply.

The model can choose either operation through the existing `transform_input` tool. Each
call still transforms original input, not a prior tool result. Tool output including its
JSON envelope is capped at 64 KiB and fails rather than truncates. No new tool names,
project permissions, filesystem/shell access, sessions or memory are added.

`docs/examples/unique-records.toml` converts array items to compact lines, keeps the first
exact line, then collects them back into an array. Object key order and string escape forms
are retained, so textually different objects can remain distinct even if an application
would interpret them as equivalent. Use a model to interpret meaning, not to estimate
exact structural conversion results.

Since v0.18.0, `jsonl-select` applies one explicit pointer to every JSONL record, with
complete-result failure and original numeric/string tokens. It is shared by CLI plans
and transform_input, adding no tool name or permission. See [record selection](jsonl-selection.md).
