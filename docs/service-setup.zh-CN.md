# 一次配好模型，再接入命令行工作流（v0.34.0）

本文以 v0.34.0 为例，覆盖配置、诊断、输出上限、模型目录和维护流程。

## 1. 在终端配置一次

```sh
zi config --name work
```

选择检测到的 Codex、WorkBuddy/CodeBuddy、dsh 或 [Claude Code](claude-code-import.md) 配置，缺少 Key 时按隐藏提示填写。
也可用 `zi config --name work --manual` 手动填写服务地址、模型 ID、协议和 Key。
配置向导需要交互终端，不接收管道输入；保存配置本身不发送模型请求，也不修改来源智能体的文件。

导入的环境变量引用会保留变量名，不复制变量中的 Key。以后启动 zi 的终端仍需拥有该变量；
已经生效的 zi 环境凭据不会要求重复填写。账号订阅登录、浏览器会话及访问令牌不作为 API Key 导入。
详细范围见[导入规则](import-key-references.md)。

## 2. 先在本地核对生效配置

```sh
zi models
zi -p work doctor --json
```

`models` 列出已保存的配置；`-p work` 只为这次调用选模型。希望以后直接用 `zi` 时都选它：

```sh
zi use work
```

`doctor --json` 默认离线，不测试服务权限。成功时退出码为 0，返回一个 `version: 1`、
`kind: "doctor-report"`、`ok: true` 的 JSON 对象。`configuration.details.credential`
说明生效 Key 的来源和变量名，不包含 Key 值。选择顺序是 `ZI_API_KEY` → 当前引用变量
（默认 `OPENAI_API_KEY`）→ 当前配置保存的 Key。

这两个 `--json` 要放在各自子命令之后：`doctor --json` 和 `models --remote --json`。
根命令的 `zi --json "指令"` 是要求模型返回 JSON，含义不同。

PowerShell 脚本可同时检查退出码与报告版本，再使用内容：

```powershell
$raw = zi -p work doctor --json
$checkExit = $LASTEXITCODE
$report = $raw | ConvertFrom-Json
if ($checkExit -ne 0 -or $report.version -ne 1 -or -not $report.ok) {
    throw '配置检查未通过，请查看诊断报告'
}
$report.configuration.details.credential
```

有效配置检查失败会输出完整诊断并以非零退出；查看 `error.kind` 和配置类错误的
`error.code`，例如 `model_missing`、`credential_missing`、`output_budget_invalid`。
非法命令参数可能在诊断执行前就被拒绝，不保证返回诊断 JSON。
详见[诊断契约](doctor-json.md)。

## 3. 按模型保存请求上限

```sh
zi config --name work --max-output-tokens 512
zi -p work doctor --json
```

这是已有配置的离线编辑，不会重新询问 Key。普通 Chat 使用 `max_completion_tokens`；
只有服务文档要求旧字段时，才显式保存兼容选项：

```sh
zi config --name work --max-output-tokens 512 --chat-token-limit-field max_tokens
```

Responses 使用 `max_output_tokens`，Anthropic 使用 `max_tokens`。不要把 Chat 字段选项
用于其他协议。上限针对每轮模型请求，工具后续轮次各自应用，不能保证整次任务的总 token 或费用。
服务可以有更严格的限制。

根命令上的 `--max-output-tokens 256` 仅临时覆盖本次调用；`--no-profile-budget` 临时忽略
保存的预算偏好。两者均不改配置。移除保存的偏好则显式执行：

```sh
zi config --name work --clear-output-budget
```

清理不要求已配置模型或有效生成协议，但文件仍需是合法的已知类型 TOML。
降级到 v0.33.0 前，先用较新的程序执行 `zi models`，核对配置名称。对每个保存过
预算的配置执行 `zi config --name NAME --clear-output-budget`，包括根 `default`、
非活跃配置和未配置模型的条目。旧版会拒绝读取仍含新预算字段的整份文件。

如果已经换回旧版，可以用较新版本的便携程序对同一个 `--config FILE` 执行清理。
清理保留模型、地址、Key 引用和默认选择。写入会重新生成 TOML 排版，注释可能丢失。
详见[预算说明](output-budgets.md)。

## 4. 需要时，显式联系服务

```sh
zi -p work models --remote --json
zi -p work doctor --json --probe
```

远端目录只查询模型 ID，不发送任务输入、不自动选模型、不保存配置。它不会生成内容，
但服务是否计费由提供方决定。成功查询也不代表有模型调用权限；404 可能只是服务不提供该目录。
失败时退出非零且 stdout 为空，脚本应先确认成功再解析。

`--probe` 才会发送一次简短、非流式、无工具的模型请求，固定请求 16 token 输出上限，
总请求超时最多 15 秒（更短的 `--timeout` 优先），不发送文档或项目上下文，不重试。
小上限可能不适合某些模型；探测成功也不代表工具能力或一般回答质量。
403 应核对账号、权限及服务/代理设置，不能靠改字段名、重试或重填同一个 Key 来证明修复。

## 5. 预览，再运行管道或文件任务

先准备一个 UTF-8 文件 `notes.txt`。预览不联网、不执行工具，也不显示认证请求头，
但会包含实际输入和请求体，应在本地查看：

```sh
zi -p work --preview --no-stream -i notes.txt "总结为三句话"
cat notes.txt | zi -p work --no-stream "总结为三句话"
zi -p work --no-stream -i notes.txt -o summary.txt "总结为三句话"
cat notes.txt | zi -p work --stream --max-output-tokens 256 "总结为三句话"
```

普通非流式调用在完整结果验证成功后输出 stdout；显式 `--stream` 出错时可能已输出部分文字。
`-o` 默认拒绝覆盖已有文件；需要覆盖时显式加 `--force`，只有任务成功后才替换原文件。
`--stream` 不能和 `-o` 合用。Shell 的 `>` 会按 Shell 规则提前截断文件，不具备 `-o` 的保留保证。

PowerShell 可以直接读文件，避免旧版 PowerShell 管道编码影响中文：

```powershell
zi -p work --no-stream -i .\notes.txt -o .\summary.txt --force "总结为三句话"
```

终端可以显示进度和样式；管道 stdout 始终保持普通数据，错误与状态信息写 stderr。
需要精确字数、字段选择或表格转换时，优先使用已有[离线数据命令](../README.zh-CN.md#离线文本与-json-处理)，
无需模型或 Key；自然语言任务仍由所选模型完成。

## 6. 后续维护与保存冲突

```sh
zi config --name work --api-key-env WORK_API_KEY --clear-saved-key
zi config --rename work personal
zi config --name personal --clear-output-budget
```

第一条只保存变量名、清除当前配置的 Key 文本；可在变量尚未设置时离线执行。
下次调用前应在启动终端提供 `WORK_API_KEY`，再用离线 doctor 核对来源；全局 `ZI_API_KEY`
仍会优先。仅改引用会保留原有 Key 回退，只有 `--clear-saved-key` 才删除它。
清理不会撤销提供方的 Key、取消环境变量或修改其他智能体/备份。

重命名会同步保存的默认选择，但 Shell 中的 `ZI_PROFILE=work`、脚本的 `-p work`
需自行改成 `personal`。删除用 `zi config --remove NAME`，只能删除已存在的非活跃命名配置；
先切换到另一个已配置模型，不能删除根 `default`。详见[配置维护](profile-maintenance.md)。

保存遇到 `configuration changed`，说明向导读取后文件被改过，应重开向导；
遇到 `configuration is busy`，应等另一个保存结束后显式重试。zi 不会用旧值自动覆盖或合并。
保存会复用旁边的空 `.lock` 文件，它不含 Key 或历史，进程退出即释放系统锁，
不要在写入期间删除它。已有配置符号链接可读取；修改时需将 `--config` 指向真实普通文件，
zi 不再静默替换链接。此保护针对合作进程的同一本地路径，详见[保存边界](configuration-saves.md)。

这些改进延续“显式输入 → 有界任务 → 可用输出”的定位，不增加自动会话记忆、模型权重、
后台服务或运行时依赖。Python 仅用于项目测试，安装后的 zi 不需要 Python、Node.js 或 Rust。
