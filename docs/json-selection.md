# Exact JSON selection and final validation

```sh
zi --select /abc/0 --join ',' -i payload.json
zi --select /id --json -i payload.json -o id.json
cat notes.txt | zi -s extract -o result.json 'id, amount, next_action'
```

`--select` and the existing `select_input` model tool now borrow the selected JSON
text rather than build a serde_json::Value tree. Oversized integers, `1.2300E+400`,
`-0`, object member order and duplicate keys inside selected values remain intact.
Unselected valid numbers outside machine ranges no longer prevent selection.

The complete input is syntax/depth checked first. Traversal resolves only the reference
path, ignores other values without numeric conversion, and checks referenced object
member names for ambiguity. Raw text is borrowed; the selected subtree is not copied
into a numeric value tree. Only one current join item and the bounded result are rendered.

Strings still return decoded text without JSON quotes. Other selected values return
compact JSON, preserving numeric/string tokens and object key order. `--join` requires
an array and joins each item's displayed value: strings decoded, others compact JSON.
It is text joining, not array/object merging. Empty arrays/strings keep their existing
display/newline behavior. A decoded selected string must itself contain valid JSON
when combined with `--json`; that option does not add quotes around plain text.

Pointers use the plain [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) form: empty
selects the root, `/` selects an empty object key, `~0` decodes tilde and `~1` slash,
with a single escape pass (`~01` becomes literal `~1`). URI fragment `#/...` forms
are not accepted. Unicode names compare exact code points without normalization.
Array indices are zero or unsigned decimal without leading zeros. `-`, signs,
overflowed indices and missing values fail. Numeric-looking object keys are literal.

A referenced duplicate object member fails, including equivalent decoded keys such as
`"a"` and `"\u0061"`. The previous implementation silently chose the last member;
do not depend on that behavior. Unreferenced duplicate names do not invalidate an
otherwise usable reference. Selecting a whole container preserves its repeated keys.
This is not a global duplicate-key validator or canonical JSON serializer.

Limits: pointer 4096 UTF-8 bytes and 128 tokens, 128 JSON containers, input 64 MiB
subject to the caller's normal `--max-input` (8 MiB by default), local result text
64 MiB, tool text 64 KiB. Join separators count toward the result cap. File output
retains its separate 64 MiB limit including the final newline. Invalid input, ambiguous
paths or output growth return an error without partial stdout/file publication.
Initial input BOM normalization and existing file-overwrite/concurrency rules remain.

`--json` final validation now checks syntax and removes only insignificant whitespace,
in one-shot tasks and interactive answers. It does not parse numbers into machine types
or reorder/deduplicate objects. It preserves escape forms and is not schema validation,
numeric arithmetic, duplicate-key validation or a model factual-accuracy guarantee.

Implementation uses existing serde_json `raw_value` and `unbounded_depth` features.
The parser recursion limit is disabled only for the selector after the explicit depth
check; network JSON parsing retains its existing limits. No new packages, tool names,
project permissions, hidden file reads or memory/session features are added.
