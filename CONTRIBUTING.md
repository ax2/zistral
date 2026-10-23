# Contributing

Keep the pipeline contract stable: only results on stdout, diagnostics on
stderr, explicit errors, UTF-8, bounded memory and requests. Avoid adding a
runtime, daemon, or shell execution to solve simple data transformations.

Before submitting changes, run `cargo fmt --check`, `cargo test --locked`, and
`cargo clippy --locked --all-targets -- -D warnings`. Network protocol changes
need mock-server integration tests; local transformations need deterministic
tests. Never commit API keys or real private inputs. Use small milestone commits.

To release, update Cargo.toml/Cargo.lock and CHANGELOG.md together and push to
main. The release workflow skips already-published versions and publishes new
ones after all builds pass. A `vX.Y.Z` tag or manual dispatch also works. It checks the tag against
the package version, tests/builds on Windows/Linux/macOS, and publishes only
after all assets are ready. A manually created tag can also trigger the workflow
through workflow_dispatch with the same tag. Do not overwrite released tags.
