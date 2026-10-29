# Stream accumulation / 流式片段累积

v0.29.1 changes one internal string-append helper. Chat content/tool identifiers,
function names/argument text and Anthropic text/thinking/signature fields append to
the existing String instead of copying the complete prefix for every fragment.
A missing/nonstring slot still becomes the received text. Responses handling does
not use this accumulator and is unchanged. There are no new flags or dependencies.

For n fragments of k bytes, repeatedly copying prefixes copies roughly
k*n*(n-1)/2 preceding bytes before accounting for normal parsing. String::push_str
grows the existing buffer with amortized allocation instead. This does not mean
zero copying or zero allocations, and final responses are still retained as before.

## Controlled decoder measurement

The table compares both complete decoder modules in **one optimized executable**,
with identical in-memory SSE input, the same serde_json/anyhow dependency versions
as the product, opt-level s/LTO/codegen-units1, Windows x64 and Rust1.99.1.
Numbers are local wall-clock milliseconds spent decoding; each value is the median
of5 alternately ordered old/new runs. Transcript creation, compilation, model/network
latency and terminal/file output are excluded. This is not an end-to-end benchmark.

| Protocol | Fragments | Text UTF-8 bytes | Before ms median | After ms median |
| --- | ---: | ---: | ---: | ---: |
| chat | 2048 | 262,144 | 30.85 | 2.17 |
| chat | 8192 | 1,048,576 | 1192.49 | 15.58 |
| chat | 8192 | 4,194,304 | 10703.31 | 21.07 |
| anthropic | 2048 | 262,144 | 31.74 | 2.76 |
| anthropic | 8192 | 1,048,576 | 1301.62 | 13.47 |
| anthropic | 8192 | 4,194,304 | 14673.68 | 19.13 |
| responses | 2048 | 262,144 | 1.46 | 1.62 |
| responses | 8192 | 1,048,576 | 6.77 | 6.87 |
| responses | 8192 | 4,194,304 | 13.58 | 12.36 |

All9 cases compare complete response Values and callback text exactly, including
Unicode. Truncating each transcript also yields the same error. Responses is a
control, not a promised performance improvement; small timing differences are not
evidence of a protocol change. No peak-memory/RSS, allocation count or model-speed
measurement is claimed. Reproduce on the intended machine before extrapolating.

```sh
python scripts/benchmark_stream.py --before v0.29.0 --after v0.29.1 --output stream-benchmark.json
```

This opt-in developer script reads Git refs, copies both modules to an isolated
temporary Cargo project, uses the comparison ref's locked versions, builds offline,
and writes a new JSON file (or stdout). Python3.11+, Cargo and cached dependencies
are development requirements only. The existing installed zi remains one executable.
It does not change the checkout, contact a model, add a benchmark runtime to zi,
download dependencies or overwrite an existing result. The intentionally slow
baseline/full comparison can take several minutes, so it is not part of routine CI.

## Preserved contracts

- Same callbacks, text, tool fields, thinking/signature metadata and usage.
- Same SSE UTF-8/wire-size/completion checks, invalid/truncated-stream errors and
  provider turn/batch validation. No larger limits or implicit retry behavior.
- Same terminal cleaning, pipe/file output and newline behavior. Explicit streaming
  can still expose partial text on failure; buffered/file output retains its rules.
- No native operation, model tool, project permission, Agent-loop branch, config
  migration, implicit task context or memory is added.

New regressions cover fragmented Chat tool fields, Anthropic hidden metadata and
2048-fragment Unicode pipe output across all3 protocols. The earlier streaming,
failure, retry and native-task suites remain unmodified and continue to run.
