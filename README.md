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

## Install

Download the archive for your platform from **Releases**, extract `zi` (`zi.exe`
on Windows), and put it in a directory on your `PATH`. Verify the archive using
the accompanying `SHA256SUMS.txt`. The first release provides Windows x64,
Linux x64, and macOS Apple Silicon binaries. macOS Intel users can build from
source. Binaries are unsigned; no installer, background service, or auto-update
is installed.

Build from source with Rust 1.85+ (a current stable toolchain is recommended):

```sh
cargo install --git https://github.com/ax2/zistral --tag v0.1.0 --locked
```

The package is named `zistral`; the installed command is `zi`. It is not yet
published on crates.io. If your shell already uses `zi` (for example as an
alias), use the full executable path or rename the downloaded executable.

## Configure a model

Choose a model available from your provider. No model name is hard-coded.

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
tools: `count_input` and `select_input`. It cannot execute shell commands,
modify files, inspect the filesystem beyond the selected input/config, or
browse the web. Nothing is persisted as conversation history. Network mode
sends the instruction and input to your configured provider, whose retention
policy still applies. Responses requests set `store: false`. API error bodies
are not echoed because they may contain private input. HTTP redirects are
rejected rather than forwarding credentials to another endpoint.

## Development and releases

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Integration tests use local mock HTTP servers and require no paid API access.
They verify actual subprocess pipes, tool round trips for both protocols,
Unicode input, invalid input, JSON validation, errors, and step limits. They
do not prove any particular provider/model's quality or compatibility.

CI runs tests on Windows, Linux, and macOS. Releases build all three platforms,
test the binaries, attach archives and SHA-256 checksums, and publish only when
every build succeeds. See [CONTRIBUTING.md](CONTRIBUTING.md).

MIT licensed. Protocol reference: [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling).
