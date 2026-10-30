# Pre-sharing boundary verification / 发送前准备的边界复核（main，尚未发布）

The candidate batch combines structural profiles, explicit redaction (including optional
known fields), and first-request inspection. Existing native/input/recipe contracts
remain bounded; no model tool name, runtime dependency, implicit history or permissions
were added. This note records a reproducible development check, not a release approval.

## Reproduce on Windows

```sh
cargo build --release --locked
python scripts/benchmark_pre_sharing.py target/release/zi.exe --runs 3 --output boundary.json
```

Use a current development executable containing the unreleased operations. Python and
Windows process counters are optional development tools; end users do not need them.
zi itself remains cross-platform. The helper needs no model configuration, key or
provider requests, generates only synthetic data in a checked temporary directory,
and refuses to overwrite a report. Each process trial has a 45-second bound.

Every trial checks the complete result: lexical mask/passthrough bytes, exact deep
profile paths/types/presence, all 100,000 record counts, or a nonzero preview-overflow
exit with zero stdout and the prior file preserved. OS process peaks include startup,
input and output buffering. Timings include process startup and IO, not just a pure
algorithm. Terminal-display expansion is not measured by this pipe/file probe.

## Recorded development snapshot

The [raw report](../benchmarks/pre-sharing-boundaries.json) belongs to runtime source
`15394a9f4583e5c155610215a75bb610b4fe95ac`, with binary SHA256
`c4a13bfe4bdc6a15dd289131f5e53c0b4b412d9d305b3d6e142d404b5a9b35db`.
This frozen development binary is distinct from the published v0.31.0 program.
The table shows medians of three trials on one Windows environment:

| 场景 | 完整进程耗时中位数（秒） | 峰值工作集中位数（MiB） |
| --- | ---: | ---: |
| 7 MiB，浅层严格遮盖 | 0.084 | 28.54 |
| 7 MiB，浅层显式可选遮盖 | 0.093 | 28.56 |
| 7 MiB，128 层，缺失路径保留 | 0.171 | 21.63 |
| 7 MiB，128 层，结构概览 | 0.120 | 21.59 |
| 十万条记录，结构概览 | 0.091 | 21.57 |
| 3 MiB 控制字符，编码超限 | 0.042 | 51.19 |

All 18 trials returned the expected complete result or expected failure. The encoded
preview overflow preserved the old file every time. These scoped results did not
justify replacing the existing traversal algorithms. They are not a statistical or
cross-platform performance guarantee, allocator-only measurement, or measurement of
the future official release assets. Keep the raw source/binary identity when citing them.

## Separate terminal display check

The pipe/file measurements above exclude terminal display escaping. A separate
regression uses an actual terminal with 3 Mi C1 characters: its raw preview fits,
but its escaped representation exceeds 16 MiB. It must exit with an error and
without a partial JSON body. The renderer now counts the complete escaped UTF-8
length before allocating the display buffer and writes escapes directly into it.
Unit tests cover exact byte boundaries, multibyte text and preserved JSON tokens.
This refinement does not change pipe/file JSON or the actual request body, and
is not an additional batch outcome or an official-package performance claim.

## Delivery boundaries

Local 308 Rust tests and nine release-rule tests passed for the runtime source above;
its [exact-source CI](https://github.com/ax2/zistral/actions/runs/41012262643) passed
Windows/Linux/macOS, Rust 1.85 and release metadata. Independent pointer/profile
oracles, strict-mode comparison, shared-request construction checks and bounded
configured-service synthetic cases are separate correctness evidence, not these
Windows measurements.

The batch still needs a selected release version, version-specific full checks,
native installer rehearsals, official package checks and illustrated Feishu article37
after publication. Frequent commits continue; a third completed outcome does not
automatically trigger another minor release on the same Beijing date. No automation,
development cutoff or fixed release schedule is introduced by this checkpoint.

Usage and semantic limits: [profiles](json-profile.md), [redaction](json-redaction.md),
[request preview](request-preview.md), [release rules](releasing.md).
