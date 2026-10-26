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
| Debian/Ubuntu x64 | `linux-amd64.deb` | `sudo apt install ./zistral-v0.7.0-linux-amd64.deb` |
| macOS Apple Silicon | `macos-arm64.pkg` | Open the package; installs `/usr/local/bin/zi` |

Then type `zi`. On first launch, choose an existing Codex, WorkBuddy or dsh configuration,
or enter your service and API key. No Rust, Python, Node.js or VC++ redistributable is needed.
Windows setup can launch zi immediately; reopen existing terminals to refresh PATH.
Upgrades retain your model settings. Uninstalling on Windows retains configuration and removes
only the PATH entry owned by the installer. Linux requires glibc 2.35+ and system CA certificates.
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
cargo install --git https://github.com/ax2/zistral --tag v0.7.0 --locked
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

## Reusable task presets

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
`commit`, and `rewrite`. Additional instruction words refine the selected task; piped input
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
| `count_input`, `select_input` | Existing deterministic pipe operations |

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


Default data mode uses only `count_input` and `select_input`. Explicit project mode adds
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
