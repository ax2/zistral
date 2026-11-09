# Unreleased grouped candidate: model-service setup and reliability

This is the review of **eight related outcomes** on unreleased `main`; it does
not select a version or publish a release. Stable downloads remain
[v0.33.0](https://github.com/ax2/zistral/releases/tag/v0.33.0), which does not include
this batch. Development package metadata also says `0.33.0`; a CI candidate is
different from the published assets even when that version string is identical.

The verified source for this review is
[`0850ca6`](https://github.com/ax2/zistral/commit/0850ca61f47be07a330de609dbf1f984f9d05e5c).
Its application source is unchanged from `c736fca`; the follow-up adds pinned
published-installer verification. Later runtime, packaging or test changes need
their own evidence. This document replaces the earlier incremental candidate
notes; the original validation section refers to this exact baseline.
Subsequent changes are recorded separately below rather than folded into its counts.

## One complete use case

Configure a provider once, understand which model and credential will be used,
make an explicit small service check, and run a task with predictable request
preferences and pipe/file behavior. The batch fits zi's
[text/data/focused-task position](../product-scope.md): no additional agent tools,
implicit history, session memory, model weights, daemon or runtime dependency.
Existing offline data/recipe workflows remain independent of model services.

| User outcome | Delivered behavior | Boundary and details |
| --- | --- | --- |
| Trust setup and diagnosis | Show the winning key source; validate defaults before display and escape control-bearing human metadata | Invalid defaults are hidden and require a valid replacement. An explicit probe is one non-streaming, tool-free turn, fixed 16-token requested cap and at most 15 seconds. [Workflow](../service-setup-verification.md) |
| Limit a task's requested model output | Explicit per-turn cap across Chat, Responses and Anthropic, including streams, tool followups and retries | Legacy Chat field is explicit; no field-name guessing/retry. This is not a total cost guarantee. [Budgets](../output-budgets.md) |
| Reuse preferences while switching profiles | Save, override, temporarily ignore or clear profile-local request budgets | CLI overrides are invocation-only. Clearing works without a model or supported generation protocol; setting/running remains validated. [Budgets](../output-budgets.md) |
| Diagnose in scripts | `doctor --json` returns one bounded version-1 report and meaningful exit code | Offline by default; explicit probe only. Safe typed errors, no keys or raw provider bodies. [JSON contract](../doctor-json.md) |
| Discover a service's model IDs | Explicit `models --remote [--json]` | Bounded same-origin catalogue GETs; no task body, selection or config write. Plain `models` and `/models` stay offline. [Catalogue](../model-catalogue.md) |
| Maintain saved profiles and keys offline | Rename/remove named profiles, change a key-variable reference, explicitly clear a saved literal | Protect root/saved-active removal; preserve unrelated settings/default selection. Key metadata edits do not read/copy variable values. [Profiles](../profile-maintenance.md), [keys](../key-settings.md) |
| Import reusable credential references | Supported Codex, WorkBuddy/CodeBuddy, dsh and personal Claude Code API imports retain an available selected variable name | The wizard uses zi’s effective credential precedence and leaves source files untouched. Claude Code uses ordinary Anthropic Messages authentication. [Import rules](../import-key-references.md), [Claude Code scope](../claude-code-import.md) |
| Save safely from multiple terminals | Stale wizards cannot overwrite newer settings or restore a removed key | Exact snapshots plus a local OS lock; busy/conflict saves fail without merging. Restart to reload. [Save conflicts and scope](../configuration-saves.md) |

Prompt/display corrections, protocol variants, regressions, packaging and this
documentation support those eight outcomes; they are not extra counted features.

For step-by-step Chinese instructions, see the [usage guide](../service-setup.zh-CN.md).
It distinguishes offline diagnosis/preview from explicit provider calls and shows
exit-status-aware PowerShell handling; it is not an installation guide for stable 0.33.0.

## Configure, inspect, run

Use a real terminal and choose a detected source or manual provider settings:

```sh
zi config --name work
zi config --name work --max-output-tokens 512
zi -p work doctor --json
```

Setup saves a profile; it does not test model access. Imported environment
references require their variable in the shell that later launches zi. Effective
key order is `ZI_API_KEY` > selected variable (default `OPENAI_API_KEY`) > saved
literal. Ordinary Chat uses `max_completion_tokens`; only for a documented legacy
service add `--chat-token-limit-field max_tokens` to the budget edit.
`--no-profile-budget` temporarily ignores saved preferences without changing them.

These two commands explicitly contact the configured provider, without supplying
a document/project as input:

```sh
zi -p work models --remote --json
zi -p work doctor --json --probe
```

A catalogue can be absent even when generation works. Membership does not prove
call permission or tool support; a tiny probe does not establish general quality.
Do not delete a saved model or guess an endpoint because a catalogue returned 404.

Preview the first request offline, then execute with explicit input/output:

```sh
zi -p work --preview --no-stream -i notes.txt 'Summarize in three sentences'
cat notes.txt | zi -p work --no-stream 'Summarize in three sentences'
zi -p work --no-stream -i notes.txt -o summary.txt 'Summarize briefly'
cat notes.txt | zi -p work --stream --max-output-tokens 256 'Summarize briefly'
```

Preview includes the supplied input/body, without credential headers. Non-streaming
stdout is emitted after completion validation. `-o` refuses an existing file unless
`--force` is explicit and replaces it only after success. Shell `>` has ordinary
shell truncation behavior. Explicit streaming may leave partial stdout on error
and cannot combine with `-o`.

Optional maintenance remains offline and explicit:

```sh
zi config --name work --api-key-env WORK_API_KEY --clear-saved-key
zi config --rename work personal
zi config --name personal --clear-output-budget
```

The first edit stores only a name and removes the literal fallback. Set
`WORK_API_KEY` separately in the launching environment before the next model task;
the edit is allowed while the variable is absent. Clearing a saved key neither
revokes it nor unsets variables or removes copies elsewhere. Rename follows a
saved active selection, but external `-p work`/`ZI_PROFILE=work` references need an
explicit update. Remove only an existing inactive named profile.

A configuration save leaves a reusable empty sibling `.lock` file, containing no
key, snapshot, PID or expiry. Process exit releases its OS lock. This coordinates
cooperating writers on the same local path; older tools/editors ignoring the lock,
path aliases and distributed filesystems are outside the universal guarantee.

## Exact validation baseline

[Five-job CI](https://github.com/ax2/zistral/actions/runs/41983234535) passed for
`0850ca6`, including formatting, Clippy with warnings denied and Rust 1.85:

| Platform | Rust tests | Suites | Ignored | Built-executable workflow checks |
| --- | ---: | ---: | ---: | --- |
| Windows | 406 | 23 | 0 | 8 pre-sharing + 7 table + 40 service setup |
| macOS | 409 | 23 | 0 | 8 + 7 + 40 |
| Linux | 410 | 23 | 0 | 8 + 7 + 40 |

The metadata job passed 27 Python rules: release notes 4, release targeting 5,
Unix archives/guards 10 and pinned-baseline preparation 8. Python is a test-host
dependency, not an installed zi requirement.

[Three native installer jobs](https://github.com/ax2/zistral/actions/runs/41983234537)
also passed for the same SHA:

- Windows x64 installed the checksum-pinned official v0.33.0 package first, then
  exercised five candidate installations with all 55 workflow checks each, including
  Unicode/space paths, duplicate PATH repair and preservation of user-owned entries.
- Linux x64 and macOS arm64 each checked a portable candidate copy, installed the
  pinned official package, then installed/reinstalled the native candidate with
  all 55 checks on each candidate copy.
- All three required the actual official executable's digest/version and the
  candidate's installed bytes to match their expected sources. Both executables
  loaded the stable-compatible multi-profile fixture; its complete bytes survived
  replacement/reinstallation/uninstall. Cleanup refused unexpected state.

The reviewed public package/executable pins are in
[`stable-upgrade-baseline.json`](../../scripts/stable-upgrade-baseline.json).
The baseline is selected explicitly, with no mutable latest-release lookup.
Reports state **`package_version_changed: false`** because both packages currently
say 0.33.0. This proves published-code to candidate-code replacement, not a
package-manager transition across different version numbers or final artifacts.

Local application validation at `c736fca` passed 406 Rust tests, formatting,
Clippy and an optimized build. The actual optimized executable passed 18 terminal,
4 configuration-transaction and 16 general-feature tests, plus the same 55
workflow checks. The application/build inputs are unchanged at `0850ca6`; its
own exact CI/native results above additionally validate the delivery follow-up.

The service helper makes 24 synthetic loopback requests across modern Chat,
explicit legacy Chat, Responses and Anthropic. It checks actual auth/budget fields,
preview/request equality, native-tool followups, stream output and failed-file
preservation. Real-terminal tests additionally exercise effective-key imports,
override removal and stale-wizard conflicts. These are controlled checks, not
real-account access or model-quality scores.

## Verified follow-ups (2026-11-06)

The original `0850ca6` baseline and its counts above remain unchanged. These later
commits have their own exact-source CI and native-installation evidence:

| Source | Refinement within the same eight outcomes | Exact validation | Rust tests: Windows / macOS / Linux |
| --- | --- | --- | --- |
| [`4b4f2a9`](https://github.com/ax2/zistral/commit/4b4f2a93195bc3a96bca1e9621c5ac3b083801ba) | Coordinate preset-library imports across destination reading, merging and publication; avoid successful concurrent imports losing a definition | [CI](https://github.com/ax2/zistral/actions/runs/41991451363), [native](https://github.com/ax2/zistral/actions/runs/41991451411) | 409 / 412 / 413 |
| [`88c970d`](https://github.com/ax2/zistral/commit/88c970d0ef3e7e09c89ac33585d9c418554d7437) | Fixed typed configuration error codes/advice in diagnostic JSON, preserving existing human errors and IO cause chains | [CI](https://github.com/ax2/zistral/actions/runs/41998926373), [native](https://github.com/ax2/zistral/actions/runs/41998926189) | 412 / 415 / 416 |
| [`88d1e77`](https://github.com/ax2/zistral/commit/88d1e7764691bbd73aadf7bfde31ad6583307824) | Preserve configuration-file symbolic links; reject saves before prompts and at publication, retaining read-only support | [CI](https://github.com/ax2/zistral/actions/runs/42141374032), [native](https://github.com/ax2/zistral/actions/runs/42141373779) | 413 / 419 / 420 |

Each row passed five CI jobs, 23 Rust suites per platform with zero ignored tests,
the same 8 + 7 + 40 executable workflow checks and 27 metadata rules. Its three
native jobs installed the pinned official 0.33.0 baseline and replaced/reinstalled
with the candidate; Windows retained five candidate installation scenarios, and
each Unix platform checked one portable copy plus two native installations.
Every row still reports `package_version_changed: false`; none is a new release.

At `88d1e77`, Unix portable/native checks additionally read linked settings through
three commands and reject seven write variants on each of three copies per
platform, preserving both the link and target bytes without save artifacts.
Terminal regressions cover manual/import startup, dangling links and an
identical-byte link introduced while the wizard is open. Windows local file-link
creation was unavailable (OS privilege 1314); that local capability failure is not
claimed as a passed Windows symbolic-link exercise. The shared non-regular-file
guard and ordinary Windows regression/installation checks passed.

The Chinese guide's noninteractive examples were replayed against the actual
optimized Windows executable from `88d1e77`: 28 checks, seven synthetic loopback
requests, zero real-provider requests. They cover diagnostic status/version
handling in PowerShell, modern/legacy saved caps, explicit catalogue/probe,
preview/request equality, pipe/file/stream output, failed-file preservation and
offline maintenance. Manual/import prompts use the separate terminal regressions;
the guide replay is not additional feature count, provider access or final-asset
evidence. Its fixtures and keys are synthetic and temporary.

## Current candidate evidence (2026-11-09)

[`206825f`](https://github.com/ax2/zistral/commit/206825f189810f79b962962de9968a03aa7ce12c)
adds personal Claude Code API settings to the existing import outcome. Terminal
checks cover a literal key, an environment reference with rotation, missing-key
entry, and source-file preservation. The rotation suite exercises eight source
forms and 16 controlled requests. The Chinese guide also passed its 28 checks
against the optimized Windows build, using seven synthetic loopback requests.
The guide replay and terminal suite make no real provider requests.
[CI](https://github.com/ax2/zistral/actions/runs/42551328623) ·
[Candidate installs](https://github.com/ax2/zistral/actions/runs/42551328604)

[`6bf5bd1`](https://github.com/ax2/zistral/commit/6bf5bd1172ec83710c0e793d9859f48969f895d9)
adds a gate to the formal Release workflow. Each platform must install the
checksum-pinned v0.33.0 package, then install the newer release build and verify
the executable and complete legacy configuration before artifact upload.
The exact commit passed 418 Windows, 424 macOS and 425 Linux Rust tests,
35 metadata rules and three candidate installer jobs. Those candidate jobs still
report `package_version_changed: false`. The formal cross-version gate runs after
version selection. [CI](https://github.com/ax2/zistral/actions/runs/42552081709) ·
[Candidate installs](https://github.com/ax2/zistral/actions/runs/42552081657)

## Compatibility and remaining publication work

Recorded account checks earlier in development returned HTTP403 for a fixed-16
probe and HTTP404 for one catalogue query (the latter on 2026-11-05). Configuration
was unchanged. Those observations do not locate the responsible provider/proxy,
prove current inference access or justify credential guessing/proxy bypass/retries.
No additional real-provider request was made for this review.

Budget fields are new to this batch. Before downgrading, clear them from **every**
affected profile, including root and inactive/unconfigured profiles, using this
candidate; v0.33.0's strict parser can reject the whole document otherwise:

```sh
zi config --name default --clear-output-budget
zi config --name personal --clear-output-budget
```

Repeat for every other affected named profile. Clearing requires valid typed
TOML and also works for a profile without a model or with an obsolete protocol.
Config writes reserialize the document, so layout and comments can change.

A Windows and Linux rollback rehearsal used the official v0.33.0 executable and
candidate [`f5e330f`](https://github.com/ax2/zistral/commit/f5e330f235f8a0770d7bc438641b5c6ec646c216).
The stable binary rejected the document until budgets were cleared from all four
fixture profiles, including an inactive profile and an unconfigured profile.
After clearing, the parsed TOML matched the legacy settings. The stable binary
listed the same models and used the same exact endpoint and environment key in
controlled loopback requests. This exercised configuration compatibility on
Windows and Linux, with no real provider calls.

Existing key-reference fields are unchanged. An older zi does not honor the new
coordination lock. Runtime settings remain snapshots per task.

Keep package metadata unchanged until a grouped release is selected. Normal
reviews are at most weekly; completion of this review or a commit is not a
publication trigger. This document creates no recurring automation.

At selection, follow the [release checklist](../releasing.md): choose the exact
source/version, update unreleased labels/stable links, add only this batch to the
new notes, pass its own complete CI,
verify genuine stable-to-new-version native upgrades and official downloadable
assets/digests/installed behavior. Passing same-version candidates do not replace
those gates. Published assets and tags remain immutable.

After actual publication, write one illustrated Feishu article in `文章/zistral`
using verified official-version screenshots, runnable examples, principal changes
and any needed configuration/data-flow diagram; a labelled actual recording is
preferred. There is no development-checkpoint article for this review.
