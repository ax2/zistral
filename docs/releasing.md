# Release explicitly after reviewing a commit

Regular `main` pushes run CI only. They do not start the Release workflow, even when
Cargo.toml already contains an unpublished version. Commit fixes and documentation as
often as useful, then choose the exact reviewed commit for publication.

## Choose a release milestone

Continue frequent, focused commits and normal CI while developing a batch. The
default release normally collects 5–8 substantial, related user-facing improvements
or fixes that complete a coherent use case. This is a planning guide, not a quota.
Count useful outcomes, not changed files, test cases or individual functions. Do not split one improvement into artificial
items or add unrelated features to reach a number. Assess the whole milestone
before publication; a completed commit or agent session is not a release trigger.

Small performance changes, minor fixes and documentation normally accumulate on
main for the next batch. An urgent defect that prevents normal use, risks data
loss, or needs a security fix can justify a standalone patch. Its release notes
must state the concrete reason. Review a normal release candidate at most once a
week; a review is not a publication deadline. If the batch has too little practical
value or validation is incomplete, keep developing rather than publish. Do not use
unrelated features, several directions of one operation, tests or documentation to
inflate the batch. An explicit request for an urgent release may override the normal
review cadence. No recurring automation is required; avoid multiple minor releases
in a day.

Keep the current package version during ordinary development. Only bump it when
the batch is ready for release checks. Before creating a release branch, summarize
the batch's user outcomes, completed validation and any material limits. Release
notes contain this batch only. Write one illustrated Feishu article per actual
release after verifying the official binaries; do not generate articles for every
commit or internal checkpoint.

For historical pre-sharing validation, see the [boundary verification note](pre-sharing-verification.md)
for reproducible checks and the distinction between development measurements and official delivery.

The [v0.33.0 grouped table milestone](releases/v0.33.0-candidate.md) records the
three outcomes and candidate-specific delivery evidence under the same gates.

The [unreleased service-setup batch](releases/service-setup-candidate.md) records
its eight related outcomes, one explicit workflow, exact runtime/native checks,
known real-service observations and remaining publication work. It does not select
a new version or describe CI candidates as a published release.

## Validate and publish the chosen batch

1. Update the package/lock version and add one changelog section for this release. Keep
   historical sections unchanged. Update the manual-dispatch default, installation examples
   and unreleased feature labels/stable-version links.
2. Run the relevant local checks, the full Rust release checks and the Python metadata/installer rules:

   ```sh
   cargo fmt --check
   cargo test --locked
   cargo clippy --locked --all-targets -- -D warnings
   cargo build --release --locked
   python scripts/test_release_notes.py
   python scripts/test_release_target.py
   python scripts/test_unix_installer_rules.py
   python scripts/test_upgrade_baseline.py
   ```

3. Commit/push to `main` and inspect CI for that exact commit. Normal CI includes three
   platforms, Rust 1.85 and release-metadata tests. Do not infer success from another SHA.
4. Explicitly create `release/vVERSION` at the reviewed commit. For example, after CI for
   a commit whose Cargo version is 0.16.2:

   ```sh
   git push origin REVIEWED_COMMIT:refs/heads/release/v0.16.2
   ```

   Replace REVIEWED_COMMIT with the actual SHA. This is an intentional release action,
   not a working branch for further edits. A GitHub API branch creation at the same SHA
   is equivalent. No local branch switch or new checkout is required.

The Release workflow accepts only exact matching `release/vVERSION` branch pushes,
matching `vVERSION` tag pushes, or `workflow_dispatch` with a matching tag input. A branch
named `release/v0.16.20` does not publish Cargo version 0.16.2. `main` push events are
rejected by both the workflow trigger filter and the release-target validator. Pull requests,
schedules and other event types are not publication entry points.

The validator checks the package version from the checked-out commit, not the moving
current main branch. Its local preview can confirm a target without GitHub/network calls:

```sh
python scripts/release_target.py --event push --ref-type branch --ref-name release/v0.16.2
```

It prints `v0.16.2` for the matching package. A rejected event/ref writes no tag output.
When GITHUB_OUTPUT is set by Actions, successful validation also appends `tag=...` there.
The preview does not create a branch, build artifacts or publish anything.

Package version/dependency metadata, packaging-source changes and edits to the
configuration/import readers also run the read-only
`Installer checks` workflow before a Release request. It builds native packages and
portable archives for Windows x64, Linux x64 (Ubuntu 22.04 build host) and macOS arm64,
using the same targets and Unix verifier as Release. Require all three native jobs for
that exact SHA to pass when present, alongside normal CI. These temporary artifacts
are CI candidates, not a GitHub Release; the package version remains unchanged during
ordinary development and can match an already published version.

Windows exercises repeated installation into a path with spaces/Chinese characters,
legacy duplicate PATH repair, user-owned PATH preservation, uninstall and configuration
preservation. The Unix verifier checks the entire portable archive's expected members,
regular-file types, executable mode and binary digest before copying only its binary
to a Unicode/space path. It tests that copy and the native executable after install
and same-version reinstall, including pipe counts and all three delivery workflows.
Uninstall must remove the owned executable and preserve a synthetic model configuration.
It refuses existing executables, package receipts or model settings and is intended
only for clean disposable runners. Python optimization is rejected before installer
commands because verification assertions must stay enabled. No recursive user-directory
cleanup or system package removal outside the known fresh test installation is used.

Candidate checks additionally install the checksum-pinned official v0.33.0 package
before the candidate, verifying both executable identities and byte-for-byte legacy
configuration preservation. Download preparation uses `gh` only on the disposable
test host. The [baseline manifest](../scripts/stable-upgrade-baseline.json) fixes
the public installer size/digest and matching executable digest; updating it requires
review of verified official assets, rather than following a mutable latest release.

Candidate metadata currently still says 0.33.0, so reports explicitly distinguish
published-code replacement from a different package version. Before publishing a
selected version, require actual stable-to-new-version native upgrades and final
official artifact verification; same-version replacements do not establish them.
The isolated configuration is removed only after preservation checks; it is not a credential migration or secure-erasure test. Archive/guard rule tests
run in normal CI without native installation. No new zi runtime dependency is added.
Keep the Windows artifact name `installer-check-SHA`; Unix artifacts add their target
to that name. All candidate artifacts expire after seven days. Release reuses the same
Unix verifier against its final native/portable packages before publication.
The installed executable also runs the [service-setup delivery check](service-setup-verification.md)
against a synthetic loopback server: profile selection, saved budgets, catalogue discovery,
versioned diagnostics/probe, a native-tool followup, explicit streaming and failed-file
preservation in all three protocols (plus legacy Chat). This is not a real provider
permission/quality test; it never reads real keys or user settings. Normal three-platform
CI runs the same delivery check before release.

The pre-sharing check covers structural reports, reusable optional-field masking, pipe/file request
preview, no model connection and preservation of existing files on failure. It uses
synthetic data and isolated configuration, with Python only on the test host; installed
zi itself still requires no Python. It does not replace full Rust tests or real model
acceptance checks. This does not replace the three-platform native checks in Release.

To verify an extracted portable executable without changing your installation:

```sh
python scripts/test_pre_sharing_delivery.py --binary /path/to/zi
```

## What the explicit release action does

The workflow checks the requested version and current release notes, skips already released
versions, and rejects an existing version tag that points to another commit. It then runs
the existing three-platform tests, formatting, Clippy, release builds, minimum Rust check,
native installation checks and packaging. Only successful builds publish six installer/archive
assets plus SHA256SUMS.txt, using only the selected changelog section. A release-branch run
creates the version tag at the same source commit during publication. Existing published
tags/assets are not a way to move a release to newer fixes: use a new patch version.

Normal CI success does not itself enforce this sequence through branch protection. A
maintainer can still deliberately create a release entry before CI finishes; the Release
workflow independently repeats checks before publishing. Keep a release branch fixed while it is building or publishing. The failed-before-publication
recovery below is the only exception. Concurrent release requests share the existing release concurrency group;
avoid launching a different version while another is publishing. No repository security or
credential settings are changed by this workflow update.

After publication, download and verify official assets, test installed executables, preserve
existing user configuration, and publish the illustrated development article in Feishu
文章/zistral. Read back its media, folder membership and series navigation before starting
the next release milestone. Development commits do not require a version bump. No recurring automation or background service is needed.

GitHub references: [branch/tag trigger filters](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpushbranchestagsbranches-ignoretags-ignore)
and [workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow).

## Recover an unpublished failed candidate

A failed build/test attempt does not publish a release. Preserve its run, failing SHA and
diagnosis, fix the problem on main, and repeat local checks plus all exact-SHA CI gates.
Do not move a published tag or replace published assets. An already successful/released
version needs a new version instead.

If both the version Release and version tag are still absent, a maintainer may explicitly
fast-forward the existing release/vVERSION branch to the corrected reviewed commit, using
an expected-old-SHA lease and no force. First verify the old attempt is terminal, save a
fresh successful CI/zero-Release snapshot for the corrected SHA, and verify absence of both
published objects. This deliberate branch update starts a new Release run; it is not a
retry of different code under an old run. Keep the failed attempt in delivery evidence.

The successful branch, main, local checkout and eventual version tag must all point to
the corrected commit. A branch mismatch, a nonterminal attempt, an existing tag/release,
or a failed corrected CI gate requires inspection before any publication action.
