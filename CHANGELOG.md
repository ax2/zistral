# Changelog

## 0.27.0 — 2026-10-27

- Add explicit `--exact` model data mode: a model chooses one native count/select/transform
  call, and zi returns its successful result directly without a final model rewrite.
  Compound transformations use ordered steps inside one call; native errors can be corrected
  within the existing turn budget. Plain model text, multiple calls and nondata tools fail.
- Keep exact results buffered and use existing pipe/file/newline/JSON validation protections.
  Empty successful results emit/save zero bytes, including interactive `/save`. Terminal
  display retains control cleanup without rich answer reformatting; activity stays on stderr.
- Reject streaming, disabled tools, project selectors, offline and management modes before IO.
  Preserve normal Agent behavior, native limits and independent tasks; no new dependencies,
  tool names, permissions or history. Add all-protocol direct-result/recovery/empty-output tests.

## 0.26.0 — 2026-10-27

- Add offline `lines-head --lines N` and `lines-tail --lines N` (`-n N`) for exact first/last
  line windows. Require 1-100000 lines; preserve order, whitespace, duplicates and blank
  lines, using existing LF output rules. Empty input succeeds without output.
- Share line windows across ordered `--step` pipelines, version-1 recipes, plan export and
  the existing transform_input tool. Compose filtering then tail selection for recent log
  evidence; all original input bytes and discarded lines still count toward limits.
- Reject missing, invalid or misplaced counts before input/output access. Keep complete-result
  publication and file protection, original-input tool isolation, dependencies, tool names,
  permissions, Agent loop, streaming and independent-task defaults.

## 0.25.2 — 2026-10-27

- Preserve valid JSON string values and keys in direct terminal output: render literal
  DEL/C1 characters as JSON Unicode escapes instead of deleting them as plain-text controls.
  Keep numeric spellings, duplicate keys, existing escape tokens and ordinary Unicode intact.
- Keep validated compact JSON byte-exact under existing rules in pipes, explicit files and
  interactive `/save`; terminal escaping affects only model JSON display. No new dependencies,
  model tools, requests, permissions, streaming changes or task history.
- Add host-independent all-DEL/C1 value/token regression coverage and all-protocol terminal,
  saved-result and raw pipe/file tests. Document the distinction between JSON escaping and
  ordinary model-text terminal cleanup.

## 0.25.1 — 2026-10-27

- Sanitize complete model answers sent directly to a terminal even when rich display is
  disabled by `--plain`, `TERM=dumb` or unavailable stderr terminal output. Reuse the
  existing terminal-control cleaner; text, line breaks and tabs remain readable.
- Preserve raw model text and existing newline rules in pipes, redirected output,
  explicit result files and interactive `/save`. Stored results are not rewritten for display.
- Add all-protocol plain/dumb-terminal and exact pipe/file-byte regression tests. Keep
  streaming, offline data operations, model requests, tools, permissions and dependencies unchanged.

## 0.25.0 — 2026-10-27

- Add interactive `/save PATH` for the latest completed answer and `/save! PATH` for
  explicit replacement, reusing existing protected file publication. Paths are literal;
  no shell parsing, automatic files or extra model requests.
- Retain at most one 16 MiB result in process memory, including complete streamed answers
  and validated compact JSON. Clear it before each new task, on /clear, /config and successful
  /use; failed, partial or invalid-JSON tasks cannot export an older result accidentally.
- Preserve independent model tasks and pipe behavior: retained results are never sent as
  conversation context. No new dependencies, model tools, permissions, Agent-loop changes
  or persistent history. Add real-terminal coverage for all three protocols and export rules.
- Match visible terminal prompts across interleaved Windows ConPTY cursor controls while
  preserving raw transcripts for security assertions; document failed unpublished candidate recovery.

## 0.24.0 — 2026-10-27

- Add offline `lines-include --contains TEXT` and `lines-exclude --contains TEXT` for
  case-sensitive literal line filtering. Keep order, duplicates and whitespace; no regex,
  glob, field predicate or Unicode normalization. No matches succeeds with empty output.
- Share strict `contains` definitions across single operations, `--step`, version-1 recipes,
  plan export and transform_input. Compose reusable log filtering with cleanup or model tasks.
- Validate all options before input, count discarded lines against existing limits, and retain
  complete-result/file protections and original-input tool isolation. No new dependencies,
  tool names, permissions, Agent-loop changes or task memory.

## 0.23.0 — 2026-10-27

- Add bundled `builtin:triage` for evidence-led log analysis and `builtin:release-notes`
  for audience-specific notes from an explicit change range. Inspect, preview and export
  these instructions through the existing preset commands.
- Add explicit `builtin:NAME` references for the original seven scenarios too, preserving
  JSON rules and original name protection. New qualified names do not reserve existing
  custom `triage`/`release-notes` names or fall back to user definitions.
- Keep user-library validation, literal defaults, model input separation and capability/file
  boundaries. Add scenario and compatibility guidance; no new dependencies, tools or memory.

## 0.22.0 — 2026-10-27

- Add offline `jsonl-project --field NAME=POINTER` to select/rename 1-64 values from each
  JSONL record into an ordered object. Keep original numeric/string tokens and selected
  subtree key order/repeated keys, with explicit pointers and literal unique output names.
- Share typed field definitions across version-1 recipes, plan export and transform_input.
  Compose projection with CSV output; add a nested-record-to-CSV example. Reject missing,
  ambiguous or invalid references, unsupported options and oversized definitions/results.
- Preserve fail-before-input argument validation and final-success-only stdout/file delivery,
  including nested-output depth limits. No new dependencies, tool names, permissions or memory.

## 0.21.0 — 2026-10-27

- Add offline `jsonl-to-csv` for nonempty flat scalar JSONL objects. The first object
  defines column order; later objects may reorder keys but must have the same unique columns.
- Decode strings, preserve exact numeric tokens and export booleans/null as text. Quote all
  fields, double embedded quotes and use LF records with no BOM. Reject nested values,
  duplicate/empty columns, shape changes, invalid records and existing resource-limit violations.
- Share CSV output with ordered steps, version-1 recipes and transform_input in every provider
  protocol. Add an API-array-to-CSV recipe and document CSV typing/spreadsheet boundaries.
  Keep complete-result/file protections, with no new packages, tool names, permissions or memory.

## 0.20.0 — 2026-10-27

- Add offline `csv-to-jsonl` for explicit comma CSV with a required unique nonempty header.
  Preserve header/record order and every cell as a JSON string, including leading zeros,
  numeric text, quoted commas, doubled quotes, whitespace and multiline contents.
- Reject malformed quoting, blank records, duplicate/empty headers and inconsistent row
  widths with value-free diagnostics; retain complete-result output and file protections.
- Share CSV import with ordered steps, version-1 recipes and the existing transform_input
  tool across all provider protocols. Add a CSV-to-unique-ID recipe and documented bounds.
  Bundle csv/csv-core inside the executable; no script runtime, new tool names, rights or memory.

## 0.19.0 — 2026-10-27

- Add offline `presets export NAME --name CUSTOM_NAME` to copy one builtin/custom task
  preset into reusable version-1 TOML, retaining its template, description and JSON rules.
- Save explicit `--var` overrides as literal defaults while keeping placeholders and unresolved
  required variables. Reuse existing template validation, preset loading and per-task overrides.
- Support stdout or the shared `-o` result writer with no-clobber by default and explicit
  whole-file replacement; reject conflicting modes before output preparation. Do not merge
  libraries, capture input/model credentials or grant tools. No new dependencies or memory.

## 0.18.0 — 2026-10-27

- Add offline `jsonl-select` to extract one value from every JSONL record using an explicit
  JSON pointer. Preserve record order, numeric/string tokens and selected subtree keys;
  reject missing/ambiguous paths or invalid records with a one-based record position.
- Compose record selection with existing data steps, version-1 recipes, plan preview/export
  and the existing `transform_input` tool across all three provider protocols. Ship an event-ID
  extraction recipe; retain normal native error recovery and original-input-per-call behavior.
- Keep fully buffered success-only output, file replacement protections and existing limits.
  No new dependencies, tool names, model calls for offline commands, permissions or memory.

## 0.17.0 — 2026-10-26

- Validate supported provider turns before dispatching tools. Reject missing/unknown Chat
  finish reasons and Chat/Anthropic stop states that contradict their tool-call phase;
  do not save incomplete Chat text as a successful result.
- Preflight the whole batch of up to 32 calls across Chat, Responses and Anthropic: required
  nonblank unique per-turn IDs, names and argument-field shapes. Check refusal/text errors
  before executing a valid prefix, preserving files when later protocol metadata is malformed.
- Keep absent streamed Chat arguments absent until a real fragment arrives; reject malformed
  stream field types. Preserve normal native tool-error recovery, correctly correlated
  32-call results, per-turn ID reuse and explicit streaming's partial-text behavior.
  No new dependencies, tool names, permissions, retry policy or persistent memory.

## 0.16.2 — 2026-10-26

- Separate regular commits from publication: main pushes run CI without starting Release.
  Start a release explicitly from `release/vVERSION`, the matching version tag, or manual
  dispatch. Validate the entry point against Cargo.toml before building artifacts.
- Check release-entry and release-note rules in normal CI, including rejected main pushes,
  mismatched refs/manual tags and unchanged workflow outputs after validation failures.
  Preserve native installers, all platform/MSRV checks and current-version-only notes.
- Document the commit → CI → explicit release workflow. No CLI behavior, Rust dependencies,
  model tools, permissions or Agent-loop changes.

## 0.16.1 — 2026-10-26

- Reject native data plans whose normalized TOML exceeds the 64 KiB recipe read limit.
  Pointer escaping can expand a smaller source file; check the exported definition before
  input or output access so a successful export remains readable by the same program.
- Apply the same check to execution and `--show-plan`, preserving existing files on failure.
  Verify oversized escaping and a smaller escaped plan that exports and reads back unchanged.
  Ordinary recipes, model tools, dependencies, permissions and the Agent loop are unchanged.

## 0.16.0 — 2026-10-26

- Add `json-select` to native data pipelines and saved recipes, so nested API-response
  arrays can be selected and converted to JSONL in one reusable task. Supply an explicit
  RFC 6901 pointer through `--pointer`, `--step json-select:/path`, or a TOML step.
- Preserve JSON values when composing: string quotes/escapes, numeric tokens, key order
  and repeated keys inside selected values. Keep existing `--select` display/join behavior.
  Validate pointer syntax/limits before IO; reject missing paths and ambiguous referenced keys.
- Extend existing `transform_input` single/ordered steps with the same pointer validation.
  Keep original-input isolation, bounded intermediates/final results and file preservation
  on failure. No new dependencies, tool names, project permissions or Agent-loop changes.

## 0.15.1 — 2026-10-26

- Reject `--config-example` combined with `--json`, `--count`, `--select`, explicit
  input or an instruction instead of silently printing/saving a TOML template. Fix
  successful `--json` calls replacing existing results with non-JSON template text.
- Reject explicit project selectors (`--agent`, `--cwd/-C`, `--read-only`, `--no-exec`)
  for offline and management commands before input, output or configuration access.
  Validate requested selectors even when `--no-tools` disables effective capabilities.
- Preserve legitimate JSON count/select, template exports, inherited model settings
  in offline modes, and model tasks that combine project selectors with `--no-tools`.
  Verify early exit with stdin held open, no probe requests or partial output, existing
  file preservation and all three model protocols. No added dependencies or tools.

## 0.15.0 — 2026-10-26

- Add `zi data --show-plan` to validate a positional operation, ordered `--step`
  list or explicit recipe and export a reusable version-1 TOML definition. Preserve
  operation order and reverse sorting while removing comments and default false values.
- Preview without reading or waiting for stdin, executing transformations, loading
  model configuration or contacting a provider. Reject explicit `--input` rather than
  silently treating data processing as a preview. Definition validity does not prove
  that future input satisfies each operation's type, syntax or resource requirements.
- Save exported plans through existing `-o` no-clobber/explicit `--force` publication;
  invalid definitions fail before output preparation. Include the step number for
  invalid per-operation options while retaining the existing recipe and step bounds.
- Verify all eight operations round-trip through exported recipes, open-stdin behavior,
  definition failures, file preservation and later data-dependent errors. No added
  dependencies, native operations, model tools, permissions or persistent task memory.

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
