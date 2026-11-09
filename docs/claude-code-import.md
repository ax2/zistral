# Import Claude Code API settings (unreleased)

The development build of zi can import personal Claude Code settings for services
that accept Anthropic Messages requests with an `x-api-key`. Use the same wizard
as for the other model sources.

```sh
zi config --list
zi config --name claude
zi -p claude doctor --json
```

The list is a local preview. Selecting the source in the wizard saves a zi model
profile. Both operations leave the source files untouched and make no API call.

The importer looks for `settings.json` in `CLAUDE_CONFIG_DIR` or `~/.claude`.
For ordinary terminal settings, the file's `env` values take precedence over
the corresponding shell variables. An empty value masks its shell counterpart.
`ANTHROPIC_MODEL` takes precedence over the top-level `model` field. Choose a
concrete provider model ID. The `sonnet`, `opus`, `haiku` and `fable` aliases
require an explicit `ANTHROPIC_DEFAULT_*_MODEL` mapping. The importer skips
dynamic aliases and bracketed context modifiers.

`ANTHROPIC_BASE_URL` defaults to `https://api.anthropic.com`. The importer
preserves it as a path prefix for `/v1/messages`. For example, a root of
`https://gateway.example/team` yields
`https://gateway.example/team/v1/messages`. An existing `/v1` in the root is
part of that prefix.

A file's `env.ANTHROPIC_API_KEY` is treated as literal text. A key inherited from
the shell is saved as the name `ANTHROPIC_API_KEY`, so rotation works on later
invocations. If neither the source nor [zi's effective credential settings](import-key-references.md)
provides a key, the wizard prompts with hidden input.

The supported scope is the personal settings file and relevant terminal
variables. Claude account credentials, project and managed settings, cloud
authentication, bearer tokens, credential helpers and custom transport
requirements are outside this import scope.

See the Claude Code documentation for [settings precedence](https://code.claude.com/docs/en/settings),
[environment variables](https://code.claude.com/docs/en/env-vars),
[model selection](https://code.claude.com/docs/en/model-config) and
[gateway authentication](https://code.claude.com/docs/en/llm-gateway-connect).
