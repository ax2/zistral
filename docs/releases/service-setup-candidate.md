# Unreleased grouped candidate: model-service setup and reliability

This records the current batch for review; it does not select a version or publish
a release. The latest stable release remains
[v0.33.0](https://github.com/ax2/zistral/releases/tag/v0.33.0).
The verified runtime/package source is commit
[`4e0d61a`](https://github.com/ax2/zistral/commit/4e0d61ae8202cb13c17f3ff76abf4867b77e5245).
Development package metadata still says `0.33.0`; CI candidate packages with that
version are not the published v0.33.0 assets. Subsequent source changes need their
own evidence. Documentation-only followups do not turn these into final artifacts.

## The use case

A small pipe-native assistant should make it straightforward to configure a
service, inspect the effective model/key settings, make a bounded explicit check,
and run a task with predictable request preferences. The batch addresses that
journey without adding tools, task history, model weights, a daemon or runtime
dependencies. Existing offline data/recipe workflows remain model-independent.

| User need | Delivered behavior | Contract |
| --- | --- | --- |
| Understand which service/key will actually be used | Human doctor reports the effective profile and winning credential source; explicit probing uses one short tool-free turn | No key values, retries or task/project input in a probe; fixed 16-token request cap and at most 15 seconds |
| Limit an individual task's requested model output | Explicit per-turn cap for Chat, Responses and Anthropic, including stream/tool followups | Protocol field chosen explicitly; no model-name guessing or fallback-field retry; a requested cap is not a total cost guarantee |
| Reuse preferences while switching models | Save, override, temporarily ignore or clear a profile's request-budget preferences | Named-profile isolation; CLI overrides are invocation-only; incompatible effective preferences fail before task IO |
| Diagnose from a script or pipe | `doctor --json` emits one bounded version-1 report with a meaningful exit code | Typed safe errors; default checks offline; configuration success does not establish inference access |
| Discover IDs exposed by a configured service | Explicit `models --remote [--json]` queries supported catalogue endpoints | No task body/model auto-selection/config write; bounded complete-result output; catalogue membership does not prove call permissions or tool support |
| Maintain saved configurations offline | Rename/remove named profiles; change an existing profile's key reference; explicitly remove a saved literal | Protect root/saved-active removal; preserve unrelated settings/default selection; key values are never resolved/copied by metadata edits |
| Import reusable credential references | Available supported Codex/WorkBuddy/CodeBuddy/dsh environment-key sources retain their variable name | Selected effective source only; literal/missing-key fallbacks remain explicit; no source-file edits or account/session-token import |
| Save from multiple terminals safely | Stale wizards cannot restore removed keys or overwrite newer configuration | Exact snapshot checks plus a same-path local OS lock; busy/conflict saves fail explicitly without merging; see [scope](../configuration-saves.md) |

Manual edits now use the same effective-key selection as requests, so Enter can
retain an available environment credential on official service hosts. `-` in the
wizard clears only the saved literal. This correction belongs to the existing
credential-reference outcome, not a separately counted release feature. Protocol
variants, tests, packaging work and documentation likewise are supporting evidence.

Import setup also respects zi's existing effective environment key, avoiding
redundant key entry while retaining the source's literal/reference. This shares
the manual setup/request selection rule and safe notice; it belongs to the
credential-reference outcome rather than another counted feature. Its follow-up
requires its own exact-source checks, as with the concurrency follow-up below.

Explicit budget clearing also works when a profile has no model or has an
obsolete generation protocol. It removes only the two preferences; setting new
preferences and running model tasks retain their normal validation. An actual
v0.33.0 executable accepted a ready profile again after root and inactive-profile
budget fields were cleared in an isolated downgrade check.

## One explicit setup-to-task workflow

Create or import a named profile in a real terminal:

```sh
zi config --name work
```

For an existing profile, prepare a key reference and optionally remove its saved
literal fallback. The variable can be supplied separately by the environment that
launches zi; the following edit neither reads its value nor changes that environment.

```sh
zi config --name work --api-key-env WORK_API_KEY --clear-saved-key
zi config --name work --max-output-tokens 512
zi -p work doctor --json
```

The budget command above uses the selected protocol's normal field. Only for a
Chat service documented to need the legacy field, explicitly add
`--chat-token-limit-field max_tokens` to that config edit. A later invocation can
use `--no-profile-budget` without changing saved preferences. Effective key order
remains `ZI_API_KEY` > selected variable (default `OPENAI_API_KEY`) > saved literal.

The following commands explicitly contact the configured provider; run them only
when that is intended. The catalogue uses GET, and the probe allows one small model
turn. Neither command supplies a document/project as input.

```sh
zi -p work models --remote --json
zi -p work doctor --json --probe
```

A provider may support generation but not expose a compatible catalogue. An HTTP
404 is not a reason to delete the saved model or guess another endpoint. Inspect
saved profiles with offline `zi models`; configure a documented model ID explicitly.

Preview a real first task request offline, then use the ordinary pipeline/file
delivery contract for execution:

```sh
zi -p work --preview --no-stream -i notes.txt 'Summarize in three sentences'
cat notes.txt | zi -p work --no-stream 'Summarize in three sentences'
zi -p work --no-stream -i notes.txt -o summary.txt 'Summarize in three sentences'
```

The preview contains the supplied input and request body, without authentication
headers. The second command writes a complete validated answer to stdout. The file
form refuses an existing target unless `--force` is explicitly added, and replaces
it only after success. Ordinary shell `>` can truncate its target before zi starts;
it does not provide that preservation guarantee. Explicit `--stream` can leave
partial stdout on failure and cannot combine with `-o`.

Metadata maintenance remains explicit:

```sh
zi config --rename work personal
zi config --name personal --clear-output-budget
zi -p personal doctor --json
```

An active rename follows the saved selection; external `-p work`/`ZI_PROFILE=work`
references are not rewritten. Removing a profile requires an existing inactive
named target. Root `default` and the saved active profile remain protected; select
another existing usable profile first. Clearing a saved key does not revoke it,
unset an environment variable or remove copies from other profiles/agents.

## Verified evidence for the runtime source

- [Exact five-job CI](https://github.com/ax2/zistral/actions/runs/41966889467):
  Windows390, macOS393 and Linux394 Rust tests; 22 suites per platform, zero ignored.
  Each platform also passed 8 pre-sharing, 7 table-preparation and 40 service-setup
  workflow checks. Rust 1.85 and all 18 release/archive rule tests passed.
- [Exact three-platform candidate installation checks](https://github.com/ax2/zistral/actions/runs/41966889460):
  Windows x64 exercised five actual installation scenarios, each with all 55 workflow
  checks. Linux x64 and macOS arm64 each checked a portable copy plus native install
  and same-version reinstall, including the 55 checks on each copy and configuration
  preservation through uninstall. The Unix checks ran on disposable matching hosts.
- Local full validation: 390 Rust tests / 22 suites / zero ignored, formatting,
  Clippy with warnings denied and optimized build. The actual optimized executable
  passed eight budget tests, six key-maintenance tests and 13 terminal/configuration tests. The service
  workflow uses the same 24 synthetic loopback requests; its new reference must
  authenticate correctly while the old variable remains present with a wrong value.

These are controlled integration/installation results, not a model-quality score
or real-account access result. The Windows local executable is 5,782,016 bytes;
its PE import inspection showed Windows system DLLs with the existing static-CRT
configuration. This is a local development-build observation, not a promised final
download size, proof for every Windows version or a new packaging feature.

## Known service observations and compatibility

The existing configured real service returned HTTP403 for a fixed16 probe, and one
explicit catalogue query returned HTTP404. Those recorded checks left configuration
unchanged; they did not prove inference access, identify whether a proxy/provider
originated the response or establish general model quality. No repeated credential
guessing, proxy bypass or automatic endpoint/field fallback is part of this batch.

Imported references require the selected variable in later launching environments;
already running tasks retain their settings snapshot. Only the chosen available
credential source is retained, not an entire upstream fallback chain. Existing
saved literal profiles are not automatically migrated. Metadata edits need valid
typed TOML and reserialize formatting/comments using the existing atomic writer.

Saved budget fields are new to this batch. Before downgrading to v0.33.0, clear
them from **every** affected profile, including root and inactive profiles, using
this version; an older strict configuration parser can reject them even when a
different profile is selected. CLI options/JSON commands introduced here are also
unavailable in v0.33.0. Existing key-reference fields are not new schema fields.

## Additional configuration-save follow-up (unreleased)

The conflict-protection row is one additional related outcome; snapshot checks,
locking, crash recovery and regressions are parts of that outcome. The historical
verification baseline above predates it and is not evidence for this follow-up.
Its exact source must pass CI and native installation checks before selection.
It adds no schema field or runtime dependency. The installed release remains
v0.33.0.

## Remaining publication work

Keep the current package version until the grouped batch is selected for release.
Normal release reviews occur at most weekly; this preparation is not a publication
trigger or a promise to release on a particular date. No recurring task is created.

At selection, follow the [release checklist](../releasing.md): choose the exact
source/version, remove unreleased wording as appropriate, add only this batch to
the new release notes, run its own complete CI and final native package checks,
and verify the official downloadable assets/digests plus installed behavior.
Published assets/tags remain immutable. Current passing candidate results do not
replace those final-artifact checks.

After actual publication, write one illustrated Feishu article under
`文章/zistral`, with official-version screenshots, runnable examples, principal
changes and any needed configuration/data-flow diagram; a labelled actual recording
is preferred. No development-checkpoint article is required for this document.

Details: [service workflow](../service-setup-verification.md),
[budgets](../output-budgets.md), [diagnostic JSON](../doctor-json.md),
[catalogue](../model-catalogue.md), [profile maintenance](../profile-maintenance.md),
[key metadata](../key-settings.md), [imported references](../import-key-references.md).
