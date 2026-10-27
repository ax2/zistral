# Choose a task mode explicitly

```sh
zi --count --json -i notes.txt
zi --select /id --json -i data.json
zi --config-example -o example.toml
zi data --show-plan --step trim -o cleanup.toml
zi -C . --read-only -s review 'Check the supplied project'
```

`--config-example` prints/saves a TOML template, not processed input or a model answer.
It conflicts with `--json`, `--count`, `--select`, explicit `--input` and an instruction.
These combinations fail instead of silently choosing template output. Existing preset,
stream and subcommand conflicts remain enforced. To export a template, use the flag alone
with optional output settings. Template contents remain generic, not derived from model
overrides, a profile or input data.

`--count --json` and `--select POINTER --json` remain valid. They are deterministic data
tasks; JSON validation is applied to their results. The local selection string-display
contract and JSON precision rules remain unchanged.

Project selectors are `--agent`, `-C/--cwd`, `--read-only` and `--no-exec`. They enable or
constrain project tools for model tasks. They are rejected for every offline or management
mode: count/select, config-example, data (including --show-plan), config, models, use,
doctor (including --probe), and presets. Pass explicit input paths to offline tasks instead
of `-C`, which is not a general-purpose process-directory switch.

The guard checks requested selectors, so adding `--no-tools` does not hide an invalid
project/offline combination. In an actual model task, combining project selectors with
`--no-tools` remains supported: no tools are advertised or executed, and the project root
is not opened. No permission or Agent-loop policy is changed.

Conflicts are checked before data reads, output preparation, config loading, import/setup
or model requests. Invalid calls do not wait for stdin EOF or publish results; existing
`-o` targets are preserved, including explicit --force. Shell `>` redirection is managed
by the shell independently and can still truncate files before zi starts. Parser errors
and runtime mode errors may use different nonzero exit codes; treat both as failure.
Argument diagnostics may repeat option names/command-line values; input data is not read
for these rejected calls.

Inherited valid model environment settings do not turn offline commands into model tasks.
They remain usable without loading the saved model config or contacting a service. This
fix concerns operation selectors and conflicting template input/output modes; it does not
redefine all benign shared flags, environment parsing or existing numeric option validation.

No new commands, operations, dependencies, model tools, config migrations or memory files.

Since v0.19.0, only `presets export NAME --name CUSTOM_NAME` also supports the shared
`-o/--output` writer. It produces reusable TOML after validating names/defaults, ignores
piped stdin and rejects explicit input/model instructions, JSON/stream flags and project
selectors. Other management actions still reject output files. See [preset export](preset-export.md).

Since v0.27.0, --exact is an explicit model-data delivery mode, compatible with --json,
presets, file output and normal request/turn limits. It rejects --stream, --no-tools,
project selectors, --count, --select, --config-example, offline and management commands
before configuration/input/output. It never silently becomes a text-only or project task.
See [exact data results](exact-results.md).
