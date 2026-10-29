# Release explicitly after reviewing a commit

Regular `main` pushes run CI only. They do not start the Release workflow, even when
Cargo.toml already contains an unpublished version. Commit fixes and documentation as
often as useful, then choose the exact reviewed commit for publication.

## Choose a release milestone

Continue frequent, focused commits and normal CI while developing a batch. The
default release contains 3–5 substantial, related user-facing improvements or
fixes that make a coherent milestone. Count useful outcomes, not changed files,
test cases or individual functions. Do not split one improvement into artificial
items or add unrelated features to reach a number. Assess the whole milestone
before publication; a completed commit or agent session is not a release trigger.

Small performance changes, minor fixes and documentation normally accumulate on
main for the next batch. An urgent defect that prevents normal use, risks data
loss, or needs a security fix can justify a standalone patch. Its release notes
must state the concrete reason. There is no fixed release schedule and no recurring
automation requirement. Avoid multiple minor releases in a day.

Keep the current package version during ordinary development. Only bump it when
the batch is ready for release checks. Before creating a release branch, summarize
the batch's user outcomes, completed validation and any material limits. Release
notes contain this batch only. Write one illustrated Feishu article per actual
release after verifying the official binaries; do not generate articles for every
commit or internal checkpoint.

## Validate and publish the chosen batch

1. Update the package/lock version and add one changelog section for this release. Keep
   historical sections unchanged. Update the manual-dispatch default and installation examples.
2. Run the relevant local checks, the full Rust release checks and the two Python suites:

   ```sh
   cargo fmt --check
   cargo test --locked
   cargo clippy --locked --all-targets -- -D warnings
   cargo build --release --locked
   python scripts/test_release_notes.py
   python scripts/test_release_target.py
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

Packaging-source changes also run the read-only `Installer checks` workflow, before a
Release request. It builds a Windows installer/archive and exercises install twice into
a directory containing both spaces and Chinese characters, then uninstall/PATH/config
preservation. Its short-lived `installer-check-SHA` artifacts are CI candidates, not a
GitHub Release. Require that exact-SHA workflow to pass when it is present, alongside
normal CI. This does not replace the three-platform native checks in Release.

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
