# First and last line windows / 精确取首尾行

`zi data lines-head --lines N` and `zi data lines-tail --lines N` select the first or
last **explicit** number of lines offline. `-n N` is the short form; N must be an integer
from 1 to 100000. No model, key or configuration is needed.

```sh
zi data lines-tail -n 50 -i app.log
zi data --step lines-include:ERROR --step lines-tail:20 -i app.log
zi data --step lines-include:ERROR --step lines-tail:20 --show-plan -o recent.toml
zi data --recipe recent.toml -i app.log
zi data --recipe recent.toml -i app.log | zi 'Summarize these recent errors'
```

Each selected line keeps its characters, leading/trailing whitespace, duplicates and
original order. Blank lines count. CRLF becomes LF, as with existing line operations;
each selected line ends in LF. A final newline terminates a line, not an extra blank
record. Empty input produces no bytes. If N exceeds the available lines, keep them all.
This is a text-line operation, not a CSV-record window or JSON-aware selector. Quoted CSV
fields can contain line breaks; use format-aware transformations for that data.

Filtering then taking a tail selects the most recent matching lines. Taking a tail then
filtering examines only that tail and can return fewer matches. Pipeline order is explicit.
There is no follow mode, background watcher, time interpretation, byte-range seeking or
implicit log discovery.

Version-1 recipe step:

```toml
version = 1
[[steps]]
operation = "lines-include"
contains = "ERROR"
[[steps]]
operation = "lines-tail"
lines = 20
```

The same operation/count is accepted by `transform_input`, either directly or in its
ordered steps. The model must supply `lines` as a JSON integer. Null, strings, fractions,
negative/zero/out-of-range values and counts on unrelated operations fail. Tool invocations
still start from original input; only the steps within one invocation consume intermediate
results. The tool does not load recipe files or read additional paths.

Definitions are validated before data/output access. All input bytes and **all lines,
including discarded lines**, count toward the existing 8 MiB input/100000-line limits.
This buffers bounded input; it is not a way to stream arbitrary-size logs. Intermediates
remain at most 16 MiB, the CLI result at most 16 MiB, and the tool's serialized result at
most 64 KiB. A later failing stage publishes no prefix or replacement file. Existing
no-clobber/explicit-force rules remain.

CLI input already removes one leading UTF-8 BOM before processing. That behavior is not
new and is not performed again at every step. No new dependency, tool name, permission,
Agent-loop state, streaming protocol or persistent memory is introduced.
