# Offline data pipelines / 离线处理流程

## Embedded recipes / 内置配方（v0.31.0）

The development build embeds reviewed native recipe definitions in the executable.
No separate recipe directory or model configuration is needed after installation.

```sh
zi data --list-recipes
zi data --recipe builtin:severity-counts -i events.jsonl -o counts.csv
zi data --show-plan --recipe builtin:recent-unique-events -o recent.toml
# Edit /id, /time or lines in recent.toml, then run your explicit definition:
zi data --recipe recent.toml -i events.jsonl
```

| Reference | Steps and assumptions |
| --- | --- |
| `builtin:cleanup` | Trim each line, keep first identical trimmed line, sort descending. |
| `builtin:severity-counts` | Count raw records by decoded string `/level`, convert counts to CSV. |
| `builtin:unique-severity-counts` | Keep first string `/id`, then count retained string `/level` as CSV. |
| `builtin:top-severity-counts` | Count string `/level`, stably rank integer counts descending, take 20 groups as CSV. |
| `builtin:recent-unique-events` | Keep first string `/id`, stably sort string `/time` descending, take first 20 records as JSONL. |

去重保留首条，不会自动选取同一 ID 的最新记录。时间只做字符串排序，不解析日期；应先统一
格式与时区。原始计数分组按首次出现顺序输出；top-severity-counts 按整数频次倒序排列，同频仍按首次出现顺序。缺失、重复歧义或非字符串字段会使整次处理失败，
不会发布部分输出。需要别的字段或行数时，先导出、审阅并编辑配方，再以文件路径调用。

`--list-recipes` is offline and never reads or waits for stdin. It rejects explicit
`--input` (including `-i -`), operation options, `--step`, `--recipe` and `--show-plan`.
`-o` saves the catalog using normal no-clobber rules. Exporting a builtin with
`--show-plan` follows the same no-input, limits and output contracts as a file recipe;
the result contains only native steps, not captured data, paths or execution grants.

The `builtin:` prefix is reserved only at the start of the supplied reference; names
are exact and case-sensitive, and unknown names fail before opening input or output.
There is no fallback to local files and no implicit project lookup. On Unix, address
a literal filename such as `builtin:cleanup` via `./builtin:cleanup`. Other paths,
including non-UTF-8 names on filesystems that permit them, retain normal explicit-file
behavior. Embedded and
file recipes share the same parser, validation and execution pipeline.

## Compose your own definition / 编写配方

Combine native data operations without a model, shell subprocess or intermediate stdout:

```sh
cat lines.txt | zi data --step trim --step dedupe --step sort:reverse
zi data --recipe cleanup.toml -i lines.txt -o clean.txt
zi data --show-plan --recipe cleanup.toml
zi data --show-plan --step trim --step dedupe --step sort:reverse -o cleanup.toml
```

Save this UTF-8 TOML definition as `cleanup.toml`:

```toml
version = 1

[[steps]]
operation = "trim"

[[steps]]
operation = "dedupe"

[[steps]]
operation = "sort"
reverse = true
```

顺序就是语义。先 trim 再 dedupe，会把 `a` 与 `a ` 合并；反过来会保留两行，随后才去空白。
`dedupe` 本身不做去空白或大小写折叠，sort 是区分大小写的 Unicode 顺序，不是自然/数字排序。

Operations: `trim`, `dedupe`, `sort`, `json-format`, `json-minify`, `json-keys`,
`jsonl-to-array`, `array-to-jsonl`. [JSONL conversion rules](json-lines.md).
`sort`, `jsonl-sort` and `jsonl-sort-int` accept `reverse = true`; `sort:reverse` is the line-sort CLI shorthand. Single-operation
`zi data sort --reverse` remains supported. Do not mix positional operations, `--step`
and `--recipe`; pipeline direction belongs to its individual step, not a global `--reverse`.

The entire recipe and all step options are checked before opening the data input or
preparing its output. Unknown fields and unsupported versions/operations fail. A recipe
does not name input/output paths; use `-i` and `-o` explicitly. No recursive includes,
variables, shell commands, expressions, implicit project discovery or model requests.
Definitions are not sent to models and do not add model tools or project permissions.

Only the final result reaches stdout or `-o`. A later JSON/type/size failure exits nonzero,
reports the failing step number and emits no intermediate result. File output retains the
existing no-overwrite default and explicit `--force`; `-o input.txt --force` can process
in place because input is read before successful publication. This does not roll back shell
redirection, other processes' changes, or unrelated Agent actions.

Limits: 64 KiB recipe, 1–16 steps, 8 MiB original UTF-8 input even if `--max-input` is raised,
16 MiB per intermediate/final result, 100000 lines/root keys, 128 nested JSON containers.
A formatted JSON intermediate may exceed 8 MiB if it stays below 16 MiB, then be minified
by the next step. Each step is complete-buffered, not token/record streaming. Only the current
and next intermediate are retained, not all stages. Normal pipe/newline/BOM rules apply:
initial BOM removed, line operations use LF, final JSON output ends in LF, empty line output
is zero bytes. JSON formatting preserves numeric/string tokens, key order and repeated keys.

配方文件是用户维护的步骤定义，不是会话、记忆或脚本引擎。团队可以将它纳入版本控制、审阅和
复用；没有读取过往任务、推测偏好或保存输入数据的行为。

## Inspect and export a plan before processing data / 先看计划，再处理数据

`--show-plan` accepts the same single operation, ordered `--step` list or explicit
`--recipe`. It validates the whole definition with the normal 1–16 step and 64 KiB
recipe limits, then emits normalized UTF-8 version-1 TOML. Order and reverse sorting
remain; comments, source formatting, a BOM and default `reverse = false` are omitted.
The output is a reusable recipe for `--recipe`, not a stable byte-canonical format or
a record of input data, past answers, paths or execution permissions.

```sh
zi data --show-plan --step trim --step dedupe --step sort:reverse -o cleanup.toml
zi data --show-plan --recipe cleanup.toml
zi data --recipe cleanup.toml -i lines.txt -o cleaned.txt
```

Preview never reads or waits for stdin, runs operations, loads model configuration or
contacts a model. `--input` (including `-i -`) is rejected to avoid silently previewing
an invocation intended to process data. Stdin remains unread even when redirected.
`--output -` prints the plan, while `-o FILE`
saves the plan through the existing no-clobber/explicit `--force` publication path.
Invalid definitions fail before output preparation and do not replace existing targets.
Saving is explicit; preview does not create configuration, cache or task-history files.

Success proves definition validity only. `trim` followed by `json-minify` is a valid
plan even without data, but actual non-JSON input will fail at step 2. JSON type/syntax,
input and intermediate growth, line counts and output races are checked only during
execution/publication. Existing file output and shell-redirection limitations still apply.
No execution simulation, type-flow inference, constant folding, model pipeline import,
new operation, tool or dependency is introduced. This preview does not grant tools or
execute embedded shell text.

`json-select` is the ninth native operation. It requires `pointer` in a TOML step,
`--pointer` in single-operation mode, or `--step json-select:/path`. It returns compact
JSON, including quoted strings, from the current intermediate. An explicit empty pointer
selects the root. See [selection recipes](json-selection-recipes.md) for exact contracts.

Since v0.18.0, `jsonl-select` applies one explicit pointer to every JSONL record, with
complete-result failure and original numeric/string tokens. It is shared by CLI plans
and transform_input, adding no tool name or permission. See [record selection](jsonl-selection.md).

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

Since v0.24.0, `lines-include`/`lines-exclude` filter whole lines by explicit bounded
`contains` literals. CLI, saved plans and transform_input share definition validation and
complete-result behavior; discarded lines still count toward the 100000-line cap. This is
case-sensitive raw-line matching, not regex, structured predicates or implicit redaction.
It adds no dependencies, model tool names, permissions, Agent-loop logic or memory.
See [line filtering](line-filtering.md).

`jsonl-where` requires an explicit `pointer` and string `equals` (empty allowed, max4096
UTF-8 bytes). Matching compares decoded JSON strings; nonstrings do not match. Referenced
missing/ambiguous fields and invalid records fail the complete result, including discarded
records. Retained whole records remain compact JSONL with original tokens. Use a recipe
for composition; no multi-parameter inline `--step` shorthand. [Details](jsonl-where.md).

## Keyed log analysis (v0.30.0)

`jsonl-dedupe`, `jsonl-sort` and `jsonl-count-by` require an explicit string-valued
`pointer`. Shorthands are `jsonl-dedupe:/id`, `jsonl-sort:/time` and
`jsonl-count-by:/level`; use recipe `reverse = true` or the single-command
`--reverse` for descending field sort. A `:reverse` suffix inside the shorthand
pointer remains literal. All records, including discarded duplicates, are checked.
See [full keyed analysis contracts and examples](jsonl-key-analysis.md).
