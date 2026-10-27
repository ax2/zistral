# Export a reusable task preset

Start from a builtin or an existing custom preset, give the copy a custom name, and save
a version-1 TOML library that the existing `--presets` option can load:

```sh
zi presets export translate --name translate-ja --var language=Japanese -o team-presets.toml
zi --presets team-presets.toml presets show translate-ja
zi --presets team-presets.toml presets render translate-ja
zi --presets team-presets.toml -s translate-ja -i notes.txt
```

Only the final model task needs model configuration. Export/show/render are offline; they
do not read piped input, send requests, import model credentials or create model configuration.
Use `--presets SOURCE.toml` before the command to choose a source library. Otherwise the
existing default library beside the selected config is used, together with the seven builtins.

The source name is positional. The required `--name CUSTOM_NAME` gives the exported entry
an explicit name: 1–64 ASCII letters/digits/underscores/hyphens, using the existing naming
rules. Builtin names cannot be used for the destination. No automatic install or implicit
project discovery is added; select the exported file explicitly or edit your own library.

## Export is different from rendering

`presets export` retains the original description, template and JSON-output flag. Existing
defaults remain; repeated `--var NAME=VALUE` arguments replace only the specified defaults.
They must refer to variables used by that template. Unknown/duplicate names, empty values
and invalid controls fail, without echoing values. Values are literal text: a value containing
`{{example}}` is not recursively expanded. The template itself keeps `{{language}}`.

`presets render` resolves variables to preview the actual prompt. A required variable may
remain unresolved in an exported definition; supply it when rendering or running the task.
Export validates the definition, not data-dependent model success or every future render's
size. Render/task limits and the additional-instruction behavior remain unchanged.

For the first command above, the file contains one entry:

```toml
version = 1

[presets.translate-ja]
description = "Translate text into the selected language (--var language=VALUE)"
prompt = "Translate the supplied input into {{language}}. Preserve meaning, formatting, code, URLs and identifiers; return only the translation."
json = false

[presets.translate-ja.defaults]
language = "Japanese"
```

The description is copied verbatim from the source; edit it when customizing. Saved defaults
and `presets show` determine the copy's actual configuration. Custom descriptions that mention
old defaults may also need editing; export does not infer or rewrite their meaning.

JSON presets retain `json = true`; an exported `extract` copy still validates the complete
answer as JSON and rejects explicit `--stream`. Presets never grant file/shell capabilities.
Supplied input remains separate from template variables, and each task remains independent.

## Result delivery and boundaries

Without `-o`, export prints reusable TOML on stdout, with diagnostics on stderr. `-o -`
also selects stdout. To save a file, use `-o FILE` with an existing parent directory. Existing
targets are rejected unless `--force` explicitly allows replacement after success; links,
directories and other unsupported targets follow the shared result writer's rules.

Exactly one definition is exported. This is not append/merge or a preset-package installer.
If `-o` points to an existing library, `--force` replaces that entire file, including other
entries. Choosing the source file itself as the forced output also replaces the source.
Use a separate path when creating a copy. Shell `>` truncation and concurrent-writer/file
metadata limits remain as documented for [result saving](../README.md#save-complete-results-without-truncating-a-file-first).

`-i/--input`, model instructions, `--json`, `--stream`, model-task project selectors and
nonzero `--retries` are rejected. Other preset management actions still reject `-o`.
Definition/name/default validation and serialization finish before preparing a result file.
Unknown fields in source libraries are rejected; no API keys or model-service fields are
added to exported definitions. Prompts/defaults are ordinary user-maintained text, not a
credential store or memory of previous tasks.

Existing limits apply: 256 KiB library/export, 64 custom entries on loading, 16 KiB template,
12 variables, 4096 UTF-8 bytes per default and 128 KiB rendered prompt including additional
instructions. Existing libraries remain valid; the exported version-1 definition can be
loaded by earlier zi releases that support custom presets. The export command itself
requires v0.19.0 or later. No dependencies, tools, permissions, model protocols or memory added.
