# Save the latest interactive result / 交互结果导出

Start `zi` in a terminal and complete a task. Then save the result you have just read:

```text
/save notes.md
/save! notes.md
/clear
```

`/save PATH` creates a new result file without overwriting an existing one. `/save! PATH`
explicitly permits replacing an existing ordinary writable file. Both reuse the same
protected output writer as one-shot `-o`: target checks, change detection, temporary
staging and successful publication. No model request is needed for either command.

The commands exist only inside the interactive prompt. To select a file before starting
a one-shot task, continue to use `zi 'instruction' -o result.txt`. Pipes and redirected
stdout keep their existing plain output; a one-shot instruction beginning `/save` is
still an ordinary model prompt, not an interactive command.

## What is retained, and when

The interactive process holds **one latest successful result, at most 16 MiB of UTF-8
bytes**. It does not retain a list of answers, inputs, tool observations or credentials.
This result is for explicit file export only: it is never added to later model requests,
sent on model switching, written to a history file or loaded by another process.

- A new task discards the previous result **before** preset validation or execution.
  Invalid presets, provider/network failures, incomplete streams, exhausted budgets or
  invalid JSON therefore leave no result available; `/save` cannot silently use an old one.
- `/clear` clears the display and retained result. `/config` clears it before configuration,
  and successful `/use NAME` clears it, including reselecting the current profile.
- Successful `/help`, `/presets`, `/models` or local `/doctor` inspection leaves it available.
  A failed save also leaves it available so you can correct the target and retry.
- Saving can be repeated to multiple explicit paths. Process exit drops the in-memory
  result. There is no implicit autosave, cache directory or conversation history.

This is bounded result retention, not a conversational session or long-term memory.
Each new task still creates a fresh Agent/project session with the existing permission,
read-revision and edit/undo boundaries. Persistent/semantic memory and automatic reuse of
past answers remain deferred; explicit presets and recipe files continue to carry reusable
definitions rather than past model conclusions.

## Exact file content

The file contains the **complete final answer**, with no welcome panel, status line,
token counters, tool messages, terminal decoration or `/save` command text. A missing final
LF is added, matching ordinary `-o` behavior; internal whitespace and line endings remain.
Model-provided text is otherwise saved as returned, like `-o`; terminal display sanitization
does not rewrite the exported answer. Markdown remains Markdown, not a screenshot or HTML.

Complete streamed answers can be saved after completion without redisplaying them. Text
that appeared before a failed stream is not a successful result and cannot be saved with
these commands. In `zi --json`, each answer must first pass the existing complete-JSON
validation; its compact lexical result is retained, preserving numeric/string tokens and
object order under the existing formatter contract. Invalid JSON clears availability.

An oversized completed result may still have been displayed, but is not retained and emits
a diagnostic instead. The limit does not truncate an answer silently. Existing API response
and file size limits still apply.

## Literal paths and file boundaries

The entire trimmed remainder after `/save ` or `/save! ` is **one unquoted OS path**.
Internal spaces, Unicode and shell punctuation are literal. There is no tokenization,
quote removal, variable or tilde expansion, globbing, redirection or shell execution.
For example, `/save release notes.md` selects that one filename; adding quotes would make
the quote characters part of the supplied path (and is invalid in Windows filenames).
Leading/trailing path whitespace cannot be specified with this grammar.

Relative paths use the process working directory, like `-o`; `-C` selects model project
tools and is not a general process-directory switch. The parent directory must already
exist. `-` is rejected rather than treated as stdout or an implicit filename. Windows
device/alternate-stream names, directories, symlinks and read-only targets retain the
existing output writer's restrictions. Existing files above 64 MiB are not replaced.

Operator-selected export paths do not add filesystem tools or permissions to a model;
they are explicit CLI output choices. `/save!` permits replacement, not bypassing target
type/revision/permission checks. Files changed by other processes can cause publication
to fail. Normal shell redirection and other processes remain outside this protection.

Source: [interactive state](../src/main.rs), [single-result holder](../src/last_result.rs),
[existing output publication](../src/output.rs). Real-terminal tests cover all three model
protocols, complete/partial streams, JSON validation, no-clobber/force, path spaces, reset
boundaries and unchanged independent request histories.
