# Select and rename several fields from every JSONL record

Use v0.22.0 or later to define an ordered object of explicit JSON pointers, without a model,
an external script, expressions or inferred values:

```sh
zi data jsonl-project --field id=/payload/id --field amount=/payload/n -i events.jsonl
cat events.jsonl | zi data jsonl-project --field id=/payload/id --field amount=/payload/n
zi data --recipe docs/examples/project-csv.toml -i events.jsonl -o table.csv
```

Input:

```jsonl
{"payload":{"id":"0007","n":123456789012345678901234567890},"unused":"not exported"}
{"payload":{"n":1.2300E+400,"id":"0008"}}
```

The single operation returns:

```jsonl
{"id":"0007","amount":123456789012345678901234567890}
{"id":"0008","amount":1.2300E+400}
```

Fields appear in definition order, independently of source key order. Values retain JSON
types and original numeric/string tokens, including huge numbers, exponent spelling, negative
zero and string escapes. Selected containers become compact JSON while retaining their
internal key order and repeated keys. This operation is not bare text extraction, a schema
validator or JSON canonicalization. Existing `jsonl-select` still selects just one value.

## Literal mappings, not expressions

- Repeat `--field NAME=POINTER` with a single `jsonl-project` operation. The **first** equals
  sign separates name and pointer; later equals signs belong to the pointer. `--field copy=`
  selects the entire record, including a primitive. Output names containing `=` can be defined
  in a recipe/tool object instead of this shorthand.
- Define 1–64 fields. Names must be unique, nonempty, at most 256 UTF-8 bytes and contain no
  control characters. Names are literal keys: dots, slashes or template-like text do not create
  nested output or execute anything. No trimming, case folding or Unicode normalization.
- Every field requires an explicit plain RFC 6901 pointer, even empty for the root. Existing
  4096 UTF-8 byte / 128 reference-token limits, `~0`/`~1` escapes and array-index rules apply.
  No JSONPath, filters, wildcards, interpolation or calculated fields. Mappings remain strict
  unless an explicit default is supplied (since v0.33.0; see below).
- All fields select from the **same source record**, not a previous output alias. A later
  pipeline step can consume the newly projected objects; fields within a step cannot chain.
- Missing paths without an explicit default, ambiguous referenced duplicate keys, invalid indices or malformed JSON fail
  the entire invocation. An existing `null` is a selected value; a missing field is an error.
  Every complete record is syntax-checked, including unselected data. Unreferenced duplicate
  keys are not globally rejected; the inherited selector rejects ambiguity on referenced paths.

Output has one LF-terminated object per input record. LF/CRLF-delimited input is supported;
blank records fail, while empty input produces zero bytes after definition validation.
The existing CLI removes one initial UTF-8 BOM. A remaining BOM on a later record fails.

## Reusable plans and CSV

`--field` belongs to a single operation and cannot be mixed with `--step` or `--recipe`.
Use a version-1 recipe for a compound projection; there is no new shorthand grammar hidden
inside `--step`. The example recipe projects nested scalar fields, then exports CSV:

```toml
version = 1

[[steps]]
operation = "jsonl-project"

[[steps.fields]]
name = "id"
pointer = "/payload/id"

[[steps.fields]]
name = "amount"
pointer = "/payload/n"

[[steps]]
operation = "jsonl-to-csv"
```

```sh
zi data jsonl-project --field id=/payload/id --field amount=/payload/n --show-plan -o project.toml
zi data --recipe project.toml -i events.jsonl
zi data --show-plan --recipe docs/examples/project-csv.toml
zi data --step json-select:/data --step array-to-jsonl -i response.json | zi data --recipe docs/examples/project-csv.toml
```

The last example assumes an API envelope with a `/data` array of the same nested objects.
Exported plans store only definitions, not input data, file paths, model configuration or
permissions. Normalized plans must still fit 64 KiB and be readable by the same version.
Older zi versions reject the new operation/fields. `--show-plan` does not read data and cannot
prove that paths exist. A top-level pointer and reverse=true remain invalid for projection; fields, even empty,
are invalid on other operations. Explicit null is not an omitted field array.

Projection can retain nested objects/arrays, but [CSV export](csv-export.md) accepts scalar
cells only. Choose scalar paths for that downstream operation. CSV has no JSON type metadata;
reimport produces strings, and quoting does not control spreadsheet inference/formulas.
Empty projected input also cannot provide CSV with an inferred header.

## Tool integration, failures and bounds

The existing `transform_input` tool accepts the same typed definitions, alone or in `steps`:

```json
{"steps":[{"operation":"jsonl-project","fields":[{"name":"id","pointer":"/payload/id"},{"name":"amount","pointer":"/payload/n"}]},{"operation":"jsonl-to-csv"}]}
```

Each new invocation starts from original supplied input; steps within it use the preceding
intermediate. The tool does not load recipe files. Errors return normal observations for
correction, identifying the 1-based record and field position without echoing aliases,
pointers or record values. Definition errors identify the field/constraint without values.

All results stay buffered until complete success. Later failure publishes no partial stdout
and does not replace an existing `-o` file, including with `--force`. Internal buffers may
contain a valid prefix; this is complete-result delivery, not task rollback. Existing shell
`>` truncation, concurrent-writer and metadata limitations still apply.

Common limits remain: 8 MiB initial input, 16 MiB intermediate/CLI output, 100000 records,
128 JSON containers, 1–16 steps, 64 KiB recipe/tool arguments and 64 KiB final tool JSON
including escaping. A projection adds a new object container: selecting a whole 128-deep
value cannot be wrapped without exceeding the cap. Repeating large selections can expand
output; oversize results fail instead of truncating. Selection traversals are bounded by
the 64-field limit; this is buffered processing, not an unlimited stream.

No dependencies, new tool names, network requests in offline commands, project permissions,
Agent-loop changes or task memory are introduced. Pointer semantics reuse the existing
selector based on [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901).


## Explicit defaults for absent fields / 显式缺失值（v0.33.0）

Since v0.33.0, per-field defaults use `--field-default` / `default_json`.
Older releases retain strict mappings and reject these additions. Defaults are
user-selected JSON literal text, rather than inferred values or captured task history:

```sh
zi data jsonl-project --field id=/id --field tag=/tag --field-default 'tag=""' -i events.jsonl
zi data jsonl-project --field id=/id --field tag=/tag --field-default tag=null --show-plan -o optional.toml
zi data --recipe optional.toml -i events.jsonl
```

Repeat `--field-default NAME=JSON_LITERAL` for different explicitly mapped output
names. Separate arguments must refer to unique names, with at most one default per
field; unknown names, duplicate defaults and default-only invocations fail. The
first equals sign separates the output name and literal. Existing names containing
`=` can still be defined in TOML/tool objects. For portable compound definitions:

```toml
version = 1

[[steps]]
operation = "jsonl-project"
fields = [
  { name = "id", pointer = "/id" },
  { name = "tag", pointer = "/tag", default_json = '""' },
  { name = "n", pointer = "/optional/n", default_json = "123456789012345678901234567890" },
]

[[steps]]
operation = "jsonl-to-tsv"
```

The `default_json` attribute is a **string containing JSON**, both in TOML and in
model-tool arguments. `default_json = "null"` supplies a JSON null; a raw boolean,
number, object or JSON null attribute is rejected. Literal text must be valid JSON
with valid Unicode, 1–4096 UTF-8 bytes, and fit the nesting cap after adding the
projected object. Known-invalid definitions fail before reading data or preparing
output. Export preserves this explicit literal and ordinary version-1 step layout;
older binaries reject the new field instead of silently ignoring it.

Use the default only when the path is absent, including a missing/noncontainer
ancestor or an out-of-range canonical array index. Present null stays null; false,
zero and empty string also remain their original selected values. An invalid or
unsupported array index and an ambiguous referenced key are errors, including when
another ancestor/record could use a default. If no default is declared for a field,
it keeps the original strict lookup. All fields still select from the same source
record; defaults do not create values for later mappings in that step.

Original JSON syntax and Unicode strings are checked before using a default, even
in unselected data. A fallback cannot hide an unpaired surrogate or malformed
record. Default number/string tokens, key order and duplicate members inside an
explicit container literal survive compact formatting. Text such as `{{input}}`,
shell syntax or a path is literal JSON string content, never interpolation or IO.

The optional default supports any JSON value. Existing CSV/TSV export still requires
scalar columns, so a container default fails a later export step rather than being
flattened or stringified automatically. Scalar null exports as the text `null`;
choose the explicit JSON empty-string literal if an empty table cell is wanted.
Byte/record/depth, 64-field and serialized tool-envelope caps remain unchanged.
Late failures publish no stdout intermediate and do not replace an existing output.
