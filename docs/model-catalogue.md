# Service model catalogue (unreleased)

This feature is on unreleased `main` for the next grouped release. Installed
v0.33.0 still supports the offline saved-profile list only.

```sh
zi models                        # Saved profiles, offline; unchanged
zi models --remote               # Selected service's model IDs, one per line
zi -p work models --remote --json # Versioned catalogue for scripts
zi models --remote | sort
zi models --remote --json | jq '.models[]'
```

Put the catalogue `--json` after `models`. The root `zi --json INSTRUCTION`
continues to request JSON model output and is rejected with management commands.
`models --json` requires `--remote`; it does not silently contact a service.
The interactive `/models` command keeps its offline saved-profile behavior.

## What is queried

The explicit remote command uses the selected profile plus normal CLI/environment
service/protocol/key overrides. It makes GET requests only, with no task body,
tool definitions, project context or stdin content. Configuration is never saved,
a model is never selected automatically, and no inference request is made.
It does not wait for stdin. Explicit `--input`, `--output`, task, retry, preview,
project and generation-budget flags are rejected before the request.

An existing model ID is not needed to discover IDs. Saved generation budgets and
the exact generation `endpoint` are irrelevant and ignored by this command. The
configuration must still be valid typed TOML and its selected profile/service
must be valid. Other commands continue to validate model generation settings.

| Protocol | Models URL | Authentication |
| --- | --- | --- |
| `chat`, `responses` | `BASE_URL/models` | Existing Bearer key |
| `anthropic` | `BASE_URL/models` when root ends in `/v1`; otherwise `BASE_URL/v1/models` | Existing `x-api-key` and `anthropic-version: 2023-06-01` |

Trailing slashes are removed; reverse-proxy path prefixes are retained. Use the
service API root as `base_url`, not a generation URL. The Anthropic version/auth
match existing zi generation requests; this does not migrate them to a newer API
version. Local/custom services may have no key. A service can support generation
without exposing a compatible models endpoint; HTTP 404 does not invalidate a
saved profile. Use `zi models` to inspect local profiles instead.

## Output contract and completeness

Plain output contains just IDs in provider order, with a trailing newline per
ID. A successful empty catalogue emits no plain stdout. Terminal-only information
goes to stderr. JSON success emits one compact newline-terminated object:

```json
{"version":1,"kind":"model-catalogue","profile":"work","protocol":"chat","service_host":"service.example","complete":true,"pages":1,"models":["vendor/model-a","vendor/model-b"]}
```

`complete` means the supported endpoint's response was completely read and its
supported pagination was exhausted. It is **not** a statement that the provider
exposes every model, that your account can invoke an ID, or that the model works
with the configured protocol, streaming or tools. No capability or quality test
is run. Unknown provider metadata is not emitted.

Anthropic pages use `limit=1000` and the validated `last_id` as a URL-encoded
`after_id`, always at the same endpoint. `has_more` is required. OpenAI-compatible
lists are treated as the documented unpaginated endpoint; a returned
`has_more:true` is rejected as unsupported pagination, never called complete.
Service-specific pagination schemes are not guessed.

IDs must be nonempty, at most 512 UTF-8 bytes, with no whitespace/control
characters; duplicates within/across pages and malformed records/cursors fail.
Optional returned object/type tags are checked when present. All pages are
validated before stdout is written: a later failure cannot leave a usable-looking
prefix. Validation/HTTP/read failures exit nonzero, leave stdout empty and send safe guidance to stderr
(including HTTP status when available). They do not emit a success-shaped JSON
object. Scripts must check the exit status before consuming a catalogue.

## Bounds and privacy

Limits: 1 MiB per page, 4 MiB cumulative response bytes, 4096 IDs and 10 pages.
The entire query has at most `min(--timeout,30)` seconds, including body reads;
connect timeout is at most 15 seconds. Redirects and retries are disabled. A limit,
invalid/truncated response, HTTP denial or transport failure produces no partial
stdout. Custom proxies and normal HTTP environment settings continue to apply.

Keys, raw transport errors/URLs, provider error bodies and unused metadata are not
printed. An ID echoing the effective credential is rejected. Known local config
validation errors use the ordinary management error path. The command adds no
runtime dependency, tool, remembered history, background service or configuration
field. It does not repair an account-level denial automatically.

Protocol references checked during implementation:
[OpenAI list models](https://developers.openai.com/api/reference/resources/models/methods/list),
[Anthropic list models and pagination](https://platform.claude.com/docs/en/api/models/list),
[Anthropic API migration](https://platform.claude.com/docs/en/api/migration-2026-10-21),
[official SDK v0.72.0 authentication/version headers](https://github.com/anthropics/anthropic-sdk-python/blob/v0.72.0/src/anthropic/_client.py).
