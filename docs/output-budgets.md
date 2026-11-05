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
does not promise that every service accepts every value. When no CLI or saved preference is supplied, Chat and Responses requests gain no
new parameter, and Anthropic keeps its existing default.
An interactive `/use NAME` validates the effective protocol and budget before
saving the selection; an incompatible switch preserves the current profile and
the last result available to `/save`.

## Save preferences once per model

For an existing Chat profile named `work`, save its budget without reopening the
service/key setup wizard:

```sh
zi config --name work --max-output-tokens 512 --chat-token-limit-field max_tokens
zi -p work 'Explain pipes'
zi config --name work --clear-output-budget
```

The `config` options go **after** `config`; the same-named root options before a
task are temporary overrides. Budget editing requires an existing profile and is
noninteractive, offline and atomic. `--name` chooses the profile to edit; otherwise
`--profile`/`ZI_PROFILE`, the saved selection, then `default` choose it. Editing a
budget does not activate a different default profile, change model/service/key/
endpoint settings, or modify other profiles. It validates the saved protocol,
not a temporary `--api`/environment override. It cannot combine with the setup
wizard's `--manual` or `--list` options. No stdin or model request is needed.

Equivalent TOML fields belong at the root for `default`, or inside one named
profile. Named profiles do not inherit the root's preferences:

```toml
[profiles.work]
model = "your-chat-model"
base_url = "https://service.example/v1"
api = "chat"
max_output_tokens = 512
chat_token_limit_field = "max_tokens"
```

An explicit CLI cap/field wins over the corresponding saved value. The Chat
selector can use a saved cap, so it need not repeat `--max-output-tokens`. A cap
without a selected field uses modern Chat's default field. Saved caps must be
1–65536 (Responses: at least 16), and a saved Chat field requires a cap and Chat.
No raw invalid value or configuration source excerpt is printed on errors.

```sh
zi -p work --max-output-tokens 256 'Short explanation'
zi -p work --chat-token-limit-field max_completion_tokens 'Use the modern field'
zi -p work --no-profile-budget 'Use protocol defaults for this task'
zi -p work --no-profile-budget --api responses --max-output-tokens 256 'Explain pipes'
```

`--no-profile-budget` ignores both saved preferences for one invocation; explicit
CLI budget values still apply, and Anthropic still has its required 4096 default.
It does not rewrite the file. Without that flag, a protocol override conflicting
with a saved Chat field fails explicitly. Invalid effective preferences fail
before task-input/output IO. Offline data commands still need no model config.

`--clear-output-budget` removes both saved preferences. It can repair invalid
budget values without activating that profile, requiring a model or supported
generation protocol, or reading input. The profile/configuration must already
exist. All unrelated fields and the saved active selection are preserved;
generation itself still requires valid effective settings, and setting new
preferences keeps the ordinary model/protocol checks. Clearing still requires
parseable TOML with the expected field types; syntax/type errors need correction
first. Before downgrading to an older strict parser, clear these new fields from
every affected root/named profile, including inactive or unconfigured ones. Regular manual
model edits retain compatible preferences; incompatible edits fail without
replacing the original configuration. Importing replacement service settings
resets that profile's preferences to protocol defaults and preserves all others.
Older binaries reject these new fields; clear them with this version before
downgrading. No preference fields are added to an unchanged older configuration.

`zi doctor` reports the effective cap/field and whether each came from CLI,
profile or protocol defaults. `--preview` shows the same resolved request. These
are explicit model settings, not task history, saved inputs or model memory.

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
requires a CLI or saved cap, except for a diagnostic probe with its fixed budget.

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
command. With no saved/CLI budget preference, its Chat field stays `max_tokens`
for existing compatible services. A saved cap or field uses the corresponding
resolved model field, while the probe cap stays 16. Select a modern field
explicitly when needed:

```sh
zi --chat-token-limit-field max_completion_tokens doctor --probe
```

The probe has no tools or input/task instructions, at most one model turn, no
retries and at most 15 seconds per request. It still hides answers and provider
error bodies, and it may fail if a reasoning model exhausts this small budget.
Tests use synthetic local servers; they do not prove real-service/model quality.
