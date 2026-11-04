# Explicit model-output budgets (unreleased)

This capability is on `main` for the next grouped release. Published v0.33.0 only
supports the older Anthropic-specific `--max-output-tokens` option. There is no new
runtime, tokenizer, dependency or model-tool permission.

Use an explicit requested cap for each model turn:

```sh
cat input.txt | zi --max-output-tokens 256 'Summarize the input'
zi --preview --no-stream --max-output-tokens 256 'Explain the input'
```

| Selected protocol | Request field | Without an explicit cap | Accepted zi range |
| --- | --- | --- | --- |
| Chat | `max_completion_tokens` | omitted | 1–65536 |
| Chat with `--chat-token-limit-field max_tokens` | `max_tokens` | selector requires an explicit cap | 1–65536 |
| Responses | `max_output_tokens` | omitted | 16–65536 |
| Anthropic | `max_tokens` | 4096 | 1–65536 |

The service/model can impose a smaller maximum or other restrictions. zi's range
does not promise that every service accepts every value. No unspecified Chat or
Responses request gains a new parameter, and Anthropic keeps its existing default.
An interactive `/use NAME` validates the effective protocol and budget before
saving the selection; an incompatible switch preserves the current profile and
the last result available to `/save`.

## Choose Chat compatibility explicitly

Modern OpenAI Chat uses `max_completion_tokens`, including reasoning tokens;
legacy `max_tokens` is deprecated and unsupported for o-series models. Some
OpenAI-compatible services instead require the legacy field. Choose that field
explicitly when the selected service documents it:

```sh
zi --max-output-tokens 256 --chat-token-limit-field max_tokens 'Explain pipes'
```

zi sends one selected field, does not infer capabilities from a model name or
service host, and does not retry with another field after a rejection. A 400 or
other nontransient rejection remains an error; explicitly enabled transient
retries repeat the same body and cap. The selector is valid only for Chat and
requires a cap, except for a diagnostic probe with its fixed budget.

Sources: [OpenAI Chat fields](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[Responses limit](https://developers.openai.com/api/reference/resources/responses/methods/create),
[Anthropic Messages](https://platform.claude.com/docs/en/api/messages/create).

## What is bounded

The requested cap is per model turn, including each transient retry, not a total
task or billing budget. Use `--max-steps`, `--retries` and request timeouts to bound
other dimensions. Reasoning tokens can consume the budget before visible text.
No client-side tokenizer or token-price accounting is introduced, and zi cannot
guarantee that an arbitrary compatible service honors a requested parameter.

The same cap appears in the first-request preview, streaming requests and every
tool-follow-up request. It does not limit native tool-result text, deterministic
`zi data` output, or an `--exact` result. Existing byte/step limits remain in place.
Offline/management modes reject a normal-task cap rather than silently ignore it.

Truncated or incomplete model answers still fail. Buffered stdout stays empty and
an existing output file is preserved. Streaming can already have emitted text;
the exit status remains nonzero. This is not automatic continuation, partial-file
publication or silent success.

## Doctor keeps its small probe

`zi doctor --probe` keeps a fixed 16-token cap; omit `--max-output-tokens` for this
command. Its default Chat field stays `max_tokens` for existing compatible services.
Select a modern field explicitly when needed:

```sh
zi --chat-token-limit-field max_completion_tokens doctor --probe
```

The probe has no tools or input/task instructions, at most one model turn, no
retries and at most 15 seconds per request. It still hides answers and provider
error bodies, and it may fail if a reasoning model exhausts this small budget.
Tests use synthetic local servers; they do not prove real-service/model quality.
