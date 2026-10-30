# Explicit JSON value redaction / 显式 JSON 值遮盖（main，尚未发布）

Before sharing a structured log, replace the values you explicitly identify without
rebuilding every field. `json-redact` processes one JSON document; `jsonl-redact`
processes every JSONL record. Both are offline and require no configuration or API key.
This is a bounded native data operation, not automatic detection of secrets.

```sh
zi data json-redact --pointer /credentials/token -i response.json
zi data jsonl-redact --pointer /user/email -i events.jsonl -o sanitized.jsonl
zi data --step jsonl-redact:/credentials/token --step jsonl-redact:/user/email -i events.jsonl
```

For example, masking `/token` in:

```json
{"id":"a","token":"synthetic-secret","count":9007199254740993,"ratio":1.2300E+400}
```

produces:

```json
{"id":"a","token":"[REDACTED]","count":9007199254740993,"ratio":1.2300E+400}
```

The selected value becomes the fixed JSON string `"[REDACTED]"`, regardless of
whether it was a string, number, boolean, null, array or object. A container's contents
are replaced together. Keys, order, unrelated values and their numeric/string token
spellings remain; insignificant whitespace is removed. Equal values elsewhere are
not changed. An empty pointer explicitly replaces the whole document or each record.
This changes the selected value's type to string; it does not remove the field.

Pointers follow the existing RFC 6901 rules: `~1` selects a slash in a key, `~0` a
tilde, and array indexes must be in range and have no leading zeros. Keys are compared
after JSON escape decoding, without Unicode normalization. A missing path, ambiguous
duplicate referenced key, invalid JSON/Unicode string, or exceeded limit fails the whole
operation. Invalid source data cannot be concealed by masking its parent or root.
Unrelated duplicate keys remain unchanged; there is no attempt to repair a record.

JSONL keeps record order and uses compact records with LF endings, including after the
last record. Empty input produces zero bytes; blank records are errors. All records
are checked and the existing 100,000-record, depth-128 and input/output limits apply.
No intermediate results reach stdout. A failure keeps an existing file result unchanged,
including with `--force`. Diagnostics identify the operation/record without echoing values.

## Review and reuse

```sh
zi data --show-plan --step jsonl-redact:/credentials/token --step jsonl-redact:/user/email -o share.toml
zi data --recipe share.toml -i events.jsonl -o sanitized.jsonl
```

Or edit an explicit version-1 recipe:

```toml
version = 1
[[steps]]
operation = "jsonl-redact"
pointer = "/credentials/token"
[[steps]]
operation = "jsonl-redact"
pointer = "/user/email"
```

Each step consumes the preceding intermediate. Mask sibling fields in separate steps.
If you mask `/user` first, a later `/user/email` no longer exists and the pipeline fails;
there is no skip-missing mode. Masking the same path twice is valid. Plans contain only
definitions, not input data or permissions, and `--show-plan` does not read stdin.

## Before a model request / 先离线处理，再决定发送

`transform_input` supports the same operations and ordered steps, retaining its existing
64 KiB final JSON-envelope bound. It adds no model tool names, project access, runtime
dependencies, Agent-loop branches or task memory.

However, a model-triggered transform acts on input **already sent to the provider**.
It cannot undo that disclosure. To withhold values from a provider, run `zi data`
offline first, inspect the complete sanitized file, then explicitly pass that file to
a model task. Only proceed after successful redaction; in a shell pipeline, downstream
commands may still run when an earlier command fails.

```sh
zi data --recipe share.toml -i events.jsonl -o sanitized.jsonl
# After success and review:
zi -i sanitized.jsonl -s builtin:triage
```

只遮盖明确选中的值，不扫描其他字段、键名、嵌入文本、其他文件或之前已发送的数据。
没有通配符、正则、自动脱敏、哈希或自定义替换。原始输入文件不会改写；输出仍需按实际
分享要求审阅。该功能属于下一批改动，正式 v0.31.0 安装包尚不包含它。
