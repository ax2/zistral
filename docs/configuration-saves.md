# Configuration save conflicts (unreleased main)

Opening `zi config` captures the configuration as it existed at startup. If a
second terminal saves a change before that wizard finishes, the old wizard exits
nonzero with `configuration changed while editing; nothing saved`. The newer
file is preserved byte for byte, including deleted keys. Restart the wizard to
reload it; zi does not merge or silently retry old settings.

The same protection covers `zi use`, profile rename/removal, key-reference and
saved-key edits, and output-budget edits. Unreleased preset-library imports use
the same OS lock for their own destination and recheck the captured library before
publication, preserving a concurrent successful import. Their preview stays
read-only; see [preset import](preset-import.md). Creation, deletion, comment-only edits
and replacement with invalid bytes also count as changes. Read-only commands
such as `models`, `doctor` and `config --list` do not create coordination files.

## One small coordination file

A save creates or reuses a sibling named `<configuration filename>.lock`, for
example `config.toml.lock`. It stays on disk and is always empty. It contains no
key, profile snapshot, PID or expiry timestamp; it is neither a backup nor task
history. Unix creation uses mode 0600, as with the configuration itself.

On Windows zi holds an exclusive file handle; on Unix it holds a nonblocking OS
file lock. Two cooperating zi writers using the same local configuration and
coordination path cannot publish simultaneously. The original bytes are checked
again under that lock immediately before the existing atomic file replacement.
A busy writer exits nonzero with `configuration is busy`; retry after the other
save finishes. No lock is held while waiting for interactive answers.

Closing or killing the lock-holding process releases the OS lock automatically.
Do not delete the empty file while writers may be running: replacing its identity
could let processes lock different files. There is no stale-PID cleanup or
background process. An existing nonempty file, directory or symbolic link at the
coordination path is rejected without truncating or replacing it; inspect such
an entry manually before using that path again.

## Scope

This is cooperative local-file protection, not a distributed lock or filesystem
transaction. Editors and older zi versions do not honor this lock. External
changes detected before the final check are preserved, but an uncooperative
writer changing the file after that check cannot be universally serialized.
Path aliases with different coordination names and network/synchronized
filesystems are outside the same-path local guarantee.

The existing serialization rules remain: successful writes preserve supported
settings and reserialize TOML formatting/comments. Save diagnostics contain no
configuration values. Task output, pipes, model calls and installed executables
are unrelated to this coordination file. No dependency, configuration field,
daemon or agent tool is added.
