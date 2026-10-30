# Import reusable task presets / 导入可复用场景（开发中，尚未发布）

An exported task definition can be installed in your user library without editing or
concatenating TOML by hand. This is offline customization of explicit instructions:

```sh
zi presets export translate --name team-fr --var language=French -o team-fr.toml
zi presets import team-fr.toml --dry-run
zi presets import team-fr.toml
zi presets render team-fr
cat article.txt | zi -s team-fr
```

`--dry-run` validates both libraries and their merged limits, then lists names to add
or replace. It never creates files/directories, reads stdin, renders a prompt, loads
model settings or sends a request. It shows the destination path and action names,
without printing prompt text or variable-default values. An invalid definition or
unapproved name collision exits nonzero without publishing any changes.

The default destination is `presets.toml` beside the selected config. `--presets PATH`
or `ZI_PRESETS` chooses another destination; a missing destination is created only by
a successful actual import. The source must be an explicit UTF-8 version-1 TOML file,
not `-`/stdin, a directory or a pipe. Existing valid leading-BOM support is retained.

```sh
zi --presets team/presets.toml presets import shared.toml --dry-run
zi --presets team/presets.toml presets import shared.toml
# After reviewing an updated definition, allow replacement of matching user names:
zi --presets team/presets.toml presets import shared.toml --replace --dry-run
zi --presets team/presets.toml presets import shared.toml --replace
```

默认拒绝同名覆盖，整份导入全部成功后才写入。`--replace` 只替换来源中同名的用户定义，
包括它的描述、模板、JSON 输出设置和整组变量默认值；其他用户场景保留。内置场景始终不能
被替换，请导出时用 `--name` 选择新的自定义名称。空来源、无效来源或无效目标库都会失败。
导入不会执行模板、调用模型或授予项目工具权限；只有随后显式选择场景执行任务才会发起请求。

The source is unchanged. Using the same resolved source/destination path is rejected. Destination
TOML is normalized on a successful import: definitions and literal values survive,
but comments, original formatting and a leading BOM do not. Keep editorial comments
in the source definition if needed. Do not mix import with `--input`, `--output`,
`--var`, model-output modes or project task flags. Use export/edit to change defaults;
`--replace` is independent of result-file `--force`.

All existing limits apply to the source, destination and merged result: at most 64
custom presets and 256 KiB TOML, with at most 16 KiB per prompt and 12 variables.
Literal defaults remain literal, including `{{...}}` inside their values. No includes,
scripts, plugin loading, captured input, automatic history or memory is imported.

Actual publication uses a temporary file in the destination directory and checks for
observed destination changes before replacement. Invalid definitions, collisions,
combined-limit failures and detected edits preserve the old library. Destination
links/devices/directories are rejected. New Unix files use mode `0600`; existing Unix
permission bits are preserved. Windows follows the destination directory's inherited
permissions. These checks do not lock out another editor or provide a filesystem
transaction against arbitrary concurrent changes by other processes.

## Locate definition errors / 定位定义错误

The development build reports a parser-provided line/column for invalid recipe and
preset TOML, without echoing source excerpts, unknown field values, prompt text or
raw parser messages. Lines and columns start at 1. Columns count Unicode characters
(a tab counts as one), with an initial BOM excluded; editor visual columns may differ.
CRLF files retain their original line numbers. If a parser supplies no valid position,
the diagnostic stays generic. Semantic checks still report their existing contextual
reason (for example an invalid pipeline step or unsupported version); a position is
not invented for errors that have none.

```text
zi: invalid recipe TOML at line 3, column 13; check version, syntax and supported fields
```

配方仍可用 `zi data --show-plan --recipe FILE` 在不读取数据的情况下完整校验和导出。
行列位置只帮助检查定义，不代表未来输入一定满足每个步骤的数据约束。
