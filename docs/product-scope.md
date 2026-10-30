# Product scope and development direction

## Position

zi is a small command-line task assistant for text, structured data and focused developer
workflows. Its home is a pipe: supply explicit input, choose a task, receive usable output.
Natural language handles interpretation; deterministic tools handle exact operations.
Project tools support a bounded developer task when explicitly enabled. They are not a
mandate to become a general Codex replacement or automate a user's entire computer.

Accept a feature when it shortens a repeated input → transformation/analysis → output task,
has a clear invocation and output contract, works across platforms, and has bounded resource
use. Keep offline data operations usable without models. Preserve stdout/stderr separation,
explicit project permissions, independent-task defaults and the single-executable installation.
Conflicting task modes must fail before IO rather than silently replace the requested
operation. Project selectors belong to model tasks; offline and management commands
reject them even when effective capabilities would be disabled by `--no-tools`.

Do not add browsers, desktop UI automation, service managers, a plugin marketplace, a model
runtime/weights, automatic account credential collection or an autonomous background agent.
Do not expose many unrelated tools merely because they can be implemented.

## Sessions and memory assessment

Short follow-up context can fit a task (clarify a summary, refine an extraction or review the
same input), but global chat history and implicit long-term memory do not fit the defaults.
They would introduce hidden provider disclosures, stale input assumptions and permission drift.
The current release keeps each prompt independent; presets are explicit reusable instructions,
not memories of past inputs or model conclusions.

Since v0.25.0, the interactive UI retains only one completed result, at most 16 MiB,
for explicit /save export. It is never model request context and is cleared by a new
task, /clear, /config or successful /use. No automatic history files are created. This
solves answer export without introducing conversational or long-term memory.

Any future task-session design must be opt-in, bounded and inspectable, distinguish current
input from previous observations, reset tool/read/undo state per task, retain model/profile
and permission boundaries, and provide a clear reset/delete path. Start with an in-memory
task session only if a concrete workflow needs it. Persistent auto-summaries, semantic
memories and cross-project retrieval are deferred, not assumed necessary.

Explicit user-maintained instructions/templates are a better first step than automatically
learning preferences. They must be previewable, editable and never grant tools or execute code.

## What to borrow from zi-devtools

The sibling project is a broader desktop toolbox. Its bounded prompt-template handling and
pure text/JSON algorithms are useful design references; its GUI, service manager, MCP host,
image/calendar/knowledge systems are outside zi's position. No source, build or runtime
dependency on the sibling checkout is introduced.

Reviewed primary sources:

- [Prompt template design](https://github.com/ax2/zi-devtools/blob/main/src/prompt_template.rs): bounded literal variables, preview before use; zi's preset implementation is independent.
- [Pure text transformations](https://github.com/ax2/zi-devtools/blob/main/crates/zi-text-core/src/transforms.rs): separate deterministic computation from UI/IO.
- [JSON core](https://github.com/ax2/zi-devtools/blob/main/crates/zi-json-core/src/lib.rs): bounded structured results.
- [Reusable workflows](https://github.com/ax2/zi-devtools/blob/main/docs/workflows.md): saved declarative steps contain definitions, not captured inputs or execution grants.

Candidate native tools should therefore target text cleanup, line selection/deduplication,
JSON inspection and small tabular conversions. They need precise semantics and examples;
they should not require a shell, network service or a desktop installation.

## Customization and scenarios

The first customization layer is a local, versioned TOML preset library. Builtin scenarios
cover summarize, translate, extract-as-JSON, explain, diff review, commit messages, rewrite,
and qualified log-triage/release-note instructions.
Additional instructions and literal variables adapt them. The selected model and the user's
explicit flags still control execution. There is no implicit project preset discovery.

Explicit `zi data --step` and versioned `--recipe` files compose the native operations,
without arbitrary shell commands, includes, dynamically loaded libraries, hidden data reads
or secrets. Definitions contain no input/result paths and are validated before input/output.
Limits are 16 steps, 64 KiB definition, 8 MiB initial data and 16 MiB per intermediate.
This is reusable user-maintained computation, not persistent task memory or a plugin engine.

Since v0.31.0, zi embeds a small reviewed recipe catalog under explicit
`builtin:NAME` references. `zi data --list-recipes` discovers it offline; `--show-plan`
exports the same versioned steps for review and local editing. Reuse existing native
operations and validation; do not add a template engine, automatic project lookup,
new model tools or persistent history merely to expose these scenarios.
`zi data --show-plan` validates and exports that definition without reading data or
running steps. A reusable reviewed recipe fits this customization layer; it contains
neither past inputs nor model conclusions and does not infer execution success from syntax.
JSONL/array conversion fits this scope as deterministic structured-log reshaping, shared
with the existing transform tool. It adds no tool names, file permissions or model runtime.

The same bounded native pipeline can be supplied inline to `transform_input`: 1–16 steps,
original input only, 16 MiB intermediate text and 64 KiB final JSON envelope. It fixes the
need for a model to reconstruct a compound deterministic transformation. It does not load
recipe files, execute expressions or carry tool output into another invocation implicitly.

Since v0.31.0, zi also supports explicit preset-library import with a no-write
preview, collision checks and bounded merge/publication. It installs declarative task
instructions, never model settings, executable code, project permissions or past input.
Provide safe line/column diagnostics for editing definitions; do not print raw parser
source excerpts or values. Preset import, preview and collision handling are one customization improvement,
not separate release items merely to reach a batch count.

Exact integer field sorting extends the existing bounded log workflow: native
frequency records contain integers and cannot use string ordering. Keep this as an
explicit `jsonl-sort-int` operation with stable ties and no float conversion/type
inference, and provide a reviewed top-frequency recipe. Do not add a general math,
query or date engine for this need. See [integer ranking](integer-ranking.md).

## Delivery rule

Every released version needs appropriate tests, Windows/Linux/macOS checks and installable
executables, current-version-only release notes, and one illustrated development article in
Feishu 文章/zistral. Articles begin with the real need, show examples and actual-version output,
explain changes/results/limits, and include diagrams when the main loop or architecture changes.
Controlled-model demos must be identified; successful execution is not evidence of live-model
quality. Record the article URL and release evidence before starting the next release.

JSON selection inside a recipe shortens nested API-response processing. An explicit bounded
RFC 6901 pointer selects from the current intermediate; values stay JSON, not display text.
The same Step validation is shared by saved plans and transform_input. This reuses the
existing lexical selector, adds no tool names or permissions, and stores no data/history.

Since v0.18.0, `jsonl-select` applies one explicit pointer to every JSONL record, with
complete-result failure and original numeric/string tokens. It is shared by CLI plans
and transform_input, adding no tool name or permission. See [record selection](jsonl-selection.md).

Since v0.19.0, `presets export` copies one explicit builtin/custom definition into readable
version-1 TOML under a required custom name, optionally saving literal variable defaults.
It shortens user customization without capturing input, model credentials, permissions or
task history. Exported files are explicitly selected, not automatically installed or merged.

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

Since v0.24.0, `lines-include`/`lines-exclude` filter whole lines by explicit bounded
`contains` literals. CLI, saved plans and transform_input share definition validation and
complete-result behavior; discarded lines still count toward the 100000-line cap. This is
case-sensitive raw-line matching, not regex, structured predicates or implicit redaction.
It adds no dependencies, model tool names, permissions, Agent-loop logic or memory.
See [line filtering](line-filtering.md).

Since v0.25.1, complete model answers sent directly to a terminal use the existing
control-sequence cleaner even when rich UI is disabled. This is a display boundary only:
pipe/file text, retained /save results, valid JSON tokens, offline data operations and
stream decoding keep their contracts. Agent/project tools and permissions are unchanged.
See [terminal output](terminal-output.md).

Since v0.26.0, lines-head/lines-tail select an explicit bounded first/last line window.
Composing literal filtering then tail selection shortens log evidence for a focused model
task across platforms, without a new tool name, permission, runtime or memory store.
Counts are validated before IO and discarded lines still count toward resource limits.
See [line windows](line-windows.md).

Since v0.27.0, explicit --exact routes one successful native data result directly to
output, avoiding a model rewrite. It fits natural-language selection of bounded native
operations, not open-ended analysis or project automation. Multiple calls/free text are
errors; native argument errors may be corrected within the existing turn budget.
No new tool, dependency, permission, persistent history or hidden task context is added.

Since v0.28.0, --exact --export-plan bridges natural-language operation selection and
explicit user-maintained recipes. It exports a validated transform definition only; input
is model context and no data transformation runs. Existing recipes remain inspectable
and offline; no implicit installation, code execution, memory or new tool is introduced.

Since v0.29.0, jsonl-where keeps complete JSONL records by one explicit pointer and
decoded-string equality. It solves structured-log false positives/escaped-string misses
without expressions, type coercion, skipped errors or implicit model context. Shared native
CLI/recipes/tool definitions preserve lexical values and complete-result rules. No new
dependency, tool name, permission, Agent branch or memory. See [field filtering](jsonl-where.md).

Since v0.30.0, three related JSONL string-key operations form one log-analysis
batch: keep first complete records per ID, stable field ordering, and exact per-field
counts. They reuse strict explicit pointer selection and native pipelines, not arbitrary
expressions, numeric/date inference, new tool names, project access or task memory.
See [keyed log analysis](jsonl-key-analysis.md). Publish this coherent batch only after
combined validation, following the batched-release rule in [releasing](releasing.md).

Unreleased on main: explicit JSON/JSONL value redaction fits pre-sharing preparation,
using one literal pointer per native step and a fixed replacement. It preserves other
lexical values, validates the original, and fails complete on missing/ambiguous paths.
It is not automatic secret discovery; model-side masking cannot retract original-input
disclosure, so withhold values by offline processing before any request. No tool names,
runtime, project permissions, Agent-loop branches or implicit history are added.
See [JSON redaction](json-redaction.md). Accumulate it into the next coherent batch.
