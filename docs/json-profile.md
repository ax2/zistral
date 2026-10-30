# Structural JSON profiles / JSON 结构概览（v0.32.0）

Before choosing fields to mask or extract, inspect the structure without sending raw
values to a model. Existing `json-keys` lists only root keys; these operations summarize
nested object paths and how their types/presence vary across explicit input records.

```sh
zi data json-profile -i response.json
zi data jsonl-profile -i events.jsonl -o structure.json
zi data --step json-select:/events --step array-to-jsonl --step jsonl-profile -i response.json
```

Both run offline without configuration, a key, filesystem/model tools or network access.
`json-profile` accepts one JSON document, including pretty JSON or a scalar root;
`jsonl-profile` accepts one complete JSON value per line. All six JSON types are valid
roots. Root arrays are not automatically expanded into records: explicitly select an
array and use `array-to-jsonl` when that is your intended record set.

For example:

```json
{"user":{"email":"synthetic@example.test"},"token":"synthetic-secret"}
{"user":null}
```

`jsonl-profile` produces this compact JSON report (formatted here for readability):

```json
{
  "records": 2,
  "paths": [
    {"pointer":"","present":2,"missing":0,"types":{"object":2}},
    {"pointer":"/token","present":1,"missing":1,"types":{"string":1}},
    {"pointer":"/user","present":2,"missing":0,"types":{"null":1,"object":1}},
    {"pointer":"/user/email","present":1,"missing":1,"types":{"string":1}}
  ]
}
```

The report contains no sample values, extrema, string lengths or value-frequency
tables. It exposes **key names as paths**, their types and aggregate counts, so it is
not automatic sanitization or anonymization. Keys themselves can contain sensitive
information; review the report before sharing it. A profile produced by a model tool
acts on input already sent to that model. Run the native command offline first when
you intend to share only structure.

## Counting rules

- Empty pointer `""` identifies the root. Recursively visit object members, including
  empty objects. Arrays are opaque leaves: their type is counted, but their elements,
  internal keys and indexes produce no paths. There are no wildcard paths.
- Paths are RFC 6901 pointers, with `~` encoded as `~0` and `/` as `~1`; an empty key
  adds an empty reference token. Keys are decoded exactly without case folding or
  Unicode normalization. Equivalent JSON escapes identify the same key.
- `paths` is sorted by encoded pointer text, not by discovery order or count.
  `types` contains only observed types: `array`, `boolean`, `null`, `number`, `object`,
  `string`. JSON integers, decimals and exponents all count as `number`; values are
  never converted to floating point, inferred as dates, or coerced from strings.
- `present` counts records where that path was visited, at most once per record.
  Type counts sum to `present`. `missing = records - present` includes records with
  an absent, null, scalar or array ancestor. An explicitly null value is present with
  type `null`; it is distinct from a missing path.
- Empty JSONL input reports `{"records":0,"paths":[]}`. Blank records are errors.
  A single JSON document always counts as one record, including an empty object/array.

## Reviewable reusable definitions

```sh
zi data --show-plan --step json-select:/events --step array-to-jsonl --step jsonl-profile -o inspect.toml
zi data --recipe inspect.toml -i response.json -o structure.json
```

Profiles have no operation-specific arguments. Their single-step form is simply
`operation = "json-profile"` or `operation = "jsonl-profile"` in a version-1 recipe.
Pointer selection, projection, masking and array expansion remain separate explicit
steps. The report is a JSON object and can feed `json-select`, formatting or other
existing steps. `--show-plan` checks definitions without reading input or running them.

`transform_input` supports the same operations/steps without adding a tool name,
project permission, dependency, memory store or model-loop branch. Its final serialized
JSON envelope is still limited to 64 KiB; a profile that fits the CLI may exceed this
tool limit. No report is silently truncated or sampled to fit.

## Limits and errors

The entire original input is validated for syntax, depth and Unicode strings, even
inside opaque arrays. Visited objects must have unique decoded keys, including at
ancestor levels; duplicates inside opaque arrays are not structurally inspected.
Malformed data is never skipped, and profiling does not repair or validate a schema.

Limits: 100,000 JSONL records, 1,024 distinct paths **including root** across the whole
input, each encoded pointer at most 4,096 UTF-8 bytes / 128 reference tokens; JSON depth
128. Existing 8 MiB initial data, 16 MiB intermediate/final CLI result and 16-step limits
apply. Paths are registered before queueing so pending traversal stays bounded.

Any invalid record or exceeded limit fails the complete operation with no stdout
report or intermediate data. Diagnostics identify the operation/record without echoing
keys or sample values. File results use normal no-overwrite/`--force` publication;
failure preserves an existing result and source files remain unchanged.

## Prepare data for a model

For a separate two-record `sharing.jsonl` example where `/token` exists in both records:

```json
{"token":"demo-token","level":"ERROR"}
{"token":null,"level":"WARN"}
```

```sh
zi data jsonl-profile -i sharing.jsonl -o structure.json
zi data jsonl-redact --pointer /token -i sharing.jsonl -o sanitized.jsonl
zi --preview --no-stream -i sanitized.jsonl -s builtin:triage -o request.json
# After successful preparation and explicit review:
zi --no-stream -i sanitized.jsonl -s builtin:triage
```

The first two commands need no model configuration. Preview needs a selected model
(from configuration or `--model`) and does not send a request; only the final command
executes the model task. Inspect the report and sanitized data before proceeding.

先查看结构报告，确定要处理的明确路径，再运行离线遮盖并审阅结果，最后用 `--preview`
确认首轮模型请求。结构概览不会自动执行遮盖或推断“敏感字段”；某路径的 `missing` 非零
时，应审阅这些记录，明确是否选用遮盖操作的 `--if-present` / 每步 `if_present = true`；
默认遮盖仍严格报错。概览中的 missing 指未访问到对象路径，数组内部不展开；它不是通用的
指针存在性测试。遮盖仍逐条独立解析指针，非法数组索引或歧义键不会因可选模式而被忽略。
只统计类型也不证明后续数据符合某份 Schema，或其他字段可以安全分享。三项能力各自明确
调用，组合成发送前准备流程。

结构概览自 v0.32.0 起提供，与离线值遮盖和请求预览一起交付。
