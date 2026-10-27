# Ordered native transformations inside one model tool

```sh
cat lines.txt | zi 'Trim each line, remove duplicate lines, then sort descending. Return only the result.'
cat events.jsonl | zi 'Collect the JSON records into an array and pretty-print it.'
```

The model may call the existing `transform_input` tool with ordered inline steps:

```json
{"steps":[{"operation":"trim"},{"operation":"dedupe"},{"operation":"sort","reverse":true}]}
```

This is a model tool argument, not a new CLI command or configuration file. The model still
chooses whether to use a tool and how to answer; when the workflow is known in advance,
use the deterministic offline equivalent without a model or API key:

```sh
cat lines.txt | zi data --step trim --step dedupe --step sort:reverse
zi data --recipe cleanup.toml -i lines.txt -o result.txt
```

Within one call, each step consumes the preceding result. Ordering matters: trim then
dedupe merges `a` and `a `; dedupe then trim leaves two `a` lines. Each separate call
starts again from the original supplied input, even after successful or failed prior calls.
There is no output reference, cross-call memory, hidden recipe file or additional model
request inside a step. Three steps count as one tool invocation, not three model turns.

The previous form remains compatible:

```json
{"operation":"sort","reverse":true}
```

Exactly one of `operation` or `steps` is required. A pipeline has 1–16 objects, each with
`operation` and operation-specific `reverse`, `pointer` or `fields`. Top-level options are for single-operation
calls only. Unknown fields, null values, duplicated fields, mixed shapes and unsupported
operations fail. Only sort accepts `reverse: true`; explicit false remains valid on any
individual step. The complete plan is validated before transforming input.

Allowed operations are trim, dedupe, sort, json-format, json-minify, json-keys,
jsonl-to-array, array-to-jsonl, csv-to-jsonl, jsonl-to-csv, jsonl-project, json-select and jsonl-select. The last two require an explicit
string `pointer`; jsonl-select applies it to each record. No query/filter expressions, shell commands,
file names, includes, variables or user-loaded code. JSON formatting/conversion retains
numeric/string tokens, object order and repeated keys. Line operations use LF and their
existing whitespace/order semantics; this is not arithmetic, numeric sorting or schema validation.

Only `{"output":"..."}` for the final success returns to the model. Intermediate text stays
inside the process, retaining only current/next buffers. Step failures include their position
and return the existing error envelope without intermediate data; invalid-plan errors do not
echo argument values. Models can correct a failed plan and call again. A tool error alone
does not abort the entire Agent task: the model may still answer or recover. Final answer
accuracy remains the model's responsibility; existing `--json`, `-o`, turn and retry rules apply.

Limits: 64 KiB UTF-8 tool arguments, 8 MiB original input, 16 MiB each intermediate,
100000 lines/root keys/JSONL records, 128 JSON containers. Final raw text and its serialized
JSON result envelope must each fit 64 KiB; quotes, backslashes and LF expand when encoded.
A large intermediate may be reduced by later steps; the final step uses the smaller output
budget. Over-limit results fail instead of being truncated. `--max-input` above 8 MiB does
not raise this tool's cap. Each step is complete-buffered, not streaming.

No new tool names or project permissions. CSV import adds csv/csv-core packages inside the executable. Default data mode still
has only count_input, select_input and transform_input. `--no-tools` disables them; project
read-only/no-exec controls are unchanged. The loop still owns model requests and observations;
the shared native pipeline core owns deterministic computation.

Since v0.20.0, `csv-to-jsonl` imports one explicit header-based comma CSV dialect into
JSONL string-valued objects. A quote/record preflight precedes csv-reader decoding; unique
headers and exact row widths are required. Shared pipelines/tools retain original-input
and final-success-only output rules. It adds csv/csv-core packages, not tool names or rights.
See [CSV import](csv-import.md).

Since v0.21.0, `jsonl-to-csv` exports nonempty scalar-valued objects with first-record
column order and exact subsequent column sets. Numeric tokens become exact text, strings
decode, every CSV field is quoted. Nested values and ambiguous columns fail; CSV has no
JSON type metadata or spreadsheet formula sanitization. The shared bounded pipeline and
original-input tools gain no new names, dependencies, permissions or memory.
See [CSV export](csv-export.md).

Since v0.22.0, `jsonl-project` selects/renames 1-64 explicit JSON pointers into each new
ordered record object. Literal unique names and typed field definitions are shared by the
single CLI command, version-1 recipe/plan and existing transform_input. Selected JSON tokens
and subtree order remain exact, missing/ambiguous references fail, and the added object
counts toward depth128. Projection can feed CSV without expressions or hidden state.
No new dependencies, tool names, permissions, Agent loop or memory.
See [JSONL projection](jsonl-projection.md).
