# Structured diagnostics (unreleased)

`zi doctor --json` is on unreleased `main` for the next grouped release. It emits
one compact, newline-terminated JSON object for scripts and pipes; normal
`zi doctor` and interactive `/doctor` keep their human-readable output.

```sh
zi doctor --json | jq '.configuration.status, .probe.status'
zi -p work doctor --json --probe
zi --no-profile-budget doctor --json
```

Put this `--json` **after** `doctor`. The root `zi --json INSTRUCTION` option still
requests JSON model output; `zi --json doctor` is not diagnostic JSON mode. Usage
errors rejected before diagnostic execution (including invalid flags/modes) use
the normal CLI error path and need not produce a report.

## Version-1 contract

Every report has `version: 1`, `kind: "doctor-report"`, `zi_version` and boolean
`ok`. `ok` means that the requested local checks and optional probe passed; an
offline success is not evidence of network access or model quality.

| Field | Meaning |
| --- | --- |
| `configuration.path` | Selected config path, or null when unavailable/omitted |
| `configuration.status` | `ok`, `failed`, or `unavailable` for report-size failure |
| `configuration.details` | Effective snapshot when valid; otherwise null |
| `probe.requested` | Whether `--probe` was explicitly selected |
| `probe.status` | `not_requested`, `not_run`, `ok`, or `failed` |
| `probe.max_requests` | Maximum allowed requests: 0 offline, 1 with probe; not a measured request count |
| `probe.max_output_tokens` | 16 for an explicit probe, otherwise null |
| `probe.field` | Selected protocol budget field when known, otherwise null |
| `probe.elapsed_ms` | Duration after a probe was attempted; otherwise null |
| `error` | Null on success; otherwise a safe error object |

`configuration.details` contains profile/model/protocol/service-host metadata,
credential presence/source, effective output-budget values/sources and names of
other present configuration environment variables. It describes the selected
profile plus current overrides, not every saved profile. Environment presence
does not imply that a variable defeated an explicit CLI flag.

The credential object contains `configured`, `source` (`environment`, `profile`,
or `none`) and a winning `environment_variable` name or null. It has no key-value
field. The budget object contains `tokens_per_turn`, `field`, `tokens_source`,
`field_source`, and `profile_preferences_ignored`. The model-request field/source
are null when no model cap is requested; the probe reports its own fixed cap.

Scripts should check `version` and tolerate additional fields. Prefer these
typed fields to parsing human-readable advice. A diagnosed check failure exits
nonzero after writing its complete report; successful checks exit zero.

## Safe errors

The error object always has `kind`, `http_status`, `retries` and `message`:

| `kind` | Meaning |
| --- | --- |
| `configuration` | Effective configuration could not be loaded/validated |
| `http` | Request received a non-success HTTP status; status and retries are retained |
| `timeout` | HTTP client timeout |
| `network` | Other transport/TLS/connectivity failure |
| `request` | Request preparation failed, for example an invalid authentication header |
| `response` | Invalid/incomplete/unsupported model response, including forbidden tool requests |
| `report_limit` | Report metadata exceeds the encoded/display size limit |

For non-HTTP errors, `http_status` and `retries` are null. Messages are fixed safe
guidance; no raw error chain, provider error body, model answer, task input or
authentication value is included. Task errors and human-readable probes use the
same safe guidance for connection failures and omit private URL paths.
HTTP status comes from the typed request error,
not text parsing. It does not identify whether a service or intervening proxy
originated the response. The usual stderr failure notice is generic in this mode.

Configuration failures additionally have a fixed `error.code`, derived from a
typed validation phase rather than parsing the error message. `kind` remains
`configuration`; `configuration.details` remains null and a requested probe stays
`not_run`. Its message gives corresponding fixed guidance without field values.

| Configuration code | What to check |
| --- | --- |
| `config_file_unreadable` | Selected path, read permissions and UTF-8 encoding |
| `config_format_invalid` | TOML syntax, supported fields and value types |
| `profile_invalid` | Select an existing valid saved profile |
| `model_missing` | Configure a model or provide `ZI_MODEL` / `--model` |
| `service_url_invalid` | API base URL scheme, host and forbidden credentials/query/fragment |
| `protocol_invalid` | Choose `chat`, `responses` or `anthropic` |
| `output_budget_invalid` | Compatible cap/Chat field, or clear obsolete saved preferences |
| `credential_missing` | Provide an effective key for an official service |
| `endpoint_invalid` | Valid exact endpoint on the base URL's origin |
| `configuration_invalid` | Fallback when no known phase marker exists |

This describes the first failed effective check, not a scan of every saved
profile or an automatic repair. CLI overrides and `--no-profile-budget` can make
unused saved settings irrelevant. Malformed typed TOML still invalidates the
file. Other error kinds retain their existing fields and need not have `code`.
Treat future codes as unknown failures; do not infer that a request was attempted.
Missing/invalid configuration reports neither create directories/lock files nor
save changes or issue the requested probe. This is an additive version-1 field;
no new runtime dependency, command or diagnostic network behavior is introduced.

## Bounds and terminal behavior

Default checks are offline. `--probe` retains one non-streaming model turn, no
tools/retries, a fixed 16-token requested cap and at most 15 seconds per request
(a shorter `--timeout` applies). Saved field compatibility and explicit Chat
field selection still apply. A tiny probe can fail on reasoning/output limits;
it does not establish tool capability or general model quality.

Reports, including the final newline, are at most 64 KiB. Encoding is bounded
before stdout is written. Metadata is checked before probing, with reserved room
for safe status/error fields. Oversized reports become a small `report_limit`
failure with metadata omitted; they do not trigger a probe. Terminal output uses
JSON escapes for DEL/C1 controls, preserving decoded values while preventing
metadata from injecting terminal commands. That escaped size is checked too.
Pipe output stays ordinary UTF-8 JSON without colors, banners or spinner text.

Tests use synthetic local servers/configuration. Configuration, HTTP, protocol,
request, timeout and overflow failures all retain nonzero status and hidden keys;
they do not claim success against the user's real provider.
