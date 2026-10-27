# Offline data pipelines / 离线处理流程

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
Only `sort` accepts `reverse = true`; `sort:reverse` is its CLI shorthand. Single-operation
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
