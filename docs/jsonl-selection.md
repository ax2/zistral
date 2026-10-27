# Select a field from every JSONL record

Use v0.18.0 or later to extract one value from each structured log/event record without
a model, shell script or numeric conversion:

```sh
zi data jsonl-select --pointer /payload/id -i events.jsonl
cat events.jsonl | zi data --step jsonl-select:/payload/id --step dedupe --step jsonl-to-array
zi data --show-plan --step jsonl-select:/payload/id --step dedupe --step jsonl-to-array -o ids.toml
zi data --recipe ids.toml -i events.jsonl -o ids.json
```

For this input:

```jsonl
{"payload":{"id":123456789012345678901234567890}}
{"payload":{"id":1.2300E+400}}
{"payload":{"id":123456789012345678901234567890}}
```

The single operation emits those three exact numeric tokens, one per line. The compound
recipe emits `[123456789012345678901234567890,1.2300E+400]` followed by LF. `dedupe` compares
exact lines, not numeric/object equivalence. Record order remains unchanged until a later
operation explicitly changes it.

An API envelope can use the same pipeline:

```sh
zi data --step json-select:/data/items --step array-to-jsonl --step jsonl-select:/id --step jsonl-to-array -i response.json
```

Version-1 recipes store `operation = "jsonl-select"` and `pointer = "/payload/id"` in the
step. They store definitions, not input/output paths or past data. Existing recipes remain
valid; older zi versions reject this new operation. `--show-plan` validates definitions
without reading data, so it cannot prove that every record contains the selected field.

## Values, pointers and errors

Every physical LF/CRLF-delimited line must contain one complete JSON value, including
unselected fields. Missing paths, ambiguous referenced duplicate keys, invalid array indices,
blank lines and malformed records fail the whole operation; there is no implicit skip,
null substitution or best-effort output. Errors identify the one-based record position
and pipeline step without echoing record contents or pointer values. `null` at an existing
path is a valid selected value, unlike a missing path.

The pointer is required, including when selecting each record's root. `--step 'jsonl-select:'`
is a shell-friendly way to express an empty pointer. Use `/` for an empty object key, `~1`
for `/` inside a key and `~0` for `~`. Colons after `jsonl-select:` belong to the pointer:
`jsonl-select:/a:reverse` selects key `a:reverse`. Array indices have no leading zeros.
There are no wildcards, JSONPath expressions or filters. `reverse = true` remains sort-only.
See [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) for pointer syntax.

Each selected value remains compact JSON. Strings keep their quotes and original escapes,
so an escaped newline never becomes a physical record separator. Numeric tokens, object
key order and repeated keys inside a selected subtree remain as supplied. This is not
canonicalization, schema validation or decoding to bare display text; existing `--select`
and `select_input` retain their display/join behavior.

Empty input emits zero bytes. A final LF is accepted but not required; output has one LF
per value. CLI input strips one initial UTF-8 BOM as an existing compatibility tolerance;
a BOM on a later line fails. The [JSON Lines format](https://jsonlines.org/) itself forbids
BOMs and blank records. This command is not a strict raw-file BOM validator.

## Agent integration and limits

The existing `transform_input` tool accepts the same operation, alone or in `steps`:

```json
{"steps":[{"operation":"jsonl-select","pointer":"/payload/id"},{"operation":"dedupe"},{"operation":"jsonl-to-array"}]}
```

Each invocation starts from original supplied input. Within an invocation each step sees
the prior intermediate. Errors return a normal tool observation, allowing the model to
correct the pointer and retry; only the final complete success returns as `{"output":"..."}`.
No local recipe files are loaded by model tools. Main-loop, permissions and task memory
are unchanged; there are still three native input tools in default data mode.

Existing resource limits apply: 8 MiB original input, 16 MiB per intermediate/CLI result,
100000 records, 128 nested JSON containers per record, 4096 UTF-8 bytes/128 tokens per
pointer, 1–16 steps, 64 KiB recipe/tool arguments and 64 KiB final tool JSON envelope
including escaping. Adding a later `jsonl-to-array` also adds one nesting level. This is
bounded, fully buffered processing, not an unbounded line stream or compressed-file reader.

Any failure emits no partial stdout and does not publish `-o` output; `--force` only replaces
the target after the whole pipeline succeeds. Shell `>` truncation, concurrent writer and
file-metadata limits remain as documented for [result saving](../README.md#save-complete-results-without-truncating-a-file-first).

No dependencies, tool names, network calls, project capabilities or persistent history added.
