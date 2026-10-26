# Explicit request retries / 显式请求重试

```sh
cat notes.txt | zi --retries 2 -s summarize -o summary.md
zi --retries 1 --verbose "Explain this input"
zi --agent --retries 1 --max-steps 8 "Fix the failing test"
```

`--retries` accepts 0–3 extra attempts per model turn; the default is 0. There is no
environment/config setting or implicit retry preference. Put the option before a trailing
natural-language instruction. Each independent interactive task inherits the CLI setting.

Only HTTP 429, 500, 502, 503, 504 and 529 are eligible, before a successful response is
accepted. The current request body is reused exactly, including its existing tool results;
the retry does not rebuild history, run tools again or consume another logical model turn.
`--max-steps` limits turns; each turn has at most `1 + retries` HTTP attempts. The existing
`--timeout` applies to each HTTP attempt, not the full task. Waits add to total task time.

When present, one `Retry-After` header may contain nonnegative integer seconds or an
HTTP-date. Dates are interpreted against the local UTC clock; past dates mean no wait.
Each delay is limited to 30 seconds. A longer, invalid, overflowed or duplicate hint ends
the task without sending another request, rather than retrying before the server's hint.
Without the header, delays are 250, 500 and 1000 ms. There is no global/shared rate limiter,
jitter, durable resume, scheduler or background retry service.

Connection/TLS failures and timeouts are not retried: the client cannot infer whether the
provider processed a request. After successful HTTP headers, malformed JSON, read errors,
truncated/SSE errors, refusal or invalid final JSON also stop. Partial explicitly streamed
text may already be visible and is never replayed. A later model turn may retry its own
transient HTTP error without replaying tools or text from earlier turns.

Repeated POST requests can still incur provider charges or duplicate provider-side work,
even when the first response has an error status. Opting in is not an idempotency or
exactly-once guarantee. Retry does not switch providers/profiles, change credentials, add
permissions or undo project changes. Authentication, bad endpoints and unsupported requests
must be corrected rather than repeatedly sent.

Rich terminals show the retry ordinal, HTTP status and delay. Plain pipelines stay quiet;
`--verbose` sends those details to stderr. Progress uses logical turns, while the final
`requests` total counts actual HTTP attempts. Error bodies and raw Retry-After values are
never echoed. Failed default pipes emit no partial final answer; `-o` preserves the target
until complete success. Shell `>` remains independent of zi's output protection.

Nonzero `--retries` is rejected for offline/management commands, including `doctor --probe`;
diagnostics remain a single small request. Offline data/count/select/recipes never gain
network access. This option changes transport attempts, not model tool/error recovery.

The compiled-in `httpdate` 1.0.3 crate parses HTTP's three date forms without a runtime
program or extra dependencies of its own. Existing installation remains one executable.
References: [RFC 9110 Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html#name-retry-after),
[httpdate API](https://docs.rs/httpdate/1.0.3/httpdate/),
[Claude 529 overload status](https://platform.claude.com/docs/en/api/errors).
