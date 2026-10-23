# Changelog

## 0.4.0 — 2026-10-23

- Add a terminal welcome panel, model details, waiting animation, request and tool
  activity, elapsed time, token usage, and highlighted headings/lists/code blocks.
- Start an interactive prompt with bare `zi`; support `/help`, `/config`, `/clear`
  and `/exit`. Each prompt starts an independent task with no saved history.
- Automatically open model import/setup on the first interactive launch.
- Preserve plain pipe/redirection output; add `--plain`, respect `NO_COLOR`, and
  remove provider-supplied terminal control sequences from rich rendered results.
- Ship Windows per-user setup (.exe), Debian/Ubuntu (.deb) and macOS ARM64 (.pkg)
  installers alongside portable archives. No language runtimes are required.
- Automatically register the Windows command and Start menu launcher, preserve
  configuration on upgrade/uninstall, and test native installation on all platforms.
- Add real-terminal first-run, tool/progress, token, independent-task and plain-mode tests.

## 0.3.0 — 2026-10-23

- Automatically discover and import Codex, WorkBuddy/CodeBuddy, and dsh model
  settings from `zi config`; ask for a hidden key only when no API key is found.
- Add `zi config --list` for redacted discovery and `--manual` for direct editing.
- Resolve Codex profiles, WorkBuddy full/custom URLs, dsh profile patches and
  provider credential references while leaving all source files untouched.
- Identify account/subscription authentication without copying its login tokens
  or executing credential commands. Malformed/unsupported sources are explained.
- Add Anthropic Messages support for imported dsh providers, including tool
  round trips, exact endpoints, and explicit truncated-answer errors.
- Test each import path in real terminals and all three API protocols with mocks.

## 0.2.0 — 2026-10-23

- Add `zi config`, an interactive setup wizard for service URL, model, API
  protocol, and hidden API key entry. Saved settings work without environment variables.
- Re-run setup to keep or change existing values, or clear a saved key with `-`.
- Validate inputs, save atomically, preserve the existing file on cancellation,
  and write private config permissions on Unix.
- Keep environment variables as overrides and avoid leaking keys in config errors.
- Add real-terminal configuration tests on Windows, Linux, and macOS.

## 0.1.0 — 2026-10-23

First release of `zi`, the Zistral command-line agent.

- UTF-8 pipe and file input with clean stdout/stderr separation.
- Configurable Chat Completions and Responses API backends.
- Bounded agent loop with exact text-counting and JSON-selection tools.
- Offline `--count`, `--select`, and `--join` operations.
- Environment/TOML/flag configuration, validated JSON output, and timeouts.
- Windows x64, Linux x64, and macOS Apple Silicon release archives/checksums.

No streaming, shell execution, web browsing, persistent sessions, or automatic
chunking in this version. Model behavior depends on the configured provider.
