# JSONL analysis by an explicit string key / 按明确字段分析日志

Development on `main`, not included in the currently published v0.29.1 packages.
These three operations form one release batch; they share the existing native
CLI, version-1 recipe and `transform_input` paths. No new dependency, model tool
name, project permission, background process or task history is added.

## One log, three questions

Consider repeated events arriving out of order:

```jsonl
{"id":"b","level":"WARN","time":"2026-10-29T01:00:02Z","n":1.2300E+400}
{"id":"a","level":"ERROR","time":"2026-10-29T01:00:01Z","n":-0}
{"id":"\u0062","level":"\u0057ARN","time":"2026-10-29T01:00:02Z","n":2}
{"id":"c","level":"ERROR","time":"2026-10-29T01:00:01Z","n":9007199254740993}
```

Whole-line deduplication keeps all four records because their payloads differ.
Whole-line sorting compares the whole serialized text, not the `time` field.
Extracting only a field loses the event's payload. These operations retain complete
records where required and count decoded field values directly:

| Question | Command | Result |
| --- | --- | --- |
| Keep the first event per ID | `zi data jsonl-dedupe --pointer /id` | Complete records b, a, c, in that order |
| Order events by a normalized timestamp string | `zi data jsonl-sort --pointer /time` | Records a, c, b, b; ties keep input order |
| Count records by severity | `zi data jsonl-count-by --pointer /level` | WARN: 2, ERROR: 2, first-seen severity order |

```sh
zi data jsonl-dedupe --pointer /id -i events.jsonl -o unique.jsonl
zi data jsonl-sort --pointer /time --reverse -i events.jsonl
cat events.jsonl | zi data jsonl-count-by --pointer /level | zi data jsonl-to-csv
```

The frequency report is JSONL, with a newly encoded decoded string and exact integer
count in each record:

```jsonl
{"value":"WARN","count":2}
{"value":"ERROR","count":2}
```

Counting after deduplication answers a different question: how many unique IDs
first appeared at each severity, rather than how many raw records were received.
The first record supplies the retained payload; this does not merge later fields
or prove repeated IDs describe the same real event.

## Reuse a reviewed analysis

```sh
zi data --show-plan --step jsonl-dedupe:/id --step jsonl-count-by:/level --step jsonl-to-csv -o severity.toml
zi data --recipe severity.toml -i events.jsonl -o severity.csv
```

For descending field sorting, use the single operation's `--reverse` or set it on
that recipe step:

```toml
version = 1

[[steps]]
operation = "jsonl-dedupe"
pointer = "/id"

[[steps]]
operation = "jsonl-sort"
pointer = "/time"
reverse = true
```

The `--step jsonl-sort:/time:reverse` shorthand selects the literal `/time:reverse`
field in ascending order. A suffix is part of the pointer, never an implied option.
All three shorthand forms require the colon; an empty suffix selects the root.
Existing arrays or CSV can be converted to JSONL in earlier steps. A later projection,
selection, line window or CSV export can reduce/reshape the result.

The model may choose the same operations inside `transform_input`, with an explicit
`pointer`. Separate tool calls restart from original task input; use `steps` to compose
operations on an intermediate. The existing `--exact` path returns one native result
without a model rewrite. `--exact --export-plan` exports its definition without running
the transformation; review and run it locally. Actual model tasks need configuration;
offline operations/preview do not. No arbitrary model-supplied scripts are executed.

## Exact key and record semantics

- Every selected key must be a JSON string. Empty strings are valid, as is an explicit
  empty pointer selecting a string root. Numbers, booleans, null, arrays and objects fail;
  there is no conversion to text or missing-key bucket. Use one explicit RFC6901 pointer,
  including `~1`/`~0` escapes and strict array indices without leading zeros.
- Comparison uses decoded strings, case sensitively. `"b"` and `"\u0062"` share a key;
  `"B"` is distinct. Composed/decomposed Unicode strings remain distinct; no normalization
  or locale/natural sorting is applied.
- Sorting compares Unicode strings lexically: `"10"` sorts before `"2"`. It does not parse
  timestamps; dates/times require a common normalized format, precision and timezone if
  lexical order is intended to mean chronological order. Mixed offsets are not repaired.
  Equal keys keep their input order, including descending sorting.
- Deduplication keeps the first complete record in original order. Sorting keeps all
  complete records. Both compact JSON whitespace but preserve number/string tokens,
  internal member order and unrelated duplicate members. They do not round large numbers
  or rewrite exponent/escape spellings. Output records end with LF.
- Counting groups by decoded key, in first-seen group order, not by highest frequency or
  alphabetical order. Its `value` string is encoded anew; escape spellings need not match
  the source. `count` is an integer record count. A repeated ID counts repeatedly unless
  you explicitly deduplicate first. There is no sum, numeric sort or multidimensional
  grouping hidden behind the command.

Every record is validated, even one later removed as a duplicate. Blank records, invalid
JSON/strings, missing keys, ambiguous referenced duplicate object keys or nonstring selected
values fail the complete operation with a 1-based record position. Diagnostics do not echo
the input, selected key or pointer. Empty input succeeds with zero bytes; a blank record is
an error. CRLF input is accepted and normal leading UTF-8 BOM handling is unchanged.

## Limits and output

Use the existing limits: 1–16 steps; a 64KiB recipe/normalized plan; 8MiB original input,
16MiB per intermediate/final CLI result; 100000 records including duplicates; JSON depth128;
pointer4096 UTF-8 bytes/128 reference tokens. A model tool returns at most64KiB including
the JSON envelope; a large native result may require local `zi data` or a later reducing step.

These are bounded, buffered operations, not streaming/external sorting. Decoded keys and
record references are held while processing; counting/sorting is not constant-memory.
Original payloads are borrowed until formatting, never converted through floating-point
JSON values. No intermediate output is published on a later failure. Existing no-clobber
file publication and explicit `--force` remain. This does not roll back a shell's own `>`
truncation or unrelated processes.

这批改动聚焦一个完整的日志分析流程：保留首条事件、稳定整理顺序、统计字段分布。
它不引入会话或长期记忆，也不会读取未明确提供的日志、隐式发现项目配置或扩张为通用自动化。
