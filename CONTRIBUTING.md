# Contributing

Keep the pipeline contract stable: only results on stdout, diagnostics on
stderr, explicit errors, UTF-8, bounded memory and requests. Avoid adding a
runtime, daemon, or shell execution to solve simple data transformations.

Before submitting changes, run `cargo fmt --check`, `cargo test --locked`, and
`cargo clippy --locked --all-targets -- -D warnings`. Network protocol changes
need mock-server integration tests; local transformations need deterministic
tests. Never commit API keys or real private inputs. Use small milestone commits.

Keep commits small and frequent, but collect related improvements into a complete
release milestone, normally 5–8 substantial related user-facing changes that complete
a use case. The count is a planning guide, not a quota; small fixes, performance tuning
and documentation normally wait for that batch. Review normal release candidates at
most once a week, and keep accumulating work when value or validation is insufficient.
Do not split a change artificially or add unrelated features to meet a count.
Only urgent issues affecting normal use, data safety or security justify a standalone
patch; record the concrete reason in its release notes.

Normal pushes to main run CI only. When the batch is complete, follow
[the release checklist](docs/releasing.md) to choose and verify an exact commit,
then explicitly create its matching release branch. Do not bump the version or
start a release merely because a commit or development session finished. Published
tags and assets are immutable. Release notes describe only the current batch.
