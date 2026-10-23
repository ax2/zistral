# Zistral · zi

轻量、跨平台、支持管道的 Rust 命令行 Agent。一个可执行文件，不需要 Python 或 Node.js。

```sh
cat 1.json | zi "将 .abc 数组的第一个元素中的内容连接起来"
cat 1.txt | zi 对内容进行字数统计
cat notes.txt | zi "总结为三句话" > summary.txt
git diff | zi "生成一句简洁的提交说明"
```

## 安装与配置

从 [Releases](https://github.com/ax2/zistral/releases) 下载对应平台的压缩包，解压后将
`zi`（Windows 为 `zi.exe`）放到 PATH 目录。使用 `SHA256SUMS.txt` 校验下载文件。
首版提供 Windows x64、Linux x64（glibc 2.35+）、macOS Apple Silicon；其他架构可从源码编译。
程序未做代码签名。项目名是 `zistral`，命令是 `zi`，尚未发布到 crates.io。

首次运行配置向导，依次设置模型服务地址、模型名称、接口协议和 API Key：

```sh
zi config
```

配置会自动保存，此后无需设置环境变量即可调用：

```sh
cat 1.txt | zi '统计汉字数量'
```

再次运行 `zi config` 可修改配置，回车保留原值。Key 输入时隐藏，不会显示已有 Key；
输入 `-` 可清除已保存的 Key，便于切换到无需认证的本地服务。Ctrl+C 中途取消不会覆盖原文件。
也支持 `zi config --config PATH`。向导需要交互终端，不接收管道输入。
OpenAI 服务默认选择 `responses`，其他兼容服务默认 `chat`，可按提供方要求修改。
若指令恰好只有 `config` 一个词，可用 `zi -- config` 将它作为普通指令。

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
- 默认最多 8 次模型请求，每次 120 秒；`--max-steps`、`--timeout` 可调整。
- 默认最多读取 8 MiB，`--max-input` 可提高到 64 MiB；完整缓冲输入，不做流式切分。
- 成功返回 0，运行错误返回 1，命令参数错误返回 2；下游关闭管道时安静退出。
- 无后台服务，无自动升级，无本地聊天历史。网络模式会把输入发送给配置的模型服务。
- 首版不执行 Shell、不修改文件、不浏览网页。可调用精确计数与 JSON 提取工具。

源码构建：

```sh
cargo install --git https://github.com/ax2/zistral --tag v0.2.0 --locked
```

更多配置、精确定义、开发与验证范围见 [English README](README.md)。MIT 开源协议。
