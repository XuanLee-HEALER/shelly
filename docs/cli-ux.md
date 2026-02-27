# CLI UX Design — shelly-cli

## 概述

shelly-cli 是 shelly 的第一个客户端，通过 UDP 协议与 daemon 交互。本文档定义 CLI 在使用层面的行为规范，包括输入处理、输出显示、字符编码、控制字符处理。

## 技术栈

| 用途 | 库 | 说明 |
|------|-----|------|
| 行编辑 | rustyline | 提供 readline 风格的行编辑能力：光标移动、退格、删除、历史记录、UTF-8 支持 |
| 终端输出 | std::io::stdout | 标准输出，配合 UTF-8 编码直接写入 |

### 为什么用 rustyline

裸 stdin 逐字节读取时，退格键输出 `\x7f` 而不是实际删除字符，方向键输出 ANSI 转义序列，中文输入需要手动处理多字节边界。rustyline 在内部处理了所有这些问题：

- 退格、删除、方向键、Home/End 等编辑键正确处理
- UTF-8 多字节字符（中文、日文、emoji）正确显示和编辑
- 光标在宽字符（中文占 2 列）上正确定位
- 输入历史（上下方向键翻阅）
- Ctrl+C 中断当前输入、Ctrl+D 退出
- 跨平台终端兼容

不需要自己处理 raw mode、ANSI 序列解析、字符宽度计算。

## 输入处理

### 基本行为

- 使用 rustyline 的 `readline` 方法读取一行完整输入
- 提示符为 `> `
- 用户按 Enter 提交输入
- 空行（只按 Enter）不发送，重新显示提示符

### 控制字符

| 按键 | 行为 |
|------|------|
| Backspace | 删除光标前一个字符（含多字节字符） |
| Delete | 删除光标后一个字符 |
| ← → | 光标左右移动（按字符，非按字节） |
| Home / Ctrl+A | 光标移到行首 |
| End / Ctrl+E | 光标移到行尾 |
| ↑ ↓ | 翻阅输入历史 |
| Ctrl+C | 取消当前输入行，重新显示提示符 |
| Ctrl+D | 空行时退出 CLI；有内容时删除光标后字符 |
| Ctrl+W | 删除光标前一个单词 |
| Ctrl+U | 清除光标前所有内容 |
| Ctrl+L | 清屏 |

以上行为由 rustyline 默认提供，无需自行实现。

### 多行输入

初期不支持多行输入。每次 Enter 即提交。后续如需支持，可通过 `\` 续行或特定分隔符标记多行模式。

### 输入历史

- rustyline 内置历史功能，上下方向键翻阅
- 历史保存到文件（`~/.shelly_history`），跨会话保留
- 重复输入不重复记录
- 历史条目上限可配置（默认 1000）

## 输出处理

### 基本行为

- RESPONSE payload 的 content 字段直接打印到 stdout
- 打印后换行，重新显示提示符
- UTF-8 编码，中文和其它多字节字符直接输出，终端负责渲染

### 状态提示

| 状态 | 显示 |
|------|------|
| 已发送 REQUEST，等待 ACK | `[waiting...]` |
| 已收到 ACK，等待 RESPONSE | 无额外提示（保持 `[waiting...]`） |
| 收到 RESPONSE | 清除状态提示，打印内容 |
| 超时 | `[error] shelly not responding` |
| 网络错误 | `[error] network error: {detail}` |
| 解码错误 | `[error] invalid response` |

`[waiting...]` 在发送后立即打印，RESPONSE 到达后覆盖（通过 `\r` 回到行首重写，或直接在下一行打印内容）。

### 错误响应

RESPONSE 中 `is_error: true` 时，content 前加 `[error] ` 前缀打印，与正常输出视觉区分。

### 长输出

不做分页。内容直接打印到 stdout，用户可以通过终端自身的滚动查看。如果需要分页，用户可以在 shelly 端让 LLM 控制输出长度。

## 字符编码

### 原则

- CLI 全程 UTF-8，不支持其它编码
- 输入：rustyline 在 UTF-8 locale 下正确处理多字节输入
- 输出：content 字段是 UTF-8 字符串，直接写 stdout
- 协议层：MessagePack 对 UTF-8 字符串原生支持，编解码无损

### 终端要求

CLI 假设终端环境为 UTF-8。如果终端 locale 不是 UTF-8，中文等字符可能显示异常。这不是 CLI 的问题，不做兼容处理。启动时可检查 `LANG` 环境变量，非 UTF-8 时打印警告。

## 会话生命周期

```
启动
  │
  ├── 打印欢迎信息（shelly-cli vX.X.X, target: addr:port）
  ├── 加载历史文件
  │
  ▼
主循环
  │
  ├── readline("> ") 读取输入
  ├── 空行 → 跳过
  ├── 编码 REQUEST → UDP 发送
  ├── 打印 [waiting...]
  ├── 等待 REQUEST_ACK（超时重传）
  ├── 等待 RESPONSE
  ├── 打印内容
  └── 回到 readline
  │
  ▼
退出（Ctrl+D / EOF）
  │
  ├── 保存历史文件
  └── 退出
```

## 配置

| 参数 | 默认值 | 说明 |
|------|--------|------|
| --target | 127.0.0.1:9700 | shelly daemon 地址 |
| --timeout | 5 | REQUEST_ACK 超时秒数 |
| --max-retries | 3 | 最大重传次数 |
| --history-file | ~/.shelly_history | 历史文件路径 |
| --history-size | 1000 | 历史最大条目数 |