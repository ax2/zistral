# Maintain API-key settings offline (v0.34.0)

These commands edit an existing zi configuration/profile;
they do not create a profile, start a wizard or contact a model.

```sh
zi config --name work --api-key-env WORK_API_KEY
zi -p work doctor --json
zi config --name work --clear-saved-key
```

The first command saves only `api_key_env = "WORK_API_KEY"`. It does not read the
variable or copy its value into the configuration. The variable may be unset
during setup, allowing the reference to be prepared before credentials are
available in a later launching environment. Set the variable using your shell's
normal environment mechanism; a running zi process does not receive changes to
its parent's environment.

Changing a reference **keeps an existing saved literal fallback**. Removing that
fallback requires the explicit `--clear-saved-key` option. Both can be requested
atomically:

```sh
zi config --name work --api-key-env WORK_API_KEY --clear-saved-key
```

Clearing alone removes only the selected profile's `api_key`; it retains the
reference. It does not unset any variable, revoke a provider key, edit another
agent, remove other copies, clear running tasks or securely erase disk backups.
No credential backup/history file is created. Use `--api-key-env OPENAI_API_KEY`
to explicitly select the usual default variable again; no separate reset flag
is needed.

Effective request precedence remains `ZI_API_KEY` > selected variable (default
`OPENAI_API_KEY`) > saved literal. An unrelated `OPENAI_API_KEY` cannot substitute
for a different selected variable. After clearing the literal, a missing/blank
selected variable means no key unless the global override is set. Official
OpenAI/Anthropic hosts require a key; a custom/local service may accept none.
Inspect `doctor` without `--probe` to check the source offline; these edits do
not verify account permissions, connection, model compatibility or billing.

## Target selection and boundaries

The target is `--name` > `--profile` / `ZI_PROFILE` > saved active profile > root
`default`. `--name` works even when a different invocation profile is present.
The saved active selection never changes. Service/model/protocol, exact endpoint,
budget preferences, root and unrelated profiles remain unchanged. Credential
metadata can be cleaned up even when a profile's generation settings are obsolete.

New CLI variable names are 1–128 ASCII letters/digits/underscores, starting with
a letter or underscore. Pass the name (`WORK_API_KEY`), not `$WORK_API_KEY`,
`${WORK_API_KEY}`, an assignment or a key value. Existing nonportable imported
references remain readable; clearing a saved literal does not rewrite them.
Name comparisons follow the operating system's environment semantics.

The two options can be combined with each other, but not interactive setup,
source listing, rename/remove, output-budget edits or task input/output/project
modes. They never read stdin. Success exits zero, with empty stdout and a short
stderr notice. Invalid names/options/profiles, unreadable or invalid typed TOML,
and missing configuration fail without publication. Saving uses the existing
atomic writer and Unix 0600 permissions. Other known values are retained;
TOML formatting/comments are reserialized, as with other config edits.

No runtime dependency, config field, agent tool, permission, secret vault,
automatic migration or background service is added. References imported from
Codex/WorkBuddy/dsh keep their existing [source selection](import-key-references.md).

## Verification

Actual-CLI tests compare the entire parsed document after target selection,
reference changes and explicit clearing; they also cover fallback/global
precedence, missing variables, protected unrelated fields, invalid/no-write
operations, permission mode and open stdin. The shared installed-service check
uses all four protocol variants: after changing the reference, the old variable
remains present with a wrong value and later controlled requests must authenticate
with the new one. Forty workflow checks retain the same 24 synthetic loopback
requests. These checks do not establish access to a real provider account.

Configuration writes also use [save-conflict protection](configuration-saves.md):
reload after a stale-save error; an empty sibling `.lock` file coordinates local
writers without storing keys or keeping a background process.
