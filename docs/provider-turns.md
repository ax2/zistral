# Check a provider turn before tool dispatch

zi receives a complete JSON reply or assembles an SSE turn before executing tools. v0.17.0
adds shared checks for supported protocol metadata so a malformed later call cannot cause
an earlier valid file/shell action to run first. This does not add tools or change their permissions.

## Completion and content

- Chat final text requires `finish_reason: "stop"` and no nonempty tool batch. Tool turns
  require `finish_reason: "tool_calls"` and a nonempty batch. Missing/null/unknown reasons,
  length/filter stops and unsupported legacy `function_call` replies fail. A non-array
  `tool_calls` field fails; absent/null/empty arrays remain valid for a final text turn.
- Anthropic final turns retain `end_turn`/`stop_sequence`; `tool_use` requires calls.
  A text stop with calls, or a tool stop without calls, fails before dispatch.
- Responses still requires `status: "completed"`. Message content must be an array;
  refusal parts and malformed `output_text` are checked throughout output before any call,
  including a message appearing after a function-call item. Reasoning/output items remain
  available for the existing stateless continuation and pending-plan behavior.
- Supported text fields must have their expected shapes. Chat/Anthropic malformed text
  cannot be silently added to history alongside tool calls. This is not a general validator
  for all future provider fields, modalities or unsupported tool types.

Providers/proxies must supply the supported completion metadata; zi does not infer success
from HTTP 200 plus some text. Check protocol/endpoint compatibility if a service omits it.
Incomplete or malformed non-streamed replies fail with no final stdout or published `-o`
result. Existing targets remain unchanged, even with `--force`; temporary output is cleaned.

## Whole-batch call metadata

Before updating tool-call history or invoking the first tool, check all calls (at most 32):

- Chat/Anthropic `id` or Responses `call_id` must be nonblank strings, unique within this turn.
- Names must be nonblank strings. Unknown nonblank names remain normal tool errors.
- Chat/Responses `arguments` must be a supplied string. Anthropic `input` must be a supplied
  JSON object. The native tool still checks JSON parsing, allowed keys, value types and limits.

Checks do not echo offending IDs, names, input text or provider refusal bodies. IDs are not
tracked globally: a later turn can reuse an ID, and observations preserve each turn's IDs
and order. A valid 32-call turn remains supported. No cross-task/session memory is introduced.

A protocol error fails this task without replaying the turn or invoking any of its calls.
Existing HTTP retry policy is unchanged. Session initialization/instruction reads and earlier
valid turns may already have occurred; this is not a claim that the whole task performed no IO.

## Native errors can still be repaired

An explicitly supplied empty/invalid JSON argument string, a wrong tool property type,
unknown tool or permission denial is a native tool error after valid call metadata. It is
returned as an observation to the model, which can correct the next invocation. A later native
tool failure does not roll back earlier valid calls in that batch or previous turns. Existing
read-before-edit, conflict checks, command limits, plans, undo and tool permissions remain.
`--no-tools` still rejects provider tool requests. No automatic retry of file edits or commands.

## Streaming

Chat assembly keeps missing arguments as null until an actual string fragment arrives.
An explicit empty fragment marks a supplied string; no fragment does not. Missing IDs/names
are rejected after assembly; wrong nonnull fragment types are rejected while decoding.
Null placeholders in optional deltas can wait for later fragments. Tool side effects occur
only after a terminal event and the same turn checks used by buffered replies.

With explicit `--stream`, some text may already have reached stdout before a malformed later
call, refusal or incomplete reply is detected. zi exits nonzero and does not execute that
turn's tools, but cannot retract visible bytes. Normal pipe output remains buffered by
default, file output does not stream, and shell `>` truncation is outside zi's protection.

Regression fixtures invoke the actual CLI against a controlled local provider in all three
protocols and both stream modes. They check file and shell markers, target preservation,
late refusal/text errors, 32 correlated results, recoverable native errors and ID reuse.
This is protocol/execution evidence, not an evaluation of a live model's task quality.

References: [Chat completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[function calling](https://developers.openai.com/api/docs/guides/function-calling),
[Anthropic tool-result association](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls).
