# Changelog

## 0.7.0 — 2026-10-26

- Add seven reusable task presets: summarize, translate, extract, explain, review,
  commit and rewrite. Invoke them with `-s/--preset`; retain separate pipe input,
  explicit model selection and existing project permission boundaries.
- Add offline `zi presets`, `show`, `render` and `init` commands, plus interactive
  `/presets` and `/run NAME [instruction]` shortcuts.
- Support versioned user TOML presets with bounded literal variables, defaults,
  `--var NAME=VALUE` overrides and explicit `--presets FILE` / `ZI_PRESETS` selection.
  Initialization never overwrites an existing file; no plugin code is executed.
- Keep extraction output validated and atomic; reject JSON streaming, malformed
  templates and missing/unknown variables before contacting the model.
- Document the focused task-assistant scope, session/memory assessment and
  zi-devtools design references. Keep tasks independent without implicit memory.
- Verify the preset workflow across all three model protocols and a real terminal;
  preserve lightweight, single-executable Windows/Linux/macOS installations.

## 0.6.0 — 2026-10-23

- Add explicit project mode with `--agent` or `-C/--cwd DIR`: inspect projects,
  plan multi-step work, edit files, execute builds/tests, fix failures and verify again.
- Expand to 14 built-in tools with gitignore-aware listing/literal search, paginated
  reads, exact edits, file creation/deletion, in-task undo, Git inspection and plans.
- Apply scoped AGENTS.md before edits; require reading existing files, reject stale
  changes and escaping paths, preserve CRLF/executable permissions and write atomically.
- Add `--read-only`, `--no-exec`, bounded command output and configurable command
  timeouts with descendant cleanup. Shell commands use the user's OS permissions;
  project mode is not an OS sandbox.
- Keep data-only defaults, clean pipelines, three model protocols and single-binary
  installers. Project tasks default to 32 model requests; data tasks retain 8.
- Test real Rust project failure → edit → passing tests across all three protocols,
  permission enforcement, instructions, edit conflicts/undo, and process timeouts.
- Document the self-implemented agent architecture, public Codex references and limits.

## 0.5.0 — 2026-10-23

- Save multiple independent model profiles with `zi config --name NAME`; list with
  `zi models`, switch with `zi use NAME`, or select once using `--profile` / `-p`.
  Existing single-model settings remain compatible as the `default` profile.
- Stream terminal responses for Chat Completions, Responses and Anthropic Messages,
  including tool calls. Add `--stream` and `--no-stream`; keep default piped output
  and `--json` complete and atomic. Handle truncated streams as failures.
- Add offline `zi doctor` configuration diagnostics and explicit `--probe` model
  connectivity/authentication checks without printing keys, error bodies or answers.
- Add `/models`, `/use NAME`, and `/doctor` interactive shortcuts.
- Keep the single-binary design and existing runtime dependencies; test profile
  isolation, streaming flush/tool loops, terminal sanitization, and pipe compatibility.
- Publish only this version's changes in GitHub Release notes.

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
