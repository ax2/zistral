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

These are bounded, buffered operations, not streaming/external sorting. Counting and
deduplication retain distinct decoded keys rather than every repeated key. Counting stores
each key once with its count and first position, then restores first-seen order. Sorting
still holds all decoded keys and record references. Input and final result are buffered;
no operation is constant-memory.
Original payloads are borrowed until formatting, never converted through floating-point
JSON values. No intermediate output is published on a later failure. Existing no-clobber
file publication and explicit `--force` remain. This does not roll back a shell's own `>`
truncation or unrelated processes.

这批改动聚焦一个完整的日志分析流程：保留首条事件、稳定整理顺序、统计字段分布。
它不引入会话或长期记忆，也不会读取未明确提供的日志、隐式发现项目配置或扩张为通用自动化。

## Ready-to-review examples / 可直接审阅和复用的场景

Three explicit recipes are included in the source tree, without automatic installation
or implicit discovery:

| Recipe | Intended question |
| --- | --- |
| [severity-counts.toml](examples/severity-counts.toml) | How many raw records per severity? |
| [unique-severity-counts.toml](examples/unique-severity-counts.toml) | How many first-seen event IDs per severity? |
| [recent-unique-events.toml](examples/recent-unique-events.toml) | Show up to20 first-seen events, newest normalized string timestamp first |

```sh
zi data --show-plan --recipe docs/examples/unique-severity-counts.toml
zi data --recipe docs/examples/unique-severity-counts.toml -i events.jsonl -o counts.csv
zi data --recipe docs/examples/recent-unique-events.toml -i events.jsonl
```

Run these paths from the source checkout, or copy the reviewed recipe to your own explicit
location. They are definitions only: no input paths, saved results, model settings, memory,
shell commands or execution grants. Read the comments, choose matching field names, and
keep the exact string/normalized timestamp requirements. The two count reports intentionally
answer different questions when IDs repeat.

Pipeline errors identify the step number and operation, such as
`step 2 failed (jsonl-count-by): cannot read JSONL key at record 2 (...)`.
Definition errors are still checked before input/output; later failures publish no partial
result or replacement file. Input/pointer contents are not included in these diagnostics.

## Local process memory measurement

On Windows x64,100000 records (7300000 input bytes),3 runs per operation and key
distribution, before/after medians from comparable local release builds:

| Operation | Distinct keys | Peak working set MiB, before → after | Peak committed MiB, before → after |
| --- | ---: | ---: | ---: |
| jsonl-count-by | 1 | 24.47 → 21.60 | 21.40 → 17.08 |
| jsonl-dedupe | 1 | 24.45 → 21.57 | 21.34 → 17.07 |
| jsonl-sort | 1 | 28.41 → 28.38 | 29.84 → 29.85 |
| jsonl-count-by | 100000 | 39.77 → 29.54 | 42.05 → 29.63 |
| jsonl-dedupe | 100000 | 34.54 → 30.77 | 35.30 → 28.20 |
| jsonl-sort | 100000 | 28.38 → 28.43 | 29.87 → 29.86 |

Every output was checked byte for byte; sorting is the unchanged control, with small
measurement variation. These are whole-process OS peaks, including startup/input/result
buffers, not allocator-only figures or guarantees for all machines/data. The two columns
are separate Windows measurements, not interchangeable estimates. Definitions follow
[Microsoft process memory counters](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters).
The exact byte values, trials, binary/source hashes and profile are in
[measurement data](../benchmarks/jsonl-memory-results.json).

To reproduce on Windows with Python (development only), build two comparable release
binaries in separate checkouts, then measure each binary with both key distributions:

```sh
python scripts/benchmark_jsonl_memory.py path/to/before/zi.exe --keys repeated --output before-repeated.json
python scripts/benchmark_jsonl_memory.py path/to/after/zi.exe --keys repeated --output after-repeated.json
python scripts/benchmark_jsonl_memory.py path/to/before/zi.exe --keys unique --output before-unique.json
python scripts/benchmark_jsonl_memory.py path/to/after/zi.exe --keys unique --output after-unique.json
```

The optional script uses only Python's standard library and Windows process counters,
validates exact outputs, rejects existing result paths, and cleans its checked temporary
input directory. It does not build/download software or contact a model. The CLI remains
cross-platform with no Python requirement. A separate seeded differential run compared133
valid/invalid CLI samples; output bytes and exit status matched the previous development
binary, while diagnostics may now include the operation name.
