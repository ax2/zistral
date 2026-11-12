# Offline data pipelines / 离线处理流程

## Embedded recipes / 内置配方（v0.31.0）

zi embeds reviewed native recipe definitions in the executable.
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
| `builtin:top-error-codes` (v0.34.0) | Keep exact `ERROR` levels, rank string `/error/code` counts, emit 20 groups as JSONL. |
| `builtin:top-http-5xx-codes` (v0.34.0) | Rank string `/error/code` values from integer `/status` values 500–599. Skip absent paths explicitly. |
| `builtin:recent-unique-events` | Keep first string `/id`, stably sort string `/time` descending, take first 20 records as JSONL. |
| `builtin:log-context` (development) | Keep lines around a literal match supplied as `--var needle=VALUE`, then take the last 20 retained lines by default. |

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

## 排查 JSONL 日志中的高频错误

```sh
cat events.jsonl | zi data --recipe builtin:top-error-codes
zi data --show-plan --recipe builtin:top-error-codes -o error-report.toml
zi data --recipe error-report.toml -i events.jsonl
```

这个配方精确匹配字符串 `/level = "ERROR"`，然后统计命中记录中的字符串
`/error/code`。输出是 `{"value":"TIMEOUT","count":12}` 这样的 JSONL，按次数从高到低保留
前 20 组，同频保留首次出现的顺序。所有记录都需要有 `/level`，存在但不是字符串的级别不匹配。
只有命中记录需要字符串 `/error/code`。无效的 Unicode 字符串即使出现在未匹配的记录中也会报错。
没有匹配时输出为空。导出后可更改级别、字段路径或条数，再运行自己的配方。
日志结构混杂时，可以在导出的第一个 `jsonl-where` 步骤显式加上
`if_present = true`，只跳过级别路径缺失的有效记录。如果旧日志有级别却缺错误码，
也可以给第二个 `jsonl-count-by` 步骤单独启用该选项。存在但不是字符串的错误码仍会报错。

## 从 HTTP 5xx 统计定位文本日志

把下面的内容保存为 UTF-8 的 `requests.jsonl`：

```jsonl
{"status":499}
{"status":500,"error":{"code":"TIMEOUT"}}
{"status":503}
{"message":"healthcheck"}
{"status":599,"error":{"code":"TIMEOUT"}}
{"status":502,"error":{"code":"BAD_GATEWAY"}}
{"status":600}
```

```sh
zi data --recipe builtin:top-http-5xx-codes -i requests.jsonl
zi data --show-plan --recipe builtin:top-http-5xx-codes -o service-errors.toml
zi data --recipe service-errors.toml -i requests.jsonl
```

```jsonl
{"value":"TIMEOUT","count":2}
{"value":"BAD_GATEWAY","count":1}
```

区间包含 500 和 599。配方跳过没有状态码的记录，以及没有错误码的 5xx 记录。
状态码必须是整数，命中的错误码必须是字符串。字段存在却是 `null` 或其他错误类型时，
整次运行失败。报告按频次降序，最多 20 组，同频按首次出现顺序。导出的四个步骤可以
分别调整路径、缺字段规则、阈值和条数。配方源文件见
[top-http-5xx-codes.toml](examples/top-http-5xx-codes.toml)。

如果应用的文本日志也记录了 `TIMEOUT`，可以用字面匹配保留附近的堆栈：

```sh
zi data lines-include --contains TIMEOUT --context 3 -i app.log -o timeout-context.log
```

统计和摘录都是离线命令，适合先核对错误分布和原始日志。上下文的具体行为见
[字面行筛选](line-filtering.md)。

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
shell commands, expressions, implicit project discovery or model requests.
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

## Pass parameters to a recipe / 每次运行传入参数（开发中）

When the steps stay the same and only a match value or a line count changes,
save a version-2 recipe and use `--var NAME=VALUE`. The
[log-context example](examples/log-context.toml) keeps lines near the specified
substring, then selects the last part of that filtered output.

```toml
version = 2

[[steps]]
operation = "lines-include"
context = 1
bind = { contains = "needle", context = "nearby" }

[[steps]]
operation = "lines-tail"
lines = 20
bind = { lines = "limit" }
```

```sh
cat app.log | zi data --recipe log-context.toml --var needle=TIMEOUT
zi data --recipe log-context.toml --var needle=DENIED --var nearby=2 --var limit=10 -i app.log
zi data --show-plan --recipe log-context.toml --var needle=TIMEOUT -o resolved.toml
```

`bind` maps a step field to a parameter name. A literal value in the same step
is the fallback for that field. In this example `needle` is required, while
`nearby` and `limit` default to 1 and 20. Bindings accept `pointer`, `contains`,
`equals`, `at_least` and `at_most` as text, and `context` and `lines` as
unsigned decimal counts. The resolved value still has to satisfy the operation's
normal rules, including exact signed integer comparisons for JSONL filters.

Names use 1–32 ASCII identifier characters and cannot be `input`. A recipe
supports up to 16 distinct names. Repeat `--var` once per supplied name, with
up to 4096 UTF-8 bytes per value. A name reused across steps must consistently
bind text or consistently bind line counts. The CLI rejects unknown, duplicate
and missing required values before it opens the input or prepares output.
Version-1 recipes use literal values.

参数替换只发生在标有 `bind` 的字段，写入的值按字面处理。导出的 `resolved.toml`
是含本次参数值的版本 1 配方，适合复查或重跑；分享前应检查其中是否包含不宜公开的匹配词。

Use the embedded `builtin:log-context` to run the example from any directory.
It requires `needle` and accepts the same optional `nearby` and `limit` values
as the file example.

```sh
zi data --list-parameters --recipe builtin:log-context
cat app.log | zi data --recipe builtin:log-context --var needle=TIMEOUT --var nearby=2
zi data --show-plan --recipe builtin:log-context --var needle=TIMEOUT -o resolved.toml
```

`--list-parameters` returns JSON for either a file or an embedded recipe.
The report's `version` is 1 and `recipe_version` identifies the source format.
Each name in `parameters` has a `type` of `text` or `line_count`, a
`required` flag, and one-based `bindings` with the step number and field name.
A name is required if any of its bindings has no fallback. The report omits
literal values and shows an empty parameter object for a version-1 recipe.
It reads only the definition and rejects `--input` and `--var`. Use
`--show-plan` with the values you intend to supply to validate the resolved
operations. You can save the parameter report with `-o`.

## Copy a definition for your team / 复制并编辑配方

```sh
zi data --export-recipe --recipe builtin:log-context -o team-log.toml
# Edit context or lines in team-log.toml to set your team's defaults.
cat app.log | zi data --recipe team-log.toml --var needle=TIMEOUT
```

`--export-recipe` copies the selected TOML definition, including its version,
bindings, defaults and comments. It accepts a file or a `builtin:` reference,
checks the syntax and declarations, and preserves the source bytes. Export
requires no parameter values. It rejects `--input` and `--var` and writes
to stdout or the path selected with `-o`. The output file uses the usual
no-overwrite rule unless you pass `--force`.

用 `--export-recipe` 保存可继续传参的配方。用 `--show-plan` 和实际参数保存这一次
执行所用的版本 1 计划。

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
