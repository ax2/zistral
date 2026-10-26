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

`transforms.rs` implements eight deterministic operations on explicit input, shared by the
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
Each tool invocation transforms the original input, not a previous tool's result; explicit
CLI pipes compose multiple transformations. `recipes.rs` additionally validates caller-selected
`--step` lists or versioned TOML `--recipe` files before reading data. The CLI executes at most
16 pure steps, buffering only current/next intermediate (16 MiB each, initial input 8 MiB),
and emits only the final success. No recipe steps are added to model tools or task context.
Sessions, permissions and memory are unchanged.

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
