# Changelog

## 0.14.0 — 2026-10-26

- Let the existing `transform_input` model tool accept an ordered `steps` array
  of 1-16 native transformations, so compound cleanup and JSONL tasks can run
  deterministically in one tool call. Keep the single-operation call compatible.
- Reuse the offline pipeline core: validate the whole plan first, preserve JSON
  lexical values, buffer only current/next stages, and return only the final output.
  Each new invocation still starts from original input, without shared tool memory.
- Bound tool arguments to 64 KiB, original input to 8 MiB, intermediate text to
  16 MiB and the final serialized tool result to 64 KiB. Reject mixed call shapes,
  unknown fields, invalid options and over-limit results rather than truncating.
- Verify composition order, all three model protocols, failure/recovery, large
  intermediate reduction, final-envelope limits and independent subsequent calls.
  No new operation names, dependencies, tools, project permissions or model runtime.

## 0.13.0 — 2026-10-26

- Fix JSON selection/join rounding oversized integers, rejecting large exponents
  in unrelated fields, and rewriting numeric tokens or object key order. Borrow
  selected JSON text instead of constructing a numeric value tree; keep strings
  as decoded text and compact other values without changing their lexical tokens.
- Apply the same syntax-check/lexical-compaction path to `--json` final answers,
  including interactive tasks, so validation cannot reintroduce numeric rounding.
- Enforce plain RFC 6901 escapes and array indices; reject duplicate member names
  on the referenced path instead of silently selecting the last value. Preserve
  repeated keys inside selected containers and leave unreferenced duplicates alone.
- Bound pointers to 4096 UTF-8 bytes/128 tokens, JSON depth to 128, selection input
  and local result text to 64 MiB, and select_input tool text to 64 KiB. Fail joins
  on output growth rather than allocating an unbounded joined result.
- Verify exact lexical results, Unicode/escape rules, strict-JSON output, all three
  protocols, real interactive terminals and file preservation. Enable existing
  serde_json raw-value features without adding packages, model tools or permissions.

## 0.12.0 — 2026-10-26

- Add explicit `--retries 0..3` per model turn, defaulting to zero. Retry only HTTP
  429/500/502/503/504/529 before accepting a successful response; preserve the same
  request body and do not spend another logical `--max-steps` turn.
- Honor integer-seconds and HTTP-date `Retry-After` values with a 30-second wait cap;
  reject invalid/duplicate/longer hints without retrying early. Without a hint, use
  bounded 250/500/1000 ms backoff. Use the small compiled-in httpdate parser.
- Never replay transport failures, timeouts, accepted-response parsing/stream errors,
  or local tool execution. Keep offline/management commands outside retry mode and
  diagnostics single-request. Preserve complete file output on exhausted failures.
- Show retry status/count/wait in rich terminals or verbose stderr, report exhausted
  attempts, and count actual HTTP requests in terminal summaries separately from turns.
- Test all three protocols, exact request reuse, budgets, wait timing, stream failure,
  redaction, file preservation, real-terminal output and one-time project commands.

## 0.11.0 — 2026-10-26

- Add offline `zi data jsonl-to-array` and `array-to-jsonl` for structured logs and
  root JSON arrays. Support pipes, input/output files, ordered steps and TOML recipes.
- Preserve original numeric/string tokens, item order, object key order and repeated
  keys, including oversized integers, exponents and negative zero. Correctly handle
  nested values, escaped delimiters, CRLF and missing final line terminators.
- Reject blank/invalid JSONL records with one-based record numbers, nonarray roots,
  more than 100000 records and nesting beyond 128 containers including the added
  collection wrapper. Empty JSONL becomes `[]`; an empty array becomes zero bytes.
- Extend the existing `transform_input` operation schema across all three protocols;
  keep original-input semantics, 64 KiB tool envelopes and existing project permissions.
  Add no model tool names, runtime dependencies, implicit memory or file discovery.
- Verify lexical round trips, record/depth/output limits, actual pipe composition,
  offline recipes and failure preservation. Include a reusable record-deduplication
  recipe and document full buffering and CLI initial-BOM compatibility explicitly.

## 0.10.0 — 2026-10-26

- Compose existing offline operations with ordered, repeated `zi data --step`,
  including `sort:reverse`, without launching multiple processes or model requests.
- Load explicit versioned TOML recipes with `zi data --recipe FILE`. Validate the
  complete definition before reading input or preparing output; reject unsupported
  operations, unknown fields, invalid reverse flags and incompatible CLI options.
- Buffer each stage and publish only the final result. Later-stage failures leave
  stdout empty and preserve existing `-o` files; keep UTF-8, empty-output and
  same-input/output behavior compatible with existing commands and pipes.
- Limit recipes to 64 KiB and 1–16 native steps, initial input to 8 MiB, and each
  intermediate/final result to 16 MiB. Preserve JSON numeric tokens, key order and
  repeated keys through composed formatting; support bounded expansion/reduction.
- Add cross-platform subprocess/pipe tests and a reusable recipe example. No shell,
  recursive includes, implicit discovery, model tools or runtime dependencies added.

## 0.9.0 — 2026-10-26

- Add `-o/--output FILE` for complete model results, offline data operations,
  count/select and configuration examples; keep stdout empty when saving a file.
  `--output -` retains normal stdout and streaming behavior.
- Default to no overwrite; require explicit `--force` for an existing regular file.
  Stage the complete UTF-8 result beside its destination, flush and publish it only
  after successful processing/JSON validation. Support reading and replacing the
  same input file without truncating it before the task succeeds.
- Check destinations before model requests; refuse observed target changes, links,
  directories, read-only files, Windows devices/alternate streams and files above
  64 MiB. Keep temporary files private on Unix and preserve basic replacement modes.
- Disable automatic token streaming for file output, reject explicit `--stream`,
  and distinguish "Response ready" from the final "Saved" terminal status.
- Verify all three protocols, real-terminal saves, stdout/streaming compatibility,
  concurrent changes, failed requests/JSON, cleanup and platform-specific file rules.
  No new model tools or runtime dependencies; document metadata and concurrency limits.

## 0.8.1 — 2026-10-26

- Display the native `transform_input` tool as "Transform input" in rich terminal
  activity instead of the misleading "Unknown tool" fallback.
- Add a real-terminal regression that reads a fixture file, completes the two-request
  native transformation loop and verifies the tool label and final output. Confirm the
  regression fails against the published v0.8.0 executable before applying the fix.
- Keep offline computation, model protocols, permissions, pipeline output and runtime
  dependencies unchanged; publish complete installers for Windows, Linux and macOS.

## 0.8.0 — 2026-10-26

- Add offline `zi data` operations: trim Unicode whitespace per line, keep first
  occurrences of exact lines, sort lines, pretty-print/minify JSON and list root keys.
  Support pipe chaining, `-i/--input`, stdin `-`, BOM handling and clean empty output.
- Preserve original JSON number/string tokens, key order and repeated keys during
  formatting; validate syntax without converting numeric values or building a JSON tree.
- Expose the same pure computation as `transform_input` in all three model protocols,
  without project permissions, shell access or additional runtime dependencies.
- Bound transformations to 8 MiB input, 100000 lines/root keys and 128 JSON containers.
  Buffer local output up to 16 MiB and model-tool results up to 64 KiB; fail without
  emitting truncated results. Document LF, case-sensitive ordering and duplicate semantics.
- Test real pipelines, Unicode/CRLF, numeric precision, escaped strings, duplicate keys,
  indentation growth, malformed input, strict tool arguments and protocol roundtrips.
- Allow the original CLI compatibility suite to target the official installed executable
  during release verification, alongside the newer protocol and terminal suites.

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
