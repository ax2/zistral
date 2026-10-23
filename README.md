# Zistral · zi

A small, cross-platform Rust agent for your terminal. Pipe data in, describe what
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
| Debian/Ubuntu x64 | `linux-amd64.deb` | `sudo apt install ./zistral-v0.4.0-linux-amd64.deb` |
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
Responses display after completion; token streaming is not implemented.

Piped/redirected stdout remains plain result data. `--json` stays strict JSON; `--plain` disables
decorations and progress, and `NO_COLOR=1` disables color. Progress uses stderr. Terminal control
sequences in model output are removed when rendering rich terminal output.

## Portable archives and source builds

Portable archives contain one executable plus documentation. Extract zi (zi.exe on Windows) to a directory on PATH. No background service or automatic updater is installed.

Build from source with Rust 1.85+ (a current stable toolchain is recommended):

```sh
cargo install --git https://github.com/ax2/zistral --tag v0.4.0 --locked
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
  are limited to 16 MiB, requests to 8 turns (`--max-steps`), and each request to
  120 seconds (`--timeout`). The whole input is buffered and sent to the model;
  this is not a streaming or automatic chunking CLI.

```sh
cat data.json | zi --json 'Convert to a JSON array containing only names' | jq .
zi --no-tools --timeout 30 'Give a short explanation of Rust ownership'
zi --help
```

## Scope and privacy

The agent can reason over piped input and call two deterministic, read-only
tools: `count_input` and `select_input`. Agent tasks cannot execute shell commands,
modify files, inspect the filesystem beyond the selected input/config, or
browse the web. Nothing is persisted as conversation history. Network mode
sends the instruction and input to your configured provider, whose retention
policy still applies. Responses requests set `store: false`. API error bodies
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
They verify actual subprocess pipes, real terminal configuration/import sessions with
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
