# Exact integer ranking / 精确整数排名（v0.31.0）

Frequency counts are integers. The existing `jsonl-sort` compares JSON strings and
intentionally rejects numeric fields, so it cannot rank `jsonl-count-by` output.
Use an explicit integer operation for counts and other integer-valued fields:

```sh
zi data jsonl-sort-int --pointer /count --reverse -i counts.jsonl
cat events.jsonl | zi data --recipe builtin:top-severity-counts
zi data --show-plan --recipe builtin:top-severity-counts -o top.toml
# Edit /level or the lines limit, inspect the definition, then reuse it offline:
zi data --recipe top.toml -i events.jsonl
```

With WARN appearing twice first and ERROR ten times later, raw counts retain
first-seen order, WARN:2 then ERROR:10. The new builtin returns quoted CSV:

```csv
"value","count"
"ERROR","10"
"WARN","2"
```

It counts decoded string `/level` values, stably sorts integer `/count` descending,
takes the first 20 groups and exports CSV. Tied counts retain the original first-seen
category order. Empty input fails at CSV export because there are no groups from
which to define columns; no header or result prefix is invented. This counts all events; to count unique events, export the definition
and explicitly insert the existing `jsonl-dedupe` step before counting.

## Integer semantics

`jsonl-sort-int` requires an explicit RFC 6901 pointer; an empty pointer selects each
root value. Every selected value must be a JSON **integer literal**: `-10`, `0`, `2`,
or a longer signed digit sequence. There is no fixed machine-integer precision bound,
although all existing input/output and record limits apply. Digits are borrowed from
the validated record and compared by sign, magnitude length and digits; no float,
big-number dependency, inferred type or date parser is used.

`-0` and `0` compare equal while keeping their original spelling and input order.
Equal values stay stable in both directions; `--reverse` reverses key comparison,
not the completed result. Whole record payloads are retained, including large numbers,
exponent spelling, duplicate unrelated fields and string escapes. Insignificant JSON
whitespace is compacted and records end with LF, as for existing keyed operations.

小数和指数写法（如 `1.0`、`1e0`）会被拒绝，即使数学值恰好是整数。字符串 `"2"`、
布尔、null、数组和对象都不作类型转换。路径不存在、索引有前导零、路径上的对象键重复歧义、
非法 JSON 或字符串 Unicode 无效时，整次处理失败，不跳过记录、不输出部分结果，也不替换
已有 `-o` 文件。原有 `jsonl-sort` 仍按字符串排序，行为不变。

The existing limits remain: 8 MiB original input, 16 MiB per intermediate/final CLI
result, 100000 records, JSON nesting128, pointer4096 UTF-8 bytes/128 tokens, and
1–16 steps/64 KiB per recipe. A model tool result must also fit its existing64 KiB
serialized envelope. Sorting is complete-buffered; it does not stream records.

## Compose, inspect and reuse

```toml
version = 1
[[steps]]
operation = "jsonl-count-by"
pointer = "/level"
[[steps]]
operation = "jsonl-sort-int"
pointer = "/count"
reverse = true
[[steps]]
operation = "lines-head"
lines = 20
[[steps]]
operation = "jsonl-to-csv"
```

The single CLI command accepts `--reverse`. The inline form
`--step jsonl-sort-int:/count` sorts ascending; everything after the prefix is a literal
pointer, so `:reverse` inside that pointer is not a direction flag. Use a version-1
recipe for a descending sort inside a compound pipeline, or the builtin shown above.

The same typed operation is available through the existing `transform_input` tool,
including `--exact` and `--exact --export-plan`. A successful exact result is published
without a model rewrite; exported plans are only validated definitions and can later
run offline. No additional model tool names, filesystem permissions, Agent-loop branches,
dependencies, saved inputs or session memory are introduced.
