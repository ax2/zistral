# Offline data pipelines / 离线处理流程

Combine native data operations without a model, shell subprocess or intermediate stdout:

```sh
cat lines.txt | zi data --step trim --step dedupe --step sort:reverse
zi data --recipe cleanup.toml -i lines.txt -o clean.txt
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

Operations: `trim`, `dedupe`, `sort`, `json-format`, `json-minify`, `json-keys`.
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
