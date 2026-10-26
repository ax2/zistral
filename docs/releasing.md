# Release explicitly after reviewing a commit

Regular `main` pushes run CI only. They do not start the Release workflow, even when
Cargo.toml already contains an unpublished version. Commit fixes and documentation as
often as useful, then choose the exact reviewed commit for publication.

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
workflow independently repeats checks before publishing. Keep a release branch fixed after
creating it. Concurrent release requests share the existing release concurrency group;
avoid launching a different version while another is publishing. No repository security or
credential settings are changed by this workflow update.

After publication, download and verify official assets, test installed executables, preserve
existing user configuration, and publish the illustrated development article in Feishu
文章/zistral. Read back its media, folder membership and series navigation before starting
the next version. No recurring automation or background service is needed.

GitHub references: [branch/tag trigger filters](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpushbranchestagsbranches-ignoretags-ignore)
and [workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow).
