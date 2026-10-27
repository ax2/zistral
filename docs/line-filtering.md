# Literal line filtering / 字面行筛选

Select explicit log or text lines locally without model configuration:

```sh
zi data lines-include --contains ERROR -i app.log
zi data lines-exclude --contains healthcheck -i app.log
zi data --step lines-include:ERROR --step lines-exclude:healthcheck --step dedupe -i app.log
zi data --show-plan --step lines-include:ERROR --step lines-exclude:healthcheck -o errors.toml
zi data --recipe errors.toml -i app.log -o errors.log
zi data --recipe errors.toml -i app.log | zi -s builtin:triage
```

`lines-include` keeps each whole line containing the exact substring. `lines-exclude`
keeps each whole line without it. Both require `contains`, a nonempty literal of at most
4096 **UTF-8 bytes**, with no CR or LF. Leading/trailing spaces and tabs are meaningful.
For a literal starting with a hyphen, pass `--contains=-value` (or use a recipe).

Matching is case-sensitive, with no regex, glob, escapes, Unicode normalization, locale
or severity interpretation. `a.*` matches those three characters; `ERROR` does not match
`error`; composed `é` does not match `e` followed by a combining accent. A literal can
contain colons, quotes, backslashes or tabs. The inline `--step lines-include:TEXT` and
`lines-exclude:TEXT` forms retain everything after the first operation prefix, including
`:reverse`. That suffix is sorting syntax only when the operation is `sort`.

Order, repeated lines and line contents remain. LF and CRLF separate lines; output uses
LF and terminates every retained line, even when the final source line had no terminator.
A bare CR is line content, not a line boundary. An initial UTF-8 BOM follows the normal
input-reader rule and is removed. Blank lines do not contain a nonempty literal, so include
drops them and exclude keeps them. Empty input or no retained lines returns **success and
zero bytes**, not an error for no match. No report, count or decoration enters stdout.

Example (CRLF/LF differences aside):

```text
ERROR start
INFO ok
ERROR healthcheck
ERROR start
```

Include `ERROR`, exclude `healthcheck`, then dedupe yields `ERROR start` once. Without
the dedupe step, the repeated line stays. This makes the order and meaning of each step
explicit rather than silently combining selection, sorting and cleanup.

## Saved definitions and model tools

The repository example [log-errors.toml](examples/log-errors.toml) is this version-1 recipe:

```toml
version = 1
[[steps]]
operation = "lines-include"
contains = "ERROR"
[[steps]]
operation = "lines-exclude"
contains = "healthcheck"
[[steps]]
operation = "dedupe"
```

The file contains definitions only, not input/output paths, history, model settings or
permissions. Select it with `--recipe`; no automatic project discovery, includes or shell.
`--show-plan` validates and exports without reading stdin, loading model settings or running
the filters. Export preserves literal meaning but not comments or original TOML formatting.
Single-operation `--contains` cannot be mixed with `--step` or `--recipe`; put the literal
inside each step instead. `contains` is invalid on all other operations, even empty or null;
reverse/pointer/fields retain their existing operation-specific restrictions.

`transform_input` accepts the same definitions, for example:

```json
{"steps":[{"operation":"lines-include","contains":"ERROR"},{"operation":"lines-exclude","contains":"healthcheck"},{"operation":"dedupe"}]}
```

The existing model tool consumes the current intermediate within that one call. A later
call starts from original task input again; it does not use earlier filtered output. No
new tool names, project permissions, loop behavior or persistent memory are introduced.

Filtering **inside** a model task cannot undo the original input already sent to that
provider. To choose what enters a model task, run the offline data command first and pipe
its result explicitly. A substring filter is not a general redaction mechanism: other lines
can still contain sensitive text. It does not inspect JSON fields, parse CSV records or
understand log levels. Filtering physical lines of multiline CSV/JSON can break their
format; select structured values with the appropriate native operation instead.

## Failure and resource limits

All steps and literal options are validated before data IO or output preparation. A missing,
empty, oversized, wrong-type or multiline literal fails even for empty data. Input must be
UTF-8; inherited model configuration is not loaded for offline commands.

Existing limits apply: 1–16 steps, 64 KiB recipe/normalized export, 8 MiB original input,
16 MiB each intermediate/final CLI output, **100000 source lines per line operation**.
Discarded lines still count; filtering cannot bypass the line/input cap. UTF-8 byte and
serialized JSON envelope caps are checked, not just character counts. Model tool arguments
and final JSON result remain limited to 64 KiB. Whole results are buffered; this is not
unbounded record streaming or a replacement for large-file grep.

Only the final successful result is emitted. A later JSON or size failure exposes no
intermediate output. `-o` keeps no-clobber defaults and explicit `--force`, including safe
in-place processing after input has been read; failures preserve the existing result file.
Shell `>` redirection can truncate a destination before zi starts and is outside that file
protection. Errors avoid quoting input/literal data in runtime validation; ordinary argument
parser diagnostics may echo command-line values.
