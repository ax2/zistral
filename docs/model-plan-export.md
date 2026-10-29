# Export a model-chosen native plan / 导出模型选择的原生方案

`--exact --export-plan` asks the configured model to propose a native transformation,
validates its definition, and returns a **version-1 TOML recipe**. It does not run data
transformations. Inspect the recipe and execute it separately when appropriate.

```sh
zi --exact --export-plan -i app.log -o recent.toml 'Keep the last 20 ERROR lines'
zi data --show-plan --recipe recent.toml
zi data --recipe recent.toml -i another.log -o recent.log
```

The first command sends the instruction and explicit input to the model as context.
Unlike offline `zi data --show-plan`, it is a model task and may incur service charges.
The second command inspects/normalizes the saved definition offline. Only the third
command executes the data transformations. No recipe is automatically installed or run.

## Definition checking, not a data dry-run

- Only the existing `transform_input` name is advertised, described as a plan proposal in
  this mode. Count/select tools and project capabilities are not available. A single
  operation or 1–16 ordered steps use exactly the existing native argument/Step schema.
- Full protocol checks and the exact one-call rule precede validation. Free-text plans,
  multiple calls and other tools fail; model prose is not parsed as executable TOML.
- The shared strict parser rejects unknown/duplicate fields and invalid argument types.
  The existing recipe validator checks operation options, pointers, fields, literal
  filters and counts. Normalized TOML must remain within the existing 64 KiB recipe cap;
  tool arguments are also at most 64 KiB. No shell, import, path or code fields are added.
- Invalid definitions become correlated error observations with the validation cause,
  allowing correction within `--max-steps`. Success returns canonical TOML immediately,
  without a second model rewrite or a native data operation.

A valid `json-minify` definition can be exported while supplied context is invalid JSON:
no JSON processing happens during export. A pointer can be syntactically valid and still
be missing in later data. Do not infer data-dependent success, input encoding validity or
resource-limit compliance from successful plan export. Offline execution remains bounded
by its original input/record/intermediate/result limits and complete-result rules.

The model can choose the wrong operation or literal. Read the saved definition before
reuse. Literal filters/pointers may contain values derived from the task; a recipe is not
a privacy scrubber. The format contains definitions, not automatic task history, captured
results, model credentials or execution permissions.

## Output and mode rules

`--export-plan` requires `--exact`. JSON output and JSON presets are rejected because the
artifact is TOML. Existing exact-mode conflicts with streaming, disabled tools, project
selectors, offline and management commands still apply. Native operations and ordinary
exact execution retain their existing behavior.

Use stdout/pipes or `-o plan.toml`. Existing no-clobber, explicit `--force` and atomic
complete-result publication apply. Export failure never publishes a partial definition or
replaces an existing file. Direct terminal display retains control cleanup; prefer a
file/pipe for the canonical artifact, as described in [terminal output](terminal-output.md).
Progress identifies plan validation/export on stderr.

Interactive plan-export tasks remain independent and can use `/save` for the completed
TOML. A new task or incompatible JSON preset clears the previous result before failure;
no stale plan is exported. The model never receives previous results as hidden memory.

No dependency, native operation, model tool name, project permission, plugin runtime or
persistent memory is added. Reuse of the same native parser and version-1 renderer keeps
model-chosen plans compatible with explicit offline user workflows.
