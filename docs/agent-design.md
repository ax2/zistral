# Agent architecture, references and limits

Zistral implements its own bounded model → tools → observations → model loop in Rust.
It does not bundle Codex, WorkBuddy, dsh, a separate agent runtime, or model weights.
Provider/model reasoning is remote (or from a separately installed compatible local model).
The installed executable includes the loop, file/search/edit/planning/process tools and TLS.

## References

The implementation uses the public model function-calling protocols. Project tools follow
common coding-agent patterns: explicit working directory, tool schemas, bounded observations,
read before edit, task planning and execution feedback. The design was checked against:

- [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling).
- [Codex shell tool schemas](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/shell_spec.rs): working directory, command timeouts and output budgets.
- [Codex execution server](https://github.com/openai/codex/blob/main/codex-rs/exec-server/README.md): command lifecycle and structured exit/output metadata.
- [Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects): Windows process-tree cleanup; Unix uses process groups.
- [ignore WalkBuilder](https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html): native gitignore-aware file discovery.

These are design/protocol references, not a claim that zi reuses the Codex framework or
provides all its features. The project/process implementation is original Zistral code;
normal Cargo dependencies and their licenses remain in Cargo.lock.

## Task execution

CLI mode validation precedes configuration, input reads and output preparation. Template
export rejects JSON/data/instruction modes; project selectors are rejected for offline and
management commands based on requested flags rather than effective tool permissions.
Model tasks still allow `--no-tools` to disable project tools. See [CLI mode rules](cli-modes.md).

`agent.rs` manages protocol histories, tool dispatch, streaming, turn budgets and final output.
`project.rs` owns each task's root, permissions, scoped instructions, read revisions, edit
preimages and plan. `process.rs` runs noninteractive commands with separate bounded streams,
exit status and timeout cleanup. Tool errors return to the model so it can adjust and retry.
Pending plan steps prompt continuation; exhausted budgets and incomplete API replies fail.

Opt-in `--retries` adds a bounded status-only inner request loop, without changing protocol
history/tool dispatch. It reuses the same body for 429/500/502/503/504/529 before accepting
successful headers. Retry-After seconds/dates have a 30s cap; invalid/long hints stop.
Transport/accepted-body/stream errors never replay. `--max-steps` counts logical turns,
while terminal `requests` counts actual HTTP attempts. Tools execute only after successful
response parsing and are not rerun by an HTTP retry. See [retry contract](retries.md).

The full project mode has 15 tools. Data mode exposes count/select/transform; read-only mode
excludes file mutation and arbitrary shell execution, including enforcement if a provider
requests tools that were not advertised. `--no-tools` disables project capabilities too.

`transforms.rs` implements thirteen deterministic operations on explicit input, shared by the
offline `zi data` command and the `transform_input` tool. They do not access a model,
filesystem or shell. JSON syntax is checked with serde's IgnoredAny; a lexical formatter
changes only insignificant whitespace, preserving number/string tokens, key order and
repeated keys. Root-key enumeration skips values without numeric conversion. Conversion
between JSONL records and root array items also avoids numeric conversion;
record collection validates every physical line, while array splitting scans only root
commas after full syntax validation and tracks string escapes/nesting. Blank/invalid
records and more than 100000 items fail. The wrapper counts toward the common depth cap.
Input is bounded to 8 MiB, 100000 lines/keys/records and 128 containers. Local output is buffered up to
16 MiB; tool results up to 64 KiB (including their JSON envelope) fail rather than truncate.
Each tool invocation transforms the original input, not a previous tool's result. The tool
also accepts an exclusive ordered `steps` array of 1–16 native operations; its single-operation
shape remains compatible. The whole plan is validated first, current/next intermediates are
bounded to 16 MiB, and the final serialized result is capped at 64 KiB. A failed step returns
an error without intermediate output; the model may adjust and call again on original input.
There is no cross-call state or additional model request inside a tool. Tool arguments are
capped at 64 KiB. See [tool pipeline contract](tool-pipelines.md).
`recipes.rs` additionally validates caller-selected
`--step` lists or versioned TOML `--recipe` files before reading data. The CLI executes at most
16 pure steps, buffering only current/next intermediate (16 MiB each, initial input 8 MiB),
and emits only the final success. Recipe files are not read by model tools or sent to task context;
the model must explicitly supply its own inline steps when using the transform tool.
Sessions, permissions and memory are unchanged.

`zi data --show-plan` uses the same prepared plan to export a reusable version-1 TOML
definition. It bypasses input reads and all step execution, rejects explicit `--input`,
and optionally uses the existing result writer to save the definition. It is a CLI-only
inspection/export path, not a model tool, hidden recipe import or data-dependent dry run.

`selection.rs` fixes the older Value-based select/join path: syntax/depth validation
precedes a bounded RFC 6901 traversal that borrows RawValue text and skips unrelated
values. Referenced duplicate keys fail; selected containers retain their lexical numbers,
key order and repeated keys. Local selection text is capped at 64 MiB; tool text at 64 KiB.
Final `--json` validation in one-shot and interactive tasks shares lexical compaction,
without a numeric round trip. serde_json's optional raw-value features add no packages;
only the validated selector disables recursion checks after the common 128-depth guard.

## Verified requirements

`--output` is caller-controlled result delivery in `output.rs`, not an agent tool. A
same-directory temporary result is prepared before requests; file mode suppresses token
streaming and validates JSON before publication. Default no-clobber and explicit replacement
check observed revisions; neither is an OS sandbox, exclusive lock or full agent transaction.
Project edits/commands are not rolled back on a delivery error. The destination's old bytes
and path are not injected into model context by the output writer. Explicit user input
or separately enabled project tools may still contain them. Stdout remains the default.

Automated local-provider fixtures drive the actual zi subprocess through all three protocols
(Chat Completions, Responses and Anthropic Messages). For each, zi inspects a real tiny Rust
project, runs cargo test and receives a failure, edits the source, creates a file, reruns the
tests successfully, completes its plan and emits only the final answer on redirected stdout.
This is a real filesystem/build-tool workflow with simulated model decisions, not a benchmark
of live-model coding quality. Other tests cover stale edits, undo, instruction scope,
ignore/path/symlink checks, permissions, process-tree timeouts, Unicode, streaming and pipes.
CI and release jobs run on Windows, Linux and macOS; installation is also tested natively.

This covers the main requirements for lightweight CLI project reading, source modification,
command execution and bounded multi-step tasks. A compatible tool-capable model and the
project's build/test programs are still required; successful completion depends on model
quality, permissions, provider compatibility and the task's scope.

## Deliberate boundaries

- No OS sandbox, per-command approval policy, persistent conversation/resume, background
  execution server, interactive PTY command sessions, MCP/plugin ecosystem or autonomous browser.
- Shell runs with the current OS account and can access outside the project. File path/secret
  checks constrain only native tools; they are not an adversarial security boundary. Avoid
  untrusted tasks/projects and review changes with version control.
- File tools handle UTF-8 regular files up to 2 MiB and exact text replacements. They are not
  a binary editor or a general patch engine. Search is literal and bounded, not semantic indexing.
- Git is optional; inspection disables hooks/fsmonitor/external diff/textconv. Arbitrary
  commands can invoke other programs, so Git and build tools are not bundled.
- Undo is in-memory, last-in-first-out and conflict-checked for file-tool changes only, up to
  32 MiB of preimages. No persistent recovery, command rollback, or whole-task transaction.
- AGENTS.md is scoped from the selected root down to the accessed path; ancestor files above
  that explicit root are not loaded. Guidance is supplied to the model, not formally enforced.
- Default limits: 32 project turns (8 data turns), 120 seconds per model request, 60 seconds
  per command, 64 KiB per command stream/read/search output. Use CLI options for turn/time budgets.

These limits keep zi compact and useful for ordinary coding tasks without claiming Codex parity.

In v0.16.0, recipes and transform_input share a pointer-aware Step definition. json-select
uses the existing lexical JSON traversal, but returns quoted JSON strings for composition;
select_input and CLI --select retain decoded display/join behavior. Pointer syntax/limits
are validated before input, and the complete source JSON is validated before selection.
Main-loop, original-input-per-call and project permission rules remain unchanged.

## Provider-turn preflight (v0.17.0)

`agent.rs` checks terminal status/refusal/text shapes, then prepares the entire supported
tool batch before history updates or tool dispatch. IDs must be nonblank and unique in
one turn; names/argument field shapes must be present. Chat and Anthropic completion phases
must agree with whether calls exist. Native argument/schema failures still return model
observations; this is protocol validation, not a transaction or rollback of valid tools.
`stream.rs` retains missing Chat arguments as null until a real string fragment arrives,
so assembly cannot turn an absent field into apparently supplied empty arguments. Explicit
empty strings remain tool arguments and can return normal recoverable JSON/schema errors.
Known malformed text/metadata fields fail before tools, including a later Responses refusal.
Explicit streaming can already have displayed text; it does not commit tool side effects
before a completed, validated turn. See [provider-turn contract](provider-turns.md).

Since v0.18.0, `jsonl-select` applies one explicit pointer to every JSONL record, with
complete-result failure and original numeric/string tokens. It is shared by CLI plans
and transform_input, adding no tool name or permission. See [record selection](jsonl-selection.md).

`presets export` (v0.19.0) is an offline CLI path before model configuration/input reads.
It serializes one renamed definition after shared variable validation, preserving templates
and JSON flags rather than rendering or contacting a model. Explicit result files reuse
output.rs. It does not change Agent dispatch, tools, protocol histories or task boundaries.

Since v0.20.0, `csv-to-jsonl` imports one explicit header-based comma CSV dialect into
JSONL string-valued objects. A quote/record preflight precedes csv-reader decoding; unique
headers and exact row widths are required. Shared pipelines/tools retain original-input
and final-success-only output rules. It adds csv/csv-core packages, not tool names or rights.
See [CSV import](csv-import.md).

Since v0.21.0, `jsonl-to-csv` exports nonempty scalar-valued objects with first-record
column order and exact subsequent column sets. Numeric tokens become exact text, strings
decode, every CSV field is quoted. Nested values and ambiguous columns fail; CSV has no
JSON type metadata or spreadsheet formula sanitization. The shared bounded pipeline and
original-input tools gain no new names, dependencies, permissions or memory.
See [CSV export](csv-export.md).

Since v0.22.0, `jsonl-project` selects/renames 1-64 explicit JSON pointers into each new
ordered record object. Literal unique names and typed field definitions are shared by the
single CLI command, version-1 recipe/plan and existing transform_input. Selected JSON tokens
and subtree order remain exact, missing/ambiguous references fail, and the added object
counts toward depth128. Projection can feed CSV without expressions or hidden state.
No new dependencies, tool names, permissions, Agent loop or memory.
See [JSONL projection](jsonl-projection.md).

Since v0.23.0, the nine bundled scenarios include qualified `builtin:triage` and
`builtin:release-notes` for log evidence and change-range writing. Existing short user names
remain available; the original seven protected builtin names also accept explicit qualified
references. Lookup never falls back to arbitrary custom entries, nor bypasses selected-library
validation. The shared preset flow keeps JSON/capability/independent-task boundaries.
No new dependencies, model tools, loop logic, permissions or memory.
See [task scenarios](task-scenarios.md).
