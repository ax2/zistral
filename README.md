# Zistral · zi

A small, cross-platform Rust task assistant for text, structured data and focused developer
workflows. It is not a general desktop automation agent. Pipe data in, describe what
you want, pipe the result out. One executable, no Python or Node.js runtime.

[中文说明](README.zh-CN.md) · [Downloads](https://github.com/ax2/zistral/releases)

```sh
cat data.json | zi "Join the first array inside .abc with commas"
cat article.txt | zi "Count the characters in the content"
git diff | zi "Write a concise commit message"
cat notes.txt | zi "Summarize in three sentences" > summary.txt
```

## Install and start

Download a native installer from [Releases](https://github.com/ax2/zistral/releases/latest):

| Platform | Package | Installation |
| --- | --- | --- |
| Windows x64 | `windows-x64-setup.exe` | Per-user installer; adds PATH and a Start menu launcher; no administrator required |
| Debian/Ubuntu x64 | `linux-amd64.deb` | `sudo apt install ./zistral-v0.31.0-linux-amd64.deb` |
| macOS Apple Silicon | `macos-arm64.pkg` | Open the package; installs `/usr/local/bin/zi` |

Then type `zi`. On first launch, choose an existing Codex, WorkBuddy or dsh configuration,
or enter your service and API key. No Rust, Python, Node.js or VC++ redistributable is needed.
Windows setup can launch zi immediately; reopen existing terminals to refresh PATH.
Upgrades retain your model settings. Uninstalling on Windows retains configuration and removes
only the PATH entry owned by the installer. Linux requires glibc 2.35+ and system CA certificates.

Choose one task mode: `--config-example` exports TOML and cannot be combined with JSON
validation, local count/select, input data or an instruction. Project selectors are for
model tasks, so remove them from offline `data`, count/select and management commands.
`--count --json` and `--select /id --json` remain supported. [Mode rules](docs/cli-modes.md).
Installers are unsigned. Portable archives and SHA256 checksums are also available.
A working provider/API key is required for model tasks; no paid credentials are bundled.
Offline operations work immediately without setup.

## Terminal experience

Bare `zi` opens an interactive prompt with a welcome panel, working directory, model/protocol,
animated waiting indicator, request progress, tool activity, elapsed time, and provider token counts
when supplied. Headings, lists and code blocks are highlighted. Use `/help`, `/config`, `/clear`,
or `/exit`; Ctrl+C exits the program. Each prompt is an independent task with no saved/sent history.
Terminal responses stream by default; use `--no-stream` for complete-answer rendering.

Piped/redirected stdout remains plain result data. `--json` stays strict JSON; `--plain` disables
decorations and progress, and `NO_COLOR=1` disables color. Progress uses stderr. Terminal control
sequences in model output are removed when rendering rich terminal output.

## Portable archives and source builds

Portable archives contain one executable plus documentation. Extract zi (zi.exe on Windows) to a directory on PATH. No background service or automatic updater is installed.

Build from source with Rust 1.85+ (a current stable toolchain is recommended):

```sh
cargo install --git https://github.com/ax2/zistral --tag v0.31.0 --locked
```

The package is named `zistral`; the installed command is `zi`. It is not yet
published on crates.io. If your shell already uses `zi` (for example as an
alias), use the full executable path or rename the downloaded executable.

## Configure a model

Choose a model available from your provider. No model name is hard-coded.

Run the interactive setup once:

```sh
zi config
```

The wizard detects existing **Codex**, **WorkBuddy/CodeBuddy**, and **dsh** model
configurations. Select a detected model to import its service URL, model,
protocol, and API key. If a key is available, no re-entry is needed; otherwise
the wizard asks for a hidden key. Select `0` to enter settings manually or keep
editing the current settings. Nothing is changed in the source agents.

For manual setup, enter the service URL (normally ending in `/v1`), model name, API protocol,
and API key. The protocol defaults to `responses` for api.openai.com and `chat`
for compatible/local services. API key input is hidden. The settings are saved
locally and used automatically by subsequent commands:

```sh
cat article.txt | zi 'Summarize in three sentences'
```

Run `zi config --manual` to edit saved settings; Enter keeps each saved value. Enter `-`
at the API key prompt to remove a saved key, for example when switching to a
local service. Ctrl+C cancels without replacing the configuration. You can
choose another file with `zi config --config PATH`. The wizard requires a
terminal and does not consume piped data. To use the single word `config` as a
model instruction instead of the setup command, use `zi -- config`.

Preview discovery without a terminal or saving anything:

```sh
zi config --list
```

| Source | Imported configuration |
| --- | --- |
| Codex | `$CODEX_HOME/config.toml` or `~/.codex/config.toml`, including named profiles, provider URL, wire API, environment API key, or API key in `auth.json` |
| WorkBuddy | `~/.workbuddy/models.json`, with `~/.codebuddy/models.json` as an additional source; complete `/chat/completions` URLs are normalized |
| dsh | `~/.dsh/profiles/*/cordis.yml` and `cordis.patch.yml`; model-provider overrides are merged, with keys resolved from the named environment variable or `.dsh/.credentials.yaml` refs |

Codex ChatGPT account/subscription tokens are **not API keys** and are not
imported. A provider using command authentication, extra headers/query parameters,
or an unsupported protocol is skipped with an explanation. The importer never
runs credential commands or plugins. dsh account/browser tokens are not imported;
only provider API-key references are resolved. Invalid sources do not block
other sources or manual setup. At most 512 candidates are listed, and each
configuration file is limited to 2 MiB. Source schema changes may require a
future adapter update.

Supported protocols are `chat`, `responses`, and `anthropic` (Anthropic Messages,
including dsh's `anthropic-messages` providers). Anthropic roots without `/v1`
use `/v1/messages`; roots already ending in `/v1` use `/messages`.
`--max-output-tokens` sets the Anthropic output limit (default 4096). WorkBuddy
custom-protocol URLs are preserved as an exact `endpoint` in the saved config;
overriding `--base-url` or `--api` discards that exact endpoint for the request.

The key is stored in the local TOML config, not in an encrypted keychain.
Linux/macOS config files are written with mode `0600`; Windows files use the
configuration directory's inherited permissions. Keep the file private. Setup
does not send a test request or validate provider credentials.

Environment variables remain available for temporary overrides:

```sh
export ZI_MODEL='your-model-id'
export ZI_API_KEY='your-api-key'
export ZI_BASE_URL='https://api.openai.com/v1'
export ZI_API='responses'  # for OpenAI models using the Responses API
printf 'Hello world' | zi 'Translate to Chinese'
```

For an OpenAI-compatible Chat Completions provider, set `ZI_API=chat` (the
default) and its API root in `ZI_BASE_URL`. Use `--no-tools` for models that do
not support function calls. Local OpenAI-compatible servers also work:

```sh
export ZI_BASE_URL='http://localhost:11434/v1'
export ZI_MODEL='your-installed-local-model'
export ZI_API='chat'
unset ZI_API_KEY OPENAI_API_KEY
zi 'Explain Unix pipes in one sentence'
```

PowerShell:

```powershell
$env:ZI_MODEL = 'your-model-id'
$env:ZI_API_KEY = 'your-api-key'
$env:ZI_API = 'responses'
Get-Content -Raw -Encoding utf8 .\article.txt | zi '统计汉字数量'
# Avoid shell encoding conversions entirely with --input:
zi -i .\article.txt '统计汉字数量'
```

You can also save `zi --config-example` as:

* Linux/macOS: `$XDG_CONFIG_HOME/zistral/config.toml`, otherwise `~/.config/zistral/config.toml`
* Windows: `%APPDATA%\zistral\config.toml`

Flags override `ZI_*` environment variables, which override the configuration
file. `--config PATH` / `ZI_CONFIG` selects another file. Keys are read from
`ZI_API_KEY`, otherwise `OPENAI_API_KEY` (or `api_key_env` in the config).
If neither environment variable is set, the saved `api_key` is used.
Do not put keys in source control. Remote endpoints require HTTPS; plain HTTP
is allowed on localhost/127.0.0.1/::1 only.

## Named model profiles

```sh
zi config --name work       # Import/manual setup; save and activate work
zi config --name local      # Save another provider without overwriting work
zi models                   # Redacted list; * marks the selected profile
zi use work                 # Persist the default selection
zi -p local 'Explain pipes' # One invocation only
zi doctor                   # Offline configuration diagnostics
zi doctor --probe           # Small model request to test access (charges may apply)
```

Each profile has its own model, service, protocol, key/key reference, and optional exact endpoint.
Legacy settings remain available as `default`; `zi use default` restores them.
`zi config` edits the selected profile; `--name NAME` creates or edits a named one.
Names contain 1–64 ASCII letters/digits/underscores/hyphens; at most 128 named profiles.
Selection precedence: `--profile` > `ZI_PROFILE` > saved active profile. Existing model/service/API/key
overrides still apply. Interactive shortcuts: `/models`, `/use NAME`, `/doctor`.

Doctor reports the config path, selected profile, model, protocol, service host, key presence and
override variable names. Only `--probe` sends a model request; it has a short timeout and disables
tools. Failed configuration/probes exit nonzero, with HTTP/authentication/rate-limit/network guidance.
Keys, provider error bodies and probe answers are never printed. Use `zi -p work doctor --probe`
to check one profile without switching the default.

## Streaming

Transient model HTTP errors can be retried explicitly with `--retries 1` (0–3, default 0).
Only 429/500/502/503/504/529 are eligible before successful response headers. Retry-After
seconds/dates are honored up to 30s; invalid or longer hints stop. Connection, timeout,
accepted-body and stream errors never replay. Retries can incur additional provider work
or charges; `--max-steps` still counts logical turns. [Details and examples](docs/retries.md).

Terminal output streams by default for Chat Completions, Responses and Anthropic Messages,
including fragmented tool arguments and tool continuations. No new runtime dependencies are added.
Non-streaming JSON responses are accepted as a fallback; for services rejecting streaming requests,
use `--no-stream`. Live text is plain; complete-answer mode retains Markdown highlighting.

```sh
zi 'Write a short story'                    # Terminal: live output
zi --no-stream 'Write a short story'        # Wait for the whole answer
cat article.txt | zi 'Summarize' > out.txt  # Redirected: whole result by default
cat article.txt | zi --json 'Extract JSON' # Validated, complete JSON
zi --stream 'Write a short story' | cat     # Explicitly opt into streaming pipes
```

Truncated streams, provider errors and timeouts exit nonzero. Live/explicit-stream output may
already contain partial text; default pipes/redirection and `--json` keep atomic complete output.
`--stream` conflicts with `--json`. `--plain` disables automatic streaming/decorations, but can be
combined with explicit `--stream`. UTF-8, SSE frame/response bounds and terminal-control sanitizing
also apply across streamed chunks. Each interactive prompt remains an independent task.

## Save complete results without truncating a file first

```sh
cat notes.txt | zi -s summarize -o summary.md
cat ticket.txt | zi -s extract -o ticket.json "name and priority"
zi data json-format -i data.json -o data.pretty.json
zi data trim -i notes.txt -o notes.txt --force
zi --count -i notes.txt -o counts.json
zi --config-example -o example.toml
```

`-o/--output FILE` saves the complete UTF-8 result and emits no stdout. It applies to
one-shot model tasks, local data/count/select, configuration examples and `presets export`.
Other management commands and bare interactive sessions reject a file destination. `--output -` keeps
normal stdout behavior, including explicit streaming; `--force` requires a real file.

The default never replaces an existing file. `--force` authorizes replacing an existing
regular file after a successful task. The parent directory must already exist. Destination
checks and a same-directory temporary file are prepared before reading input or calling
a model. The program writes and flushes the complete result, then publishes it; failed
requests, invalid JSON and failed processing preserve the destination. Reading and saving
the same file is supported with `--force`. This protection does not apply to shell `>`
redirection; do not redirect into the input file using the shell.

File output disables automatic terminal token streaming and rejects explicit `--stream`.
The terminal reports `Response ready`, followed by `Saved` only after publication; saved
content is not displayed. Pipelines stay quiet unless `--verbose` is requested. JSON
validation and newline rules match stdout; an empty local line transformation saves zero
bytes. Existing destinations and complete result files are limited to 64 MiB.

The writer rejects links, nonregular/read-only targets and Windows device/alternate-stream
names, and compares observed content/metadata again before publication. This is not an
exclusive lock or cross-process compare-and-swap: another writer can still race the final
check. Use a separate destination when other programs are writing to the same file.
Temporary files are private on Unix; replacement preserves basic rwx permissions, not
ownership, ACLs, extended attributes or hard-link identity. A process crash can leave a
temporary file; no guarantee of power-loss durability is made. Project-agent edits and
commands remain independent side effects; saving an answer does not roll them back.
Only the caller-selected destination is used. The output writer does not add its path or
old content to model requests. Explicit user input/instructions and separately enabled
project tools can still include that information; `--output` itself enables no file tools.

## Offline text and JSON transformations

```sh
cat lines.txt | zi data trim | zi data dedupe | zi data sort
cat lines.txt | zi data sort --reverse
zi data json-format -i payload.json
cat payload.json | zi data json-minify
cat payload.json | zi data json-keys
cat events.jsonl | zi data jsonl-to-array
zi data array-to-jsonl -i payload.json -o events.jsonl
```

These commands do not load model configuration, require a key, call a model or enable
project tools. `trim` removes Unicode whitespace from each line and retains blank lines.
`dedupe` keeps the first exact occurrence, without trimming or case folding. `sort` uses
case-sensitive Unicode order, retains duplicates, and accepts `--reverse`; it is not a
numeric, locale or natural sort. LF/CRLF input lines become LF-terminated output records.
Empty input produces zero bytes; a blank line remains one blank line.

`jsonl-to-array` collects one valid JSON value per line into an array, rejecting blank/bad
records with their record number; empty input produces `[]`. `array-to-jsonl` emits each root
array item as one compact JSON line; `[]` emits zero bytes. Both preserve numeric/string
tokens, item order, object key order and duplicate keys. They support at most 100000 records
and 128 JSON containers (including the wrapper added by collection). No gzip or unbounded
streaming is implied. [Examples and input/BOM rules](docs/json-lines.md).

JSON formatting/minification changes only insignificant whitespace: number/string tokens,
key order and duplicate keys are retained, including integers beyond machine precision.
This is not canonical JSON or duplicate-key validation. `json-keys` requires a root object
and returns a JSON array in source order, including repeated keys; escaped key names are
decoded. Nested values are skipped without numeric conversion. JSON CLI output ends in LF.

Input is capped at 8 MiB even with a larger `--max-input`; line operations/root key lists
support at most 100000 records/keys and JSON at most 128 nested containers. Local output
is buffered up to 16 MiB, including indentation growth. Errors exit nonzero with no output.
`-i/--input FILE` works before or after `data`; `-` reads stdin. Only an explicit
`-o FILE --force` can replace an input file. Windows PowerShell users can use
`Get-Content -Raw` for UTF-8 input.

The same core is available to the model as `transform_input`, including an ordered `steps`
array for compound tasks such as trim → dedupe → sort. The original single-operation
shape remains valid. Each invocation starts from original input; only steps inside that
invocation consume the preceding intermediate. Plans contain 1–16 existing operations,
with 8 MiB original input, 16 MiB intermediates and a 64 KiB final JSON tool-result limit.
Errors return no intermediate output. No tool names or permissions are added.
See [tool pipeline examples and boundaries](docs/tool-pipelines.md).
`data` is a reserved command; `zi -- data ...` treats it as a literal model instruction.

## Reusable offline pipelines

Since v0.31.0: `zi data jsonl-sort-int --pointer /count --reverse` ranks
whole records by exact JSON integer literals. `2` precedes `10` ascending; arbitrarily
long digits retain precision within existing size limits, and ties (including -0/0)
stay stable both ways. Decimal/exponent syntax and string coercion are rejected.
Use `--recipe builtin:top-severity-counts` for a ready-to-run top20 frequency report.
[Semantics, limits and reusable examples](docs/integer-ranking.md).


Since v0.31.0: installed binaries embed five reviewed recipes. Discover,
inspect and run them without a source checkout or model configuration:

```sh
zi data --list-recipes
zi data --recipe builtin:severity-counts -i events.jsonl
zi data --show-plan --recipe builtin:recent-unique-events -o recent.toml
# Edit pointers or the line limit in recent.toml, then reuse it offline:
zi data --recipe recent.toml -i events.jsonl
```

`builtin:cleanup` trims/deduplicates/sorts lines descending; `builtin:severity-counts`
counts raw `/level` values as CSV; `builtin:unique-severity-counts` first deduplicates
by `/id`; `builtin:recent-unique-events` keeps first `/id`, sorts `/time` descending
and takes 20 records. `builtin:top-severity-counts` ranks exact integer counts descending
and exports the top 20 groups as CSV. Category/ID/time keys remain strings. Timestamp order is lexical;
normalize formats/timezones first. Names are exact and case-sensitive. `--list-recipes`
does not read stdin; explicit `--input` and processing options are rejected.


```sh
cat lines.txt | zi data --step trim --step dedupe --step sort:reverse
zi data --recipe docs/examples/cleanup.toml -i lines.txt -o clean.txt
zi data --show-plan --recipe docs/examples/cleanup.toml
zi data --show-plan --step trim --step dedupe --step sort:reverse -o cleanup.toml
```

Use one operation, repeated `--step` in execution order, or an explicit `--recipe` file (or qualified builtin reference in development builds).
Recipes have `version = 1` and `[[steps]]` entries with `operation` and optional `reverse`
(sort, jsonl-sort or jsonl-sort-int only). All definitions are checked before reading input. Only the complete final
result is emitted; a failed later step emits no intermediate stdout and does not replace
an existing output file. Shell `>` still opens/truncates its destination independently.

Limits: 1–16 steps, 64 KiB UTF-8 recipe, 8 MiB initial input, 16 MiB per intermediate/final
result; existing line/key/depth limits still apply. The recipe is caller-supplied local
data, never a model prompt or project permission. No shell, includes, variables, implicit
discovery or model access. [Recipe format and examples](docs/data-recipes.md).

`--show-plan` validates the same plan and exports normalized version-1 TOML without
reading/waiting for stdin or running data operations. Explicit `--input` is rejected;
`-o` saves the definition using the usual no-overwrite rules. Exported plans are reusable
recipes, not dry-run results or a guarantee that future data will pass each operation.

## Reusable task presets

Since v0.31.0: import an exported version-1 preset into your library:

```sh
zi presets import team-fr.toml --dry-run
zi presets import team-fr.toml
zi presets import team-fr.toml --replace --dry-run
```

Names collide only with explicit `--replace`; unrelated custom presets stay, builtins
cannot be replaced. Preview lists names without printing prompts/defaults or writing
files. Both libraries and their merged limits are checked before publication. A successful
import normalizes TOML and drops comments/formatting. [Import, limits and safe diagnostics](docs/preset-import.md).


```sh
zi presets                              # Offline list of builtin and user presets
cat article.txt | zi -s summarize
cat notes.txt | zi -s translate --var language=French
git diff | zi -s review 'Focus on correctness'
git diff --cached | zi -s commit
cat ticket.txt | zi -s extract 'name, urgency, next_action'
zi presets show translate
zi presets render translate --var language=Chinese  # Offline prompt preview
```

Builtins: `summarize`, `translate` (Chinese by default), `extract`, `explain`, `review`,
`commit`, `rewrite`, `builtin:triage`, and `builtin:release-notes`. Additional instruction words refine the selected task; piped input
remains a separate data message. `extract` enables whole-answer JSON validation automatically
and conflicts with `--stream`. Presets use the selected model and existing pipe/stream rules.
They never enable project, file or command capabilities; add project flags explicitly when
appropriate, for example `-C . --read-only -s review`. Reviewing a piped diff requires none.

Create an editable personal preset file without overwriting existing content:

```sh
zi presets init
zi --presets ./my-presets.toml presets init
zi --presets ./my-presets.toml -s team-summary --var audience=developers
```

Default: `presets.toml` beside the selected model config (`--config` / platform directory).
`--presets FILE` / `ZI_PRESETS` selects another file. Project directories are not scanned for
presets; a repository cannot silently install instructions. Example format:

```toml
version = 1
[presets.team-summary]
description = "Summarize for a particular audience"
prompt = "Summarize the input for {{audience}}. Preserve decisions and action items."
# json = true  # Optional: validate the entire answer as JSON
[presets.team-summary.defaults]
audience = "the engineering team"
```

Only literal `{{name}}` variables are substituted once, without expressions, commands,
recursive expansion or file reads. Names use ASCII letters/digits/underscore, start with
a letter/underscore, and have at most 32 bytes. `input` is reserved: original input is sent
separately, never interpolated. Values are 1–4096 bytes, with at most 12 distinct variables;
missing, duplicate and unused overrides fail before contacting a model. Double braces are
variable syntax, not an escape language; ordinary single JSON braces are unchanged.
Files are limited to 256 KiB / 64 custom presets, prompts to 16 KiB, rendered tasks to 128 KiB.
Custom names cannot replace builtins. Unknown fields/versions and malformed presets fail
explicitly without echoing raw TOML. `init` is atomic, never clobbers a file, and uses `0600`
on Unix. Presets are ordinary local text, not encrypted storage: do not put keys in prompts.

Interactive shortcuts: `/presets` lists tasks, `/run NAME [additional instruction]` runs one
as an independent task. Use CLI `--var` for parameterized overrides; interactive `/run` uses
the preset defaults plus its additional instruction. Nothing is added to persistent memory.
For a literal instruction beginning with the reserved word `presets`, use `zi -- presets`.

## Work on a project

```sh
zi -C ./my-project 'Fix the failing tests and run verification'
zi --agent 'Inspect this project and implement the requested change'
zi -C . --read-only 'Review this project and explain the main risks'
zi -C . --no-exec 'Update the documentation without running commands'
zi -C . --command-timeout 300 'Run the build and fix errors'
git diff | zi -C . 'Review these changes against the project source'
```

`--agent` enables project tools in the current directory; `-C` / `--cwd DIR` selects a
project and enables them. Bare `zi -C DIR` opens the interactive prompt in project mode.
Use a model that supports tool calls. Each task can inspect, plan, edit, run commands,
read failures, fix them and verify again; default budget is 32 model requests.
`--max-steps` allows 1–128. Each interactive prompt starts a fresh task.

| Tools | Capabilities |
| --- | --- |
| `project_info`, `list_files`, `search_files`, `read_file` | Project context, scoped AGENTS.md, gitignore-aware file listing, literal search and paginated UTF-8 reads |
| `write_file`, `edit_file`, `delete_file`, `undo_edit` | Create/replace, exact text replacement, single-file deletion and conflict-checked undo within the current task |
| `git_status`, `git_diff` | Bounded read-only Git inspection; Git is optional |
| `run_command`, `update_plan` | Noninteractive builds/tests, exit status/output/timeout, and step tracking |
| `count_input`, `select_input`, `transform_input` | Deterministic counting, JSON selection and bounded text/JSON transformations |

Existing files must be read before changing them; stale revisions are refused. File edits
preserve executable permissions and CRLF replacements, write atomically, and keep up to 32 MiB
of undo preimages in memory. Root/nested `AGENTS.md` instructions are supplied before edits.
Text files are limited to 2 MiB; each read/search/command stream returns at most 64 KiB.
Commands use PowerShell on Windows and `/bin/sh` on Unix, with no interactive stdin;
default timeout is 60 seconds, configurable up to 3600. Background descendants are terminated
when a command finishes or times out. Build tools needed by your project must be installed
separately; zi itself still needs no language runtime.

**Project mode authorizes local changes and commands without per-tool confirmation.**
Use it for trusted tasks/projects. `--read-only` permits inspection and fixed Git reads only;
`--no-exec` allows file edits but disables arbitrary shell commands. Native file tools restrict
paths to the project, reject escaping symlinks and common credential/metadata paths, and skip
ignored/cache files during discovery. These checks are **not an OS sandbox**: enabled shell
commands run with your account's permissions and can access outside the selected directory.
Keep a Git commit/backup for valuable work. Undo does not cover commands and is lost on exit.

Default pipes remain data-only unless project mode is explicitly enabled. In project mode,
stdout still contains the final result; command/tool details go to the model and activity goes
to stderr. `--json` and default redirected output validate/buffer the answer, but file changes
happen immediately and are not rolled back on model/network failures. See
[agent design and references](docs/agent-design.md) for architecture, validation and limits.

## Exact local operations — no model needed

```sh
cat article.txt | zi --count
printf '{"abc":[["a","b","c"]]}' | zi --select /abc/0 --join ','
# a,b,c
cat data.json | zi --select /abc/0
```

`--select` uses [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901): `/abc/0`
is the first item of `abc`, `~1` escapes `/`, and `~0` escapes `~`. An empty
pointer selects the root. Strings print without JSON quotes; other values
print compact JSON. `--join` requires an array and joins its displayed values.

Selection/join preserves original numeric tokens, object key order and repeated keys
inside selected containers, without a numeric value tree. Referenced duplicate member
names fail rather than choosing one. Plain pointers are bounded to 4096 UTF-8 bytes/128
tokens, JSON depth to 128, local input/result text to 64 MiB (normal input defaults still
apply) and select_input tool text to 64 KiB. `--json` also uses lexical compaction, so
validation preserves oversized numbers and escape forms. [Exact rules](docs/json-selection.md).

`--count` returns JSON with clearly defined fields:

| Field | Meaning |
| --- | --- |
| `bytes` | UTF-8 bytes after removal of one leading BOM |
| `characters` | Unicode scalar values, including whitespace |
| `graphemes` | Extended grapheme clusters, e.g. a composed emoji counts as one |
| `non_whitespace_characters` | Unicode scalars excluding whitespace |
| `words` | Whitespace-delimited tokens, **not** Chinese word segmentation |
| `cjk_characters` | Han ideographs in the supported Unicode ranges |
| `lines` | Text lines; a final newline does not create an extra empty line |

Natural-language counting and JSON requests use these same functions as agent
tools. The model chooses the tool/arguments and formats the final answer;
`--count`/`--select` avoid model interpretation when exact output is required.

## Pipeline contract

* UTF-8 stdin or `--input FILE`; `--input -` explicitly reads stdin.
* Interactive terminal input is not waited on unless `--input -` is given.
* Only the final result goes to stdout; a missing final newline is appended.
  An explicit `--output FILE` saves it instead and leaves stdout empty.
* Errors and `--verbose` tool progress go to stderr. No spinner or banner.
* Exit `0`: success (including a downstream closed pipe). Exit `1`: runtime
  failure. Exit `2`: invalid command-line syntax.
* `--json` validates the entire answer before emitting compact JSON; invalid
  output never leaks a partial result to stdout. A locally selected string
  must itself contain JSON when combined with `--json`.
* Options go **before** the instruction. All remaining words become the prompt.
* Input is limited to 8 MiB by default (`--max-input`, at most 64 MiB). Responses
  are limited to 16 MiB; tasks default to 8 data turns or 32 project turns
  (`--max-steps`, maximum 128), and each request to
  120 seconds (`--timeout`). The whole input is buffered and sent to the model;
  input is not automatically chunked. Terminal response streaming is described above.

```sh
cat data.json | zi --json 'Convert to a JSON array containing only names' | jq .
zi --no-tools --timeout 30 'Give a short explanation of Rust ownership'
zi --help
```

## Scope and privacy

Feature boundaries and the session/memory assessment are recorded in
[product scope](docs/product-scope.md). Presets/custom prompts do not grant tools, create
implicit history, load repository configuration or run plugin code.


Default data mode uses only `count_input`, `select_input` and `transform_input`. Explicit project mode adds
local inspection, file changes and commands as described above. There is no built-in web
browser, persistent conversation history, background agent service or OS sandbox. Network
mode sends the instruction, input and any inspected project/tool content to the configured
provider, whose retention policy still applies. Common sensitive file names are blocked by
native file tools, but arbitrary shell commands are not confined to those checks. Responses requests set `store: false`. API error bodies
are not echoed because they may contain private input. HTTP redirects are
rejected rather than forwarding credentials to another endpoint.
The separate `zi config` setup command writes only its selected configuration file.

## Development and releases

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Integration tests use local mock HTTP servers and require no paid API access.
They verify a real Rust project through failing tests, source edits and passing tests for
+all three protocols, permission enforcement, stale edits/undo, process-tree timeouts,
+actual subprocess pipes, real terminal configuration/import sessions with
hidden key entry, source preservation, missing keys, saved configuration and
environment precedence, tool round trips for all three protocols,
Unicode input, invalid input, JSON validation, errors, and step limits. They
do not prove any particular provider/model's quality or compatibility.

CI runs tests on Windows, Linux, and macOS. A new package version on `main`
automatically starts a release; already published versions are skipped.
Releases build all three platforms,
test the binaries, attach archives and SHA-256 checksums, and publish only when
every build succeeds. See [CONTRIBUTING.md](CONTRIBUTING.md).

MIT licensed. Protocol reference: [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling).


Select nested JSON inside the same saved pipeline:

```sh
zi data --step json-select:/data/items --step array-to-jsonl -i response.json
zi data --show-plan --step json-select:/data/items --step array-to-jsonl -o records.toml
zi data --recipe records.toml -i response.json -o records.jsonl
```

`json-select` requires an explicit RFC 6901 `pointer` (empty selects root), also available
as `zi data json-select --pointer /data/items`. It emits a JSON value: strings retain
quotes/escapes, numbers remain exact. Existing `--select` still displays decoded strings
and supports `--join`. Pointer validation precedes IO; existence/type depends on actual data.
The same pointer-bearing steps work in `transform_input` without a new tool or permission.
See [nested JSON recipes](docs/json-selection-recipes.md).

Maintainers: [release workflow](docs/releasing.md). Main commits run CI; publication starts
only from an explicit release branch/tag or a matching manual dispatch.

Provider turns are checked before tools run: incomplete Chat replies fail, and malformed
call metadata in a later batch member cannot execute earlier tools. Normal native argument
errors still return observations for model recovery. [Protocol and streaming limits](docs/provider-turns.md).

## Extract from every JSONL record

```sh
zi data jsonl-select --pointer /payload/id -i events.jsonl
cat events.jsonl | zi data --step jsonl-select:/payload/id --step dedupe --step jsonl-to-array
zi data --recipe docs/examples/event-ids.toml -i events.jsonl -o ids.json
```

One exact JSON value per record, in order; strings remain quoted and numbers retain their
original tokens. A missing/ambiguous field or malformed later record fails the whole result
with a record number, without partial stdout or replacing an existing `-o` target.
Also available in the existing `transform_input` tool. No model is used for `zi data`.
[Rules, recipe export, limits and examples](docs/jsonl-selection.md).

## Export and adapt a task preset

```sh
zi presets export translate --name translate-ja --var language=Japanese -o team-presets.toml
zi --presets team-presets.toml presets render translate-ja
zi --presets team-presets.toml -s translate-ja -i notes.txt
```

Export one builtin/custom definition as reusable version-1 TOML. Keep placeholders, description
and JSON-output rules; `--var` saves literal new defaults. Export is offline and does not read
piped input or grant tools. Choose a non-builtin destination `--name`; `-o` defaults to no
overwrite. Explicit `--force` replaces the whole target library, without merging.
[Exact rules and export versus render](docs/preset-export.md).

## Import a CSV table without changing cell types

```sh
zi data csv-to-jsonl -i table.csv
zi data --step csv-to-jsonl --step jsonl-select:/id --step dedupe --step jsonl-to-array -i table.csv
zi data --recipe docs/examples/csv-ids.toml -i table.csv -o ids.json
```

Comma CSV requires a unique nonempty header and consistent rows. Quoted commas, doubled
quotes and multiline cells are supported. All cells become JSON strings, preserving leading
zeros and large numeric text; no automatic trimming, type inference or skipped bad records.
The same operation is available in `transform_input`, with existing result/permission limits.
[Dialect, resource limits, errors and examples](docs/csv-import.md).

## Export JSONL records as CSV

```sh
zi data jsonl-to-csv -i records.jsonl -o table.csv
zi data --recipe docs/examples/api-csv.toml -i response.json
```

The first flat object defines column order; later records must have the same unique columns,
with any key order. Strings decode; number tokens, boolean and null become exact cell text.
All fields are quoted, with LF terminators and no BOM. Missing/extra/duplicate keys, nested
values or bad records fail without partial results. CSV has no JSON type metadata; quoting
does not control spreadsheet inference or sanitize formula-like content.
See [CSV export](docs/csv-export.md) for limits, typing and consumer boundaries.

## Select and rename multiple JSONL fields

```sh
zi data jsonl-project --field id=/payload/id --field amount=/payload/n -i events.jsonl
zi data --recipe docs/examples/project-csv.toml -i events.jsonl -o table.csv
```

Ordered literal field definitions build one new object per record, retaining selected JSON
values and exact number/string tokens. Missing or ambiguous paths fail; no row skipping,
inferred defaults or expressions. Use a version-1 recipe for compound projection and CSV.
`--show-plan` can save a reusable definition without reading data.
See [JSONL projection](docs/jsonl-projection.md) for field/pointer/output limits.

## Triage logs or draft release notes

```sh
zi -s builtin:triage -i app.log 'Separate evidence, hypotheses and next checks'
git log --oneline v0.22.0..HEAD | zi -s builtin:release-notes --var audience=users
zi presets render builtin:release-notes --var audience=operators
```

Two more bundled instructions bring the catalogue to nine. The new qualified names leave
custom `triage`/`release-notes` entries untouched. Existing builtins also accept `builtin:NAME`;
these references never select an unrelated custom definition. Preview and export are offline,
while model tasks still need configuration and factual review. No new tools or memory.
See [task scenarios](docs/task-scenarios.md) for lookup, export and quality boundaries.

## Filter log lines before a task

```sh
zi data lines-include --contains ERROR -i app.log
zi data --recipe docs/examples/log-errors.toml -i app.log -o errors.log
zi data --step lines-include:ERROR --step lines-exclude:healthcheck -i app.log | zi -s builtin:triage
```

Keep or exclude whole lines by an explicit case-sensitive literal substring, without a model.
Matching is not regex, JSON-field filtering or secret detection. Order and duplicates stay;
no matches succeeds with empty output. The same `contains` definition can be previewed,
exported and used by transform_input. Filtering inside a model tool does not undo disclosure
of the task's original input; filter locally before piping to a model when you want to choose
its input. See [line filtering](docs/line-filtering.md) for limits and newline semantics.

## Save an answer after reading it

After a task completes in interactive `zi`, use:

```text
/save notes.md
/save! notes.md
/clear
```

`/save` creates a new file; `/save!` explicitly permits replacement. Use an unquoted literal
path, including spaces. Only the latest successful result is retained (at most 16 MiB, in
process memory); a new task, /clear, /config or successful /use discards it. Complete streamed
answers can be saved, partial/failed answers cannot. No extra request or chat history is sent.
See [interactive export](docs/interactive-export.md) for JSON, file and path boundaries.

Complete model answers printed directly to a terminal also remove provider terminal controls
under `--plain` or `TERM=dumb`. Pipes, explicit files and `/save` retain original text and
existing newline rules; see [terminal output boundaries](docs/terminal-output.md).

For `--json` model results, direct terminal output escapes literal DEL/C1 characters as
JSON Unicode escapes so parsed values survive display. Pipes, explicit files and `/save`
keep the existing compact JSON bytes; numeric and existing escape tokens are preserved.

Select exact first/last lines offline with `zi data lines-head -n 20` or
`zi data --step lines-include:ERROR --step lines-tail:20 -i app.log`. Counts are explicit;
whitespace, duplicates and blank lines remain. See [line windows](docs/line-windows.md).

Use `cat app.log | zi --exact "Keep the last two ERROR lines, preserving spaces"`
to let the model choose one native data operation/pipeline and return its result without
a model rewrite. This guarantees native execution of the selected plan, not interpretation
of every instruction. See [exact data results](docs/exact-results.md).

Use `zi --exact --export-plan -i app.log -o recent.toml "Keep the last 20 ERROR lines"`
to export a model-chosen native recipe for review and later offline reuse. No data
transformation is executed during export. See [model plan export](docs/model-plan-export.md).

Filter whole structured log records by a decoded JSON string, offline:

```sh
zi data jsonl-where --pointer /level --equals ERROR -i events.jsonl
zi data jsonl-where --pointer /level --equals ERROR --show-plan -o errors.toml
```

[JSONL field filtering / 字段筛选](docs/jsonl-where.md)

## Structured log analysis (v0.30.0)

[Structured log analysis](docs/jsonl-key-analysis.md) groups three related improvements:
`jsonl-dedupe --pointer /id` keeps the first complete event per decoded string ID;
`jsonl-sort --pointer /time` stably sorts by an explicit string key;
`jsonl-count-by --pointer /level` produces exact frequency records. All share offline
recipes and the existing model tool. See the v0.30.0 release for the corresponding installable packages.
