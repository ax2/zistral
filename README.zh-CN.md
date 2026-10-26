# Zistral · zi

聚焦文本、结构化数据和明确开发场景的轻量 Rust 命令行助手，跨平台、支持管道。
一个可执行文件，不需要 Python 或 Node.js；不扩张为通用桌面自动化工具。

```sh
cat 1.json | zi "将 .abc 数组的第一个元素中的内容连接起来"
cat 1.txt | zi 对内容进行字数统计
cat notes.txt | zi "总结为三句话" > summary.txt
git diff | zi "生成一句简洁的提交说明"
```

## 安装与配置

从 [Releases](https://github.com/ax2/zistral/releases/latest) 下载原生安装包：

| 平台 | 安装包 | 安装方法 |
| --- | --- | --- |
| Windows x64 | `windows-x64-setup.exe` | 双击安装，无需管理员权限，自动添加用户 PATH 和开始菜单入口 |
| Linux x64 / Debian、Ubuntu | `linux-amd64.deb` | `sudo apt install ./zistral-v0.7.0-linux-amd64.deb` |
| macOS Apple Silicon | `macos-arm64.pkg` | 双击安装，命令放到 `/usr/local/bin/zi` |

安装后直接运行 `zi`。Windows 安装结束可立即启动，或在开始菜单打开 zi；
已经打开的终端需要重开一次以读取新的 PATH。无需安装 Rust、Python、Node.js 或 Visual C++ 运行库。
升级保留已有模型配置；Windows 卸载只移除自身添加的 PATH 项，保留配置。
Linux 包需要 glibc 2.35+ 和系统 CA 证书。其他架构可从源码编译。
同时保留无需安装的便携压缩包；便携版需要自行放到 PATH 目录。所有下载均提供 `SHA256SUMS.txt` 校验。
程序未做代码签名。项目名是 `zistral`，命令是 `zi`，尚未发布到 crates.io。

首次直接输入 `zi` 会自动打开配置向导，检测 Codex、WorkBuddy 和 dsh 中已有的模型配置。
也可随时手动打开：

```sh
zi config
```

选择检测到的模型后，自动导入服务地址、模型名称、接口协议和 API Key；找到 Key 时无需重复输入，
缺少 Key 时再隐藏提示填写。选择 `0` 可手动配置。来源智能体的文件保持原样。

```sh
zi config --list     # 只预览模型列表，显示 Key 是否存在，不显示 Key
zi config --manual   # 跳过检测，手动配置或修改现有设置
```

支持的配置来源：

- Codex：`$CODEX_HOME/config.toml` 或 `~/.codex/config.toml`，包括自定义 provider、命名 profile、环境变量和 `auth.json` 中的普通 API Key。
- WorkBuddy：`~/.workbuddy/models.json`，兼容 `~/.codebuddy/models.json`。完整请求地址和自定义路径会正确处理。
- dsh：`~/.dsh/profiles/*/cordis.yml` 与 `cordis.patch.yml`，合并模型 provider 配置，解析对应环境变量或 `.dsh/.credentials.yaml` 中的 API Key 引用。

Codex 的 ChatGPT 账号登录与订阅凭据不会被当作 API Key 导入；没有普通 API Key 时需要补填。
也不会导入 dsh 的浏览器会话或账号访问令牌，不执行任何来源的认证命令或插件。
需要额外认证请求头、查询参数、命令认证或不支持协议的配置会说明原因并跳过。
单个来源格式错误不会阻止其他来源和手动配置。配置文件上限 2 MiB，最多列出 512 个候选项。

支持 `chat`、`responses` 和 `anthropic` 三种协议，其中 `anthropic` 适配 dsh 的 Anthropic Messages 服务。
Anthropic 地址末尾没有 `/v1` 时会使用 `/v1/messages`，已有 `/v1` 时使用 `/messages`。
`--max-output-tokens` 可调整 Anthropic 输出上限，默认 4096。

配置会自动保存，此后无需设置环境变量即可调用：

```sh
cat 1.txt | zi '统计汉字数量'
```

运行 `zi config --manual` 可修改现有配置，回车保留原值。Key 输入时隐藏，不会显示已有 Key；
输入 `-` 可清除已保存的 Key，便于切换到无需认证的本地服务。Ctrl+C 中途取消不会覆盖原文件。
也支持 `zi config --config PATH`。向导需要交互终端，不接收管道输入。
OpenAI 服务默认选择 `responses`，其他兼容服务默认 `chat`，可按提供方要求修改。
`config`、`models`、`use`、`doctor`、`presets` 是保留命令；例如 `zi -- doctor` 可将该词作为普通指令。

Key 保存在本地 TOML 文件中，没有使用加密钥匙串；Linux/macOS 文件权限为 `0600`，
Windows 使用配置目录继承的访问权限。向导只保存配置，不发送模型测试请求。

仍可使用环境变量临时覆盖已保存的配置（模型名称由提供方决定）：

```sh
export ZI_MODEL='你的模型名称'
export ZI_API_KEY='你的API密钥'
export ZI_BASE_URL='https://api.openai.com/v1'
export ZI_API='responses'
cat 1.txt | zi '统计汉字数量'
```

OpenAI 兼容服务可使用 `ZI_API=chat`（默认），并指定服务商的 API 根地址。
本地服务示例：`ZI_BASE_URL=http://localhost:11434/v1`，模型使用本地已安装的名称。
不支持工具调用的模型可以加 `--no-tools`。本地服务通常不需要密钥。

PowerShell：

```powershell
$env:ZI_MODEL = '你的模型名称'
$env:ZI_API_KEY = '你的API密钥'
$env:ZI_API = 'responses'
Get-Content -Raw -Encoding utf8 .\1.txt | zi '统计汉字数量'
# 直接读文件，避免旧版 PowerShell 管道编码影响中文：
zi -i .\1.txt '统计汉字数量'
```

`zi --config-example` 可输出配置模板。默认位置为 Windows 的
`%APPDATA%\zistral\config.toml`，或 Linux/macOS 的 `~/.config/zistral/config.toml`。
支持 `XDG_CONFIG_HOME` 和 `--config`。参数优先于环境变量，环境变量优先于配置文件。
密钥读取 `ZI_API_KEY`，其次 `OPENAI_API_KEY`，也可在配置中指定 `api_key_env`。
没有设置密钥环境变量时，自动读取向导保存的 API Key。

## 终端体验

直接输入 `zi` 进入交互界面：欢迎面板、当前目录、模型与协议、等待动画、请求进度、工具调用、耗时和服务返回的 token 用量。
结果会突出显示标题、列表和代码块，支持中文和终端自动换行。输入 `/help`、`/config`、`/clear`、`/exit` 管理交互。
每次提问是独立任务，不保存或发送上一轮对话；Ctrl+C 结束程序。配置好后也可直接用 `zi '问题'`。

管道或重定向输出自动保持纯文本，`--json` 保持严格 JSON。`--plain` 关闭装饰和进度，
`NO_COLOR=1` 关闭颜色。进度信息进入标准错误；不会把服务返回的终端控制指令作为终端命令执行。
普通终端默认流式显示；`--no-stream` 可保留完整回答模式。

模型功能仍需要你可用的服务配置和 API Key；安装包不内置付费密钥。
已有智能体配置可直接导入，离线 `--count` / `--select` 不需要任何模型配置。

## 多模型保存与切换

每个配置保存独立的服务地址、模型、协议和 Key；导入新模型不会覆盖其他命名配置。

```sh
zi config --name work      # 导入或手动填写，保存为 work，并设为当前配置
zi config --name local     # 再保存另一个模型
zi models                  # 列出已保存模型，* 标记当前配置，不显示 Key
zi use work                # 切换默认配置
zi -p local "解释这段代码"   # 仅本次使用 local，不修改默认配置
zi doctor                  # 本地检查，不发送模型请求
zi doctor --probe          # 发送一个小请求，验证模型连接和认证
```

旧版单模型配置继续作为 `default` 使用，可用 `zi use default` 切回。
再次运行 `zi config --name work` 修改该配置；`zi config` 修改当前选中的配置。
配置名支持 1–64 个英文字母、数字、下划线和短横线，最多 128 个命名配置。
优先级：`--profile` > `ZI_PROFILE` > 保存的当前配置；模型、地址、协议和密钥的已有参数/环境变量覆盖规则仍有效。
在交互界面可用 `/models`、`/use NAME` 和 `/doctor`。

`zi doctor` 显示配置路径、所选配置、模型、协议、服务主机、Key 是否存在及覆盖变量名称。
`--probe` 才会发送少量内容并可能产生服务商费用；失败时返回非零状态，区分认证、模型/地址、限流和网络错误。
诊断不会打印 Key、服务端错误正文或探测回答。可用 `zi -p work doctor --probe` 检查指定配置。

## 流式输出

普通终端默认边生成边显示，支持 Chat Completions、Responses 和 Anthropic Messages，以及流式工具调用。
服务返回普通 JSON 时自动使用完整回答；不支持流式请求的服务可加 `--no-stream`。

```sh
zi "写一个简短的故事"                  # 终端默认流式
zi --no-stream "总结今天的计划"       # 等待完整结果
cat article.txt | zi "总结" > out.txt # 重定向默认完整输出
cat article.txt | zi --json "提取为 JSON" # 保持完整 JSON 校验
cat article.txt | zi --stream "总结"  # 显式开启流式，包括重定向场景
```

流式响应仍有大小和超时限制；中断、截断和服务错误返回失败，不把部分回答当作成功。
终端或显式 `--stream` 已显示的内容无法收回；默认管道/重定向和 `--json` 保持完整结果输出。
`--stream` 不能与 `--json` 一起使用。`--plain` 关闭自动流式和界面装饰，仍可显式搭配 `--stream`。
流式文本逐段显示，完整回答模式保留标题、列表和代码块高亮。

## 场景命令与自定义提示词

```sh
zi presets                                # 离线列出内置和自定义场景
cat article.txt | zi -s summarize
cat notes.txt | zi -s translate --var language=French
git diff | zi -s review "重点检查正确性"
git diff --cached | zi -s commit
cat ticket.txt | zi -s extract "姓名、优先级、下一步"
zi presets show translate
zi presets render translate --var language=Chinese  # 只预览提示词，不调用模型
```

内置 `summarize`、`translate`、`extract`、`explain`、`review`、`commit`、`rewrite` 七个场景。
翻译默认目标为中文；后面的自然语言可补充要求。输入仍单独作为数据发送，不拼进提示词模板。
`extract` 自动开启完整 JSON 校验，不能搭配 `--stream`。模型选择、终端和管道规则沿用原有设置。
场景不会自动授权读写文件或运行命令；审查管道 diff 无需项目权限，要读项目时可明确搭配
`-C . --read-only -s review`。

```sh
zi presets init
zi --presets ./my-presets.toml presets init
zi --presets ./my-presets.toml -s team-summary --var audience=研发团队
```

默认在当前模型配置文件旁创建 `presets.toml`，已有文件绝不覆盖。支持 `--presets FILE` / `ZI_PRESETS`；
不会扫描项目并自动加载其提示词。自定义格式如下，修改文件后下一次调用直接读取：

```toml
version = 1
[presets.team-summary]
description = "面向团队的总结"
prompt = "为 {{audience}} 总结输入，保留事实、决定和下一步。"
# json = true
[presets.team-summary.defaults]
audience = "研发团队"
```

`{{name}}` 仅做一次字面替换，不执行表达式、脚本或命令，不递归展开变量，不读取其他文件。
变量名为最多 32 字节的英文字母/数字/下划线，不能以数字开头；`input` 保留，管道数据单独传递。
双花括号专用于变量，普通 JSON 单花括号不受影响。值为 1–4096 字节，最多 12 个变量；
缺值、重复、未使用的覆盖参数会在请求前报错。文件最多 256 KiB / 64 个自定义场景，
模板最多 16 KiB，展开后连同补充指令最多 128 KiB。自定义名称不能覆盖内置场景。
未知字段/格式版本和错误 TOML 明确报错，不回显原始配置正文。

初始化使用原子写入，Unix 权限为 0600；模板是普通本地文本，不要写入 API Key。
交互界面支持 `/presets` 和 `/run NAME [补充要求]`，每项仍是独立任务；变量覆盖使用命令行 `--var`。
字面指令恰好从保留词 presets 开头时，使用 `zi -- presets`。

## 在项目中执行多步任务

```sh
zi -C D:/work/my-project "修复失败的测试，修改后运行验证"
zi --agent "读取当前项目，完成这项修改并检查结果"
zi -C . --read-only "审查代码并说明主要问题"
zi -C . --no-exec "更新文档，不运行命令"
zi -C . --command-timeout 300 "构建项目并修复错误"
git diff | zi -C . "结合项目源码审查这些改动"
```

`--agent` 开启当前目录的项目能力；`-C` / `--cwd DIR` 选择项目并开启这些能力。
`zi -C DIR` 可进入项目交互模式。需要模型支持工具调用；每个任务可完成读取、计划、修改、
执行测试、分析失败、修复和再次验证。默认最多 32 次模型请求，`--max-steps` 可设 1–128。
交互界面的每次提问仍是独立任务，不保存上一项任务的上下文。

共内置 14 个工具：项目环境与分层 `AGENTS.md`、文件列表、文本搜索、分页读取、创建/替换、
精确修改、单文件删除、任务内撤销、Git 状态与差异、命令执行、步骤计划，以及原有计数和 JSON 提取。
列表和搜索遵循 `.gitignore`，跳过常见缓存目录，不依赖外部 ripgrep；Git 查看工具需要已安装 Git。

修改已有文件前必须先读取，文件被外部改动时会拒绝覆盖；支持 CRLF、可执行权限保留、原子写入，
撤销暂存在当前任务内，原始内容最多 32 MiB。修改前会提供目录内适用的 `AGENTS.md`。
文本文件上限 2 MiB，单次读取、搜索结果及命令每路输出上限 64 KiB。
Windows 使用 PowerShell，Linux/macOS 使用 `/bin/sh`；命令不接收交互输入，默认 60 秒超时，
可设至 3600 秒，结束或超时后清理后代进程。项目自身的 Rust、Node.js 等构建工具需要另外安装，
zi 本身仍是独立可执行文件。

**项目模式允许直接改文件和执行命令，不逐次询问确认。** 请用于可信项目与任务。
`--read-only` 只允许读取和固定 Git 查看；`--no-exec` 允许修改文件，禁止任意 Shell 命令。
原生文件工具限制项目路径，拒绝越界符号链接和常见凭据、仓库元数据文件；这些检查不是系统级沙箱。
开启的 Shell 命令使用当前账户权限，可能访问项目之外的文件。重要项目建议保留 Git 提交或备份。
撤销仅覆盖本次任务的文件工具修改，不覆盖命令操作，退出后不保留。

不加项目参数时，原来的管道数据模式保持不变。项目模式的标准输出仍为最终结果，工具详情交给模型，
进度进入标准错误。默认管道和 `--json` 会等待完整回答，但文件修改立即生效，不因模型或网络失败自动回滚。
读取的项目内容及命令结果会发送给所配置的模型服务。更多架构、参考来源和限制见
[Agent 设计说明](docs/agent-design.md)。

## 不用模型也能精确处理

```sh
cat 1.txt | zi --count
cat 1.json | zi --select /abc/0 --join ','
```

`--count` 输出 JSON：UTF-8 字节数、Unicode 字符数、可见字符簇数、非空白字符数、
空白分隔词数、汉字数、行数。英文单词计数与中文分词不是一回事，`words` 按空白分隔；
中文统计可明确指定“汉字数”或“包含空格的字符数”。

`--select` 使用 JSON Pointer：`.abc` 的第一个元素对应 `/abc/0`。
`--join` 将选中的数组用指定分隔符连接。自然语言模式也能调用这些内置工具，
但工具选择与最终表述由模型决定；需要严格确定结果时使用本地参数模式。

## 脚本行为

- 标准输入与文件均为 UTF-8，支持移除开头的 UTF-8 BOM。
- 标准输出只有最终结果；错误和 `--verbose` 过程信息进入标准错误。
- `--json` 验证模型输出，只有完整合法 JSON 才输出。
- 参数放在指令前，例如 `zi --json '提取为 JSON'`。
- 数据模式默认最多 8 次模型请求，项目模式默认 32 次，每次 120 秒；`--max-steps`、`--timeout` 可调整。
- 默认最多读取 8 MiB，`--max-input` 可提高到 64 MiB；完整缓冲输入，不做流式切分。
- 成功返回 0，运行错误返回 1，命令参数错误返回 2；下游关闭管道时安静退出。
- 无后台服务，无自动升级，无本地聊天历史。网络模式会把输入发送给配置的模型服务。
- 默认数据模式使用计数与 JSON 提取工具；显式项目模式提供读写与命令能力。没有内置网页浏览器或系统级沙箱。

源码构建：

```sh
cargo install --git https://github.com/ax2/zistral --tag v0.7.0 --locked
```

更多配置、精确定义、开发与验证范围见 [English README](README.md)。MIT 开源协议。
