# Common task scenarios and explicit builtin references

Since v0.23.0, two more reusable task instructions are bundled with zi:

```sh
zi -s builtin:triage -i app.log 'Use Chinese; separate evidence from hypotheses'
git log --oneline v0.22.0..HEAD | zi -s builtin:release-notes --var audience=users
zi presets show builtin:triage
zi presets render builtin:release-notes --var audience=operators
```

Actual model tasks require configured service/model credentials. List/show/render/export
remain offline. These presets are instructions, not new deterministic analyzers or a model
accuracy guarantee. Use the existing native count/JSON tools for exact computations.

## What the scenarios ask for

| Reference | Intended task | Limits of the instruction |
| --- | --- | --- |
| `builtin:triage` | Organize supplied logs into observations, hypotheses, unknowns and prioritized next checks | Cite only supplied snippets/timestamps; do not invent causes/files/metrics or claim unperformed checks |
| `builtin:release-notes` | Turn an explicit diff/change list into concise notes for an audience | Keep only supported user-visible/breaking changes; do not invent versions, dates, issue links, test/deployment results or the target range |

Release-note audience defaults to `users`; `--var audience=VALUE` changes only this task.
The requested output language follows the additional instruction, otherwise input language.
If a change list includes several historical ranges without a clear target, the prompt asks
for missing scope rather than choosing an arbitrary release. Review the model's draft.

Use a narrow diff or log range as input. This preset does not run git, open log files on its
own, fetch issue pages or deterministically extract a changelog chapter. `-i`/pipes supply
the data, and the selected provider still receives task input. No browsing or hidden sources
are introduced.

## Keep existing user names

The new canonical names contain `builtin:` so valid older libraries with custom `triage`
or `release-notes` entries keep working. They are **not** new reserved short names:

| Request | Resolution |
| --- | --- |
| `-s triage` | Your custom entry, when defined; otherwise unknown preset |
| `-s builtin:triage` | The bundled triage definition |
| `-s release-notes` | Your custom entry, when defined; otherwise unknown preset |
| `-s builtin:release-notes` | The bundled release-note definition |
| `-s summarize` or `-s builtin:summarize` | The existing summarize builtin |

The original seven short names remain protected as before: summarize, translate, extract,
explain, review, commit and rewrite. They all also accept explicit `builtin:NAME` references.
The catalogue lists nine definitions, not one extra entry per alias. Qualified references
never fall back to an arbitrary user definition: `builtin:team` does not select custom `team`.

`builtin:` must use this exact spelling and a known builtin name. The suffix uses the usual
1–64 ASCII letter/digit/hyphen/underscore name rules; nested prefixes and path syntax fail.
User definition/export names still cannot contain a colon. Model profile names are unchanged.
An explicitly selected `--presets FILE` is still loaded and validated, even with a qualified
reference: this syntax does not hide an invalid library or bypass the original name protection.

## Inspect and customize without capturing data

```sh
zi presets render builtin:release-notes
zi presets export builtin:release-notes --name team-notes --var audience=operators -o notes.toml
zi --presets notes.toml presets show team-notes
zi --presets notes.toml -s team-notes -i changes.txt 'Only this range; use Chinese'
```

The exported version-1 TOML retains `{{audience}}` and the literal default value. Another
`--var` temporarily overrides it; no task input, model key, endpoint, output path or permissions
are added to the file. A short custom name such as `triage` can be chosen for a copy.
Export remains one definition, with the existing no-clobber / explicit whole-file replacement
rules. It does not install or merge libraries.

Old versions do not recognize the new qualified lookup syntax or contain these two presets.
Exported ordinary version-1 definitions use the existing format and can be loaded by v0.22.0.
The definition itself is portable; availability and output quality of a selected model are
separate questions.

## Inherited boundaries

- The same preset resolver serves show/render/export and model tasks. Qualified `extract`
  retains `json=true`: complete JSON validation, protected file delivery and explicit-stream
  rejection still apply. The two new prose scenarios have `json=false`.
- Presets do not grant filesystem/shell access, execute checks by themselves or enforce a
  security sandbox. Existing explicit project flags and `--no-tools` retain authority.
- Each task remains independent. No chat history, task memory, background watcher or model
  configuration migration is added.
- Existing limits remain: 256 KiB user library/export, 64 custom entries, 16 KiB template,
  12 variables, 4096 UTF-8 bytes per value and 128 KiB rendered instruction including additions.
- The same chat/responses/anthropic request paths, stdout/stderr split, pipe behavior and
  single executable are reused, with no new packages, tool names or runtime dependencies.

Protocol tests verify selected instructions, input separation, variables, tool capabilities
and final-result handling using controlled services. They do not establish a general diagnostic
accuracy score or guarantee factual release notes from every real model.
