# Imported API-key references (unreleased)

This change is on unreleased `main` for the next grouped release. v0.33.0 imports
a resolved source key as a saved literal. New imports now preserve the selected
environment-variable reference when it supplies the key, avoiding a plaintext
snapshot in zi's configuration and allowing rotation between invocations.

```sh
zi config --list
zi config --name work
zi -p work doctor --json
```

Discovery lists `Key environment` for an available environment-sourced credential.
The wizard keeps its existing choice flow and skips key entry when a usable key
is available. A selected reference saves `api_key_env = "VARIABLE_NAME"`, not the
variable's value in `api_key`. Model, service, protocol and supported exact
endpoint import behavior are unchanged. Existing named profiles/root settings
are preserved, and only the replaced profile's budget preferences are reset.

## Recognized sources

| Source | Existing recognized environment form |
| --- | --- |
| Codex | Provider `env_key`; environment syntax in `experimental_bearer_token`; effective `OPENAI_API_KEY` fallback for providers using OpenAI auth |
| WorkBuddy/CodeBuddy | `apiKeyEnv`; `$NAME` or `${NAME}` in `apiKey` |
| dsh | A usable provider `apiKeyEnv`; environment syntax in `apiKey` or a supported plain credential-reference value |

This preserves the **selected credential source**, not an entire upstream fallback
chain. Existing lookup order is unchanged. If an environment reference is absent
or blank during discovery and a supported literal fallback is available, that
literal is still imported. If no usable key is available, the wizard prompts for
a hidden key as before. A supplied fallback is saved only because it was entered
explicitly; the environment value itself is never saved by reference import.

Codex `auth.json` API-key fields, WorkBuddy literal keys and supported dsh API-key
credential records remain literal imports. Account/browser/ChatGPT subscription
tokens are not treated as API keys; command authentication, encrypted references
and unsupported extra request settings are not newly enabled by this change.
Source-agent files and credential stores are read only and never modified.

## Rotation and missing variables

Set the same variable in the terminal/environment that launches zi. A later
invocation reads its current value; changing a parent environment does not alter
an already running process. Long-running tasks use their effective settings
snapshot rather than changing credentials midway through a tool loop.

If the variable is later missing/blank, zi does not retain a hidden copy of the
old environment value. Doctor reports the current winning key source/presence;
official service checks can require a key, while custom services may allow none.
This is not a live watch of another agent's auth file or secret store. Re-import
explicitly to pick up changes in a literal source credential or provider config.

Normal precedence remains `ZI_API_KEY` > the profile's named variable (default
`OPENAI_API_KEY`) > a saved literal. An intentional global override still wins.
Doctor quotes variable names safely and never prints values. References remain
ordinary profile settings and follow rename/remove behavior. No new configuration
field, key vault, command, dependency, background watcher, or agent tool is added.

## Verification

Real-terminal tests cover seven reference forms, hidden-key prompts for three
missing-source cases, and literal-fallback precedence. Imported named profiles
retain only the variable name; root/unrelated profiles and source files stay
unchanged. Two controlled requests per form use initial and rotated key values;
then an offline report with an empty variable confirms no stale saved-key fallback.
Custom references are tested with an unrelated `OPENAI_API_KEY` present, confirming
that it does not replace the profile's selected variable.
These synthetic loopback checks do not establish real account access or model quality.

The native installed-service workflow also verifies source discovery under all
four protocol variants: 36 checks and the same 24 loopback requests. Its source
files contain only declared variable names. Python remains a test-host dependency;
installed zi is still one executable with no additional runtime.
