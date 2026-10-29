# Model answer display and raw output / 模型回答的显示与原文

`--plain` disables zi's decorative interface; it does not make provider-supplied terminal
commands part of that interface. Since v0.25.1, **complete model answers sent directly to
a terminal** use the existing control-sequence cleaner even when rich UI is disabled.
This includes `--plain`, `TERM=dumb` and a nonterminal stderr with terminal stdout.

The fix affects display only. It does not parse, rewrite or add model requests, tools,
project permissions or conversation state.

| Destination | Model answer treatment |
| --- | --- |
| Direct terminal, rich UI | Existing cleaner and rich formatting |
| Direct terminal, plain/dumb/fallback UI | Existing cleaner, plain text |
| Pipe or redirected stdout | Original result text and existing final-newline rule |
| Explicit `-o FILE` | Original complete result and existing file/newline rules |
| Interactive `/save` or `/save!` | Original retained successful result and existing file rules |

The cleaner removes terminal controls, including ESC-based CSI styling/cursor sequences
and OSC title/hyperlink payloads. Ordinary text, Unicode, newlines and tabs remain.
In a raw result file, those model-provided bytes remain data, as before. A downstream
program that later sends raw pipe/file text to a terminal controls its own display behavior.
This does not claim that every consumer of the output becomes a terminal sanitizer.

For example, a controlled service returning actual `ESC[31m` color bytes and an OSC title
sequence should display the readable text without applying those provider controls.
The same answer redirected to a file keeps the actual bytes; it is not replaced with the
literal words `ESC[31m`, nor silently shortened. Existing code examples containing ordinary
backslash escape notation remain ordinary text. These examples describe bytes, not commands
the user needs to type into a terminal.

Complete nonstreaming answers reuse `ui::clean`; streaming still uses the existing stateful
`StreamDisplay`, including controls split across chunks. This patch does not change SSE
decoding, retries, completion validation, partial-output rules or turn budgets. A failed
stream can still have displayed partial text under the existing explicit streaming contract.

Interactive result retention stays separate: display borrows the answer, while `/save`
keeps the original successful text. It does not capture the sanitized screen transcript.
JSON tasks still validate and compact before display/export. Since v0.25.2, model JSON
sent directly to a terminal uses JSON escaping instead of the ordinary text cleaner:
literal DEL/C1 characters in strings and keys become `\u007f` through `\u009f`.
The displayed JSON parses to the same values, without sending those literal controls to
the terminal host. Existing escape tokens, numeric spelling (including large exponents),
duplicate keys and ordinary Unicode pass through without parsing/reserializing the result.
Pipe/file output and retained `/save` results keep the validated compact original bytes.
This applies in default, `--plain` and `TERM=dumb` display modes; JSON remains buffered and
fully validated before anything is displayed.

A host-independent regression checks every DEL/C1 character in both string keys and values,
separately from the operating system's terminal renderer. Real terminals verify all three
model protocols, escaped display and original saved bytes. Separate pipe/file tests compare
literal controls and original number/escape tokens byte for byte. No live model is needed.

Offline count/select/data operations, configuration export and management commands retain
their existing contracts. This fix is for model-answer display, not a general rewrite of
user-selected input data or every stdout producer.

Regression coverage uses harmless color, title and link markers in headless real terminals,
for Chat Completions, Responses and Anthropic. Both plain and `TERM=dumb` display modes
reject provider title/link metadata while preserving visible text and original `/save`
bytes. Separate redirected/file tests compare original UTF-8/control/CRLF bytes with and
without `--plain`. No clipboard-control sequence or real provider request is required.

Since v0.29.1, internal Chat/Anthropic string accumulation avoids repeated prefix
clones. Display/cleaning, Responses, callbacks and output/error contracts stay unchanged;
controlled decoder measurements are [documented separately](stream-performance.md).
