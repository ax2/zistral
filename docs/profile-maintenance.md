# Maintain saved model profiles (unreleased)

These commands are on unreleased `main`, accumulating for the next grouped
release. Installed v0.33.0 does not include them. They maintain zi's own saved
named profiles offline; they do not contact providers or imported source agents.

```sh
zi models
zi config --rename work personal
zi use default
zi config --remove obsolete
```

Use explicit profile names as shown. Names follow the same 1–64 ASCII
letter/digit/underscore/hyphen rule and are case-sensitive on every platform.
`--profile` / `ZI_PROFILE` do not change the explicit rename/remove targets.
These commands never read stdin, start the setup wizard, or create a missing
configuration. Task input/output/project/budget modes and other config-edit
options cannot be combined with maintenance.

## Rename without changing services

`zi config --rename OLD NEW` moves the complete stored profile under a new name:
model, service, protocol, saved key/key reference, exact endpoint and optional
request-budget preferences stay together. If the saved active selection was OLD,
it becomes NEW; otherwise it stays unchanged. Other profiles/root settings remain
unchanged. NEW must not exist. Same-name renames, missing OLD and either name
being the reserved root `default` are rejected without writing.

External `ZI_PROFILE` values, scripts using `-p OLD`, open processes and other
references are not rewritten. Update those explicit references to NEW yourself;
the command reports that external references are unchanged. It does not reset
permissions or grant capabilities. Metadata changes do not validate a profile's
generation settings, so an obsolete/misconfigured profile can still be renamed.
A renamed invalid profile remains invalid for generation until repaired.

Renaming works at the normal 128-profile creation limit, since it does not add
a profile. A case-only rename is allowed if the target name does not exist.

## Remove an inactive named profile

`zi config --remove NAME` removes that complete named profile, including its saved
key or key reference, from zi's configuration. Root `default` settings cannot be
removed, and a saved active profile is protected. Run `zi use NAME` to select a
different valid profile first; deletion never picks a replacement automatically.
Unknown/invalid names and attempts to remove protected profiles leave the file
unchanged. An invocation-only `--profile` override does not alter this saved
default-selection protection.

Other profiles may still contain/reference the same key; they are preserved.
Removing a saved copy does not revoke the key at the provider, unset an environment
variable, clear an already running process or edit Codex/WorkBuddy/dsh settings.
No backup/history file containing the removed credential is created. This is a
configuration update, not secure deletion from disk backups or provider accounts.

## Saving and errors

Both commands require readable, valid typed TOML. Parse/type/unknown-field errors
do not print source lines or credential values. They use the existing atomic
configuration writer and restrictive Unix file permissions; no new configuration
fields, dependencies, tools or runtime are added. Successful saves reserialize
known TOML values, like other config edits; comments/formatting are not retained.
Unrelated values are preserved, rather than validating every stored profile.

Success exits zero with an empty stdout and a short stderr notice. Errors exit
nonzero without a success notice; failure checks cover unchanged source bytes.
Pipes remain available for subsequent commands, and no model query is implied.
The existing installed-service delivery check includes profile maintenance in
all four protocol variants, with no additional HTTP requests.
