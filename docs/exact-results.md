# Exact native data results / 精确交付原生数据结果

`zi --exact 'instruction'` uses the configured model to choose **one** native data tool
call. After that call succeeds, zi returns its native result directly. It does not ask
the model to restate, summarize or format the successful result.

```sh
cat data.json | zi --exact 'Join /abc/0 with a hyphen'
zi --exact -i app.log -o recent.log 'Keep the last two ERROR lines, preserving spaces'
zi --exact --json -i data.json 'Select /payload/items and retain JSON types'
```

The model still receives the instruction and supplied input. This mode guarantees native
execution and delivery of the **selected operation**, not perfect model interpretation.
Check whether the chosen pointer/filter/count matches your intent. If the operation is
already known, offline `zi --count`, `--select` or `zi data` avoids the model entirely.
Use ordinary zi for explanation, writing or open-ended project tasks.

## Execution contract

- Only `count_input`, `select_input` and `transform_input` are eligible. No new tool names
  are added. Put compound deterministic operations in one transform's ordered `steps`.
- All existing protocol status/refusal/text-shape/call-ID/batch checks run first. Exact
  mode then requires one native call; text-only answers, multiple calls and nondata calls
  fail before dispatch. Companion model text is never the exact result.
- A native argument/limit error is returned as a correlated observation so the model can
  correct it within `--max-steps`. Each attempt uses original input, never hidden previous
  results. Exhaustion produces no final output. Successful native output ends the task
  immediately, normally after one model request. Configured HTTP retries retain their
  existing bounded semantics; one logical turn does not always mean one HTTP attempt.
- `--json` additionally validates/compacts the native result with the lexical formatter.
  A decoded selection string may not itself be JSON; the model can recover by using a
  JSON-preserving transform. Number spelling, repeated keys and existing escape tokens
  are not round-tripped through a numeric object tree. Error-shaped selected JSON data
  remains successful data when native execution succeeded.

`--exact` is incompatible with `--stream`, `--no-tools`, project selectors, local count/
selection modes, configuration templates, offline commands and management commands.
Requested conflicts fail before configuration/input/output access. `--no-stream` is
allowed but redundant: exact mode is always buffered, including in a rich terminal.
Unexpected provider SSE is rejected without displaying a prefix.

## Bytes, files and display

For pipes and explicit files, return the native text, append one LF only when a nonempty
result lacks a final LF, and emit/save **zero bytes** for a successful empty result.
Native operation rules still apply: line operations normalize CRLF to LF; JSON operations
use their existing syntax/token contracts. The transform's outer output envelope is
decoded once; the contained result is not rewritten by a model.

`-o` uses existing complete-result/no-clobber/explicit-force publication. Failures never
publish a partial result or replace an existing target. Rich activity remains on stderr;
stdout skips rich answer reformatting. Direct terminal display retains control cleanup
and JSON-safe escaping as documented in [terminal output](terminal-output.md), so screen
display is not a raw-byte guarantee. Pipe/file results remain native data.

Interactive tasks retain the completed native result under the existing one-result
export contract. `/save` can save an empty success as an empty file. New tasks, failures,
model changes and `/clear` keep their existing stale-result protections. Interactive tasks
still use the existing supplied-input rules and independent requests; the model does not
receive previous results as conversation memory.

Existing native limits remain: bounded CLI input, 8 MiB/100000-item transform input,
16 MiB transform intermediates and 64 KiB serialized transform envelope; selection tool
text is at most 64 KiB. Exact mode does not expand these limits, follow logs in the
background, enable filesystem tools or add persistent history/dependencies.

## Evidence and limits

A bounded real-service v0.26.0 sample selected the correct ERROR filter/tail pipeline;
the native observation retained a final space while the model's final restatement lost
it. That observation motivated this explicit delivery mode. Controlled tests cover all
three protocols, direct values, correction loops, valid JSON recovery, error-shaped data,
large numeric tokens, invalid batches, streams, files and terminal export. A few live
samples on one model are examples, not a statistical benchmark or a promise for all models.

Since v0.28.0, --exact --export-plan can return a validated transformation definition
instead of executing it. This TOML-only mode advertises transform_input proposals only;
supplied data is context and definition checking is not a data dry-run. See
[model plan export](model-plan-export.md) before reuse.
