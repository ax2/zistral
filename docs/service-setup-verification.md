# Service setup: a complete workflow (unreleased)

The service diagnostics, saved request budgets, credential references and remote catalogue described here
are on unreleased `main`. They are accumulating for a grouped release; v0.33.0
does not include them. A development commit or passing check is not publication.

## Configure, discover, diagnose, run

Start with your provider's model ID, API root, protocol and key. Existing Codex,
WorkBuddy or dsh settings can be imported through the interactive wizard:

```sh
zi config --name work
zi -p work models --remote
```

The first command saves a profile. Available source environment-key references are
retained as references, so their values are not copied into zi settings. See
[imported key references](import-key-references.md) for supported forms and fallbacks. The second explicitly queries that service for
IDs; it never selects or saves a returned ID. If necessary, edit the profile with
`zi config --name work --manual` using the returned ID and your provider's protocol
documentation. Catalogue availability does not prove inference access or tool
support. Plain `zi models` and interactive `/models` list saved profiles offline.

Save an optional per-turn requested output cap, then inspect the effective settings:

```sh
zi config --name work --max-output-tokens 512
zi -p work doctor --json
zi -p work doctor --json --probe
```

The budget edit changes only that existing profile's budget preferences. Ordinary
Chat requests use `max_completion_tokens`; explicitly add
`--chat-token-limit-field max_tokens` to the config edit for legacy services that
require it. Do not guess/retry field names. Responses uses `max_output_tokens`,
and Anthropic uses `max_tokens`. Read the effective field/source in the report.
The cap is a per-turn request parameter, not a total cost or billing guarantee.

Offline doctor makes no network request. The explicit probe sends one short,
non-streaming, tool-free request with a fixed 16-token requested cap, retaining
the selected compatible field. It sends no input/project context and does not
prove general model quality. A tiny cap can fail on some models. Inspect the
exit code and report's error classification; a real service denial is not fixed
by a successful local check.

Preview the first task request without sending it, then execute the same task:

```sh
zi -p work --preview --no-stream -i notes.txt 'Summarize in three sentences'
cat notes.txt | zi -p work --no-stream 'Summarize in three sentences' > summary.txt
cat notes.txt | zi -p work --stream --max-output-tokens 256 'Summarize briefly'
zi -p work --no-stream -i notes.txt -o summary.txt --force 'Summarize briefly'
```

Preview shows the task input and body but omits credential headers; review it
locally. For pipes, normal non-streaming completion is validated before stdout.
Explicit `--stream` can leave partial stdout on errors. An explicit `-o` target
is replaced only after successful complete output; a failed task preserves the
old file. Shell `>` has ordinary shell truncation behavior and cannot provide
zi's file-preservation guarantee. Streaming and `-o` are mutually exclusive.
CLI budget overrides apply for that invocation only, including tool followups;
they do not change saved preferences.

```sh
zi config --name work --clear-output-budget
zi use default
```

Clearing removes the profile's saved budget preferences, keeping its service,
model and key. Defaults resume (Anthropic 4096; Chat/Responses omitted). `use`
selects an existing saved profile; it does not copy settings between profiles.
Neither command needs a model connection. No history or session memory is added.

Maintain named profiles offline with `zi config --rename OLD NEW` or
`zi config --remove NAME`. Rename preserves settings and follows a saved active
selection; remove rejects the root `default` and saved active profile. Select
another profile before removing it. See [profile maintenance](profile-maintenance.md)
for external references and credential-removal semantics.

## Repeatable delivery verification

Run the same check against a freshly built, portable or installed executable:

```sh
python scripts/test_service_setup_delivery.py --binary /absolute/path/to/zi
```

The check uses Python 3.11+ on the **test host** only. Installed zi still needs no
Python/Node/Rust runtime. It creates temporary directories with spaces/Chinese
characters, synthetic TOML and fake keys; it isolates model/proxy/home settings
and contacts only its own loopback service. It does not modify a user's selected
profile, key, installed executable or PATH. All fixtures are removed on exit.

It tests the same journey under modern Chat, explicit legacy Chat, Responses and
Anthropic: 40 checks and 24 controlled HTTP requests. Requests verify actual
auth/protocol/budget fields; the first task body equals the preview, a real native
count result returns on the correlated tool followup, and saved/CLI/probe caps
remain distinct. Offline profile rename/remove and default-selection protection are checked too.
An explicit key-reference edit plus saved-literal removal is compared against the
entire reference mapping; subsequent requests must authenticate through the new
variable while the old variable remains present with a deliberately wrong value.
Exact Unicode stream output, error-body redaction, no retry and
old-file preservation are checked. The whole parsed configuration is compared
with a reference mapping after every intended edit or no-write operation.

Normal three-platform CI runs it against the built executable. Candidate installer
checks also run it against actual native copies on all three platforms. Windows
retains its install/upgrade/PATH ownership scenarios; Linux/macOS verify a portable
copy plus native install and same-version reinstall before cleanup. Release uses
the same Unix verifier on its final packages. These checks do not themselves
publish a Release or prove a previous-version upgrade/final-artifact result.
The helper's zero exit and JSON check count must both be verified. A nonzero exit
or missing/count-mismatched report is failure, even if earlier checks printed OK.

These are integration/delivery checks, not new runtime features or evidence of
real provider access, capability, billing, or model quality. Keep the Rust tests,
exact-commit CI, packaging checks and eventual official-artifact verification.

Detailed contracts: [output budgets](output-budgets.md),
[diagnostic JSON](doctor-json.md), [model catalogue](model-catalogue.md),
[file output](../README.md), [release gates](releasing.md).
