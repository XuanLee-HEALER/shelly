# Executor Module Design — Shelly

## 概述

Executor 是 Shelly 的执行模块。它是一个无状态的系统操作执行器，职责是接收工具调用请求，执行对应的系统操作，返回执行结果。它不参与任何编排逻辑，不解释执行结果的语义，不维护任务队列。

## 设计原则

### 不使用 MCP

Executor 与 Brain 运行在同一进程内，工具调用通过直接函数调用完成。不引入 MCP 协议（JSON-RPC over stdio/HTTP），因为在进程内通信的场景下，MCP 的协议层、序列化层、传输层都是无意义的开销。

### 直接映射

Brain 返回的 `ToolUse { id, name, input }` 经过上层提取后，以 `name` + `input`（raw JSON）的形式传入 Executor。Executor 内部负责将 raw JSON 反序列化为具体工具的强类型参数，执行操作，返回结果。

### 同步阻塞

单次工具调用绑定一个子进程的完整生命周期：启动 → 等待结束 → 收集输出。不做异步任务追踪、不做进程池、不做后台执行。在 tokio 环境中通过 `tokio::process::Command` 适配。

## 技术栈

| 用途 | 库 | 说明 |
|------|-----|------|
| 进程执行 | tokio::process | 异步子进程管理，与项目统一的 tokio runtime 一致 |
| 序列化 | serde + serde_json | 工具输入参数的反序列化 |
| 配置解析 | toml | 解析工具描述配置文件（TOML 格式） |
| 错误处理 | thiserror | 定义 ExecutorError 类型化错误 |
| 结构化日志 | tracing | 记录每次执行的命令、耗时、退出码、输出摘要 |

## 职责边界

### Executor 负责

- 管理已注册工具的集合
- 根据 tool name 路由到对应的工具实现
- 将 raw JSON input 反序列化为工具的强类型参数
- 执行系统操作（当前仅 bash）
- 对单次执行施加约束（超时、输出大小限制）
- 返回结构化执行结果或错误
- 记录执行日志（命令、耗时、退出码、输出摘要）

### Executor 不负责

- 决定执行什么工具（由上层决定）
- 解释执行结果的语义（由上层决定）
- 任务队列、并发控制、后台任务管理
- 重试逻辑（执行失败直接返回，由上层决定是否重试）
- 对话状态或推理逻辑

## 对外表面积

Executor 对外暴露三样东西：

1. **一个接口** — `execute`
2. **工具定义导出** — `tool_definitions`
3. **一组错误类型** — `ExecutorError`

## 接口定义

### `execute`

唯一的公共执行方法。接收工具名和原始输入，返回执行结果。

```
async fn execute(&self, tool_name: &str, input: serde_json::Value) -> Result<ToolOutput, ExecutorError>
```

内部流程：

1. 根据 `tool_name` 查找已注册的工具，未找到返回 `ExecutorError::UnknownTool`
2. 将 `input` 反序列化为该工具的参数类型，失败返回 `ExecutorError::InvalidInput`
3. 执行工具操作
4. 收集结果，施加输出约束（截断超长输出）
5. 返回 `ToolOutput`

### `tool_definitions`

返回所有已注册工具的定义列表，格式与 Anthropic Messages API 的 tools 字段兼容。上层在构造 `MessageRequest` 时直接将此列表传入 Brain，无需手动维护工具定义。

```
fn tool_definitions(&self) -> Vec<ToolDefinition>
```

`ToolDefinition` 与 Brain 模块中定义的类型相同（从共享的类型模块引入），包含 name、description、input_schema（JSON Schema）。

## 数据类型

### ToolOutput

单次工具执行的结果：

| 字段 | 类型 | 说明 |
|------|------|------|
| content | String | 执行输出的文本内容 |
| is_error | bool | 是否为执行错误（命令非零退出等） |

`content` 的格式由具体工具决定。对于 bash 工具，它是 stdout 和 stderr 的组合文本。`is_error` 为 true 时，上层在构造 `tool_result` 发回 Brain 时设置 `is_error: true`，让模型知道执行失败了。

注意：命令返回非零退出码不算 `ExecutorError`，而是正常的 `ToolOutput { is_error: true }`。`ExecutorError` 保留给 Executor 自身无法完成执行的情况。

### ExecutionConstraints

单次执行的约束条件：

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| timeout_secs | u64 | 30 | 最大执行时间 |
| max_output_bytes | usize | 1048576 (1MB) | stdout + stderr 的最大采集大小 |
| working_dir | Option\<PathBuf\> | None | 工作目录，None 时继承 daemon 的工作目录 |

约束由 Executor 全局配置提供默认值，不由单次调用传入（初期简化）。

## 错误处理

### ExecutorError

| 错误变体 | 含义 | 上层应做的 |
|----------|------|-----------|
| UnknownTool | tool_name 不匹配任何已注册工具 | 构造错误 tool_result 返回给 Brain，让模型修正 |
| InvalidInput | input JSON 无法反序列化为工具参数 | 同上 |
| SpawnFailed | 子进程无法启动（权限、命令不存在等） | 记录日志，构造错误 tool_result |
| Timeout | 执行超时，子进程已被 kill | 构造错误 tool_result，让模型知道超时 |
| OutputCaptureFailed | 无法读取子进程输出 | 记录日志，构造错误 tool_result |

所有 ExecutorError 变体都携带上下文信息（tool_name、原始 input 摘要等）。

### 命令失败 vs Executor 错误

这个区分很重要：

- `rm nonexistent_file` 返回退出码 1 → **不是** ExecutorError，是 `ToolOutput { content: "rm: cannot remove ...", is_error: true }`
- 命令执行了 60 秒被 kill → **是** `ExecutorError::Timeout`
- `tool_name` 写成了 `"bsh"` → **是** `ExecutorError::UnknownTool`

原则：子进程成功启动并正常退出（无论退出码是什么），都是 `ToolOutput`。Executor 自身层面的失败才是 `ExecutorError`。

## 内置工具：Bash

### 工具定义

工具的结构部分（name、input_schema）硬编码在 Rust 代码中，不会变。工具的 description 文本从外部 TOML 配置文件加载，支持热更新。

每次调用 `tool_definitions()` 时从磁盘读取配置文件，解析出最新的 description。调用频率很低（每轮推理最多一次），读一个小文件的开销可忽略。不需要 file watcher、不需要信号通知、不需要缓存失效机制。

读取失败时 fallback 到编译时内置的默认描述，不中断运行。

配置文件格式为 TOML，因为工具描述中可能包含换行、引号、代码片段等特殊字符，TOML 的多行字符串天然支持，无需转义。

配置文件示例（`tools.toml`）：

```toml
[bash]
description = """
Execute a shell command via /bin/sh -c.
The system is Ubuntu 24.04 ARM64.
Available tools include: systemctl, journalctl, docker, ip, ss, df, free, top, curl, git, cargo.
Commands run with daemon process privileges.
Stdout and stderr are captured. Exit code is returned.
"""
```

加载逻辑伪代码：

```
fn tool_definitions(&self) -> Vec<ToolDefinition> {
    let desc = load_tool_description(&self.config.tools_toml_path, "bash")
        .unwrap_or_else(|_| self.default_descriptions.bash.clone());

    vec![
        ToolDefinition {
            name: "bash".into(),                    // 硬编码
            description: desc,                       // 从配置加载
            input_schema: bash_input_schema(),        // 硬编码
        }
    ]
}
```

### 技术栈补充

| 用途 | 库 | 说明 |
|------|-----|------|
| 配置解析 | toml | 解析工具描述配置文件 |
```

### 输入参数

```
command: String    // 要执行的 bash 命令
```

### 执行方式

通过 `sh -c "{command}"` 执行。使用 `tokio::process::Command`，设置超时，采集 stdout 和 stderr。

### 输出格式

```
[stdout]
{stdout content}

[stderr]
{stderr content}

[exit_code]
{code}
```

stdout 或 stderr 为空时省略对应段落。输出总长度超过 `max_output_bytes` 时从尾部截断并附加 `\n...(truncated)` 标记。

### is_error 判定

exit_code != 0 时 `is_error = true`。

## 初始化与生命周期

### 初始化

Executor 在进程启动时初始化一次，注册所有内置工具，之后在整个进程生命周期内复用。

```
Executor::new(config: ExecutorConfig) -> Executor
```

初始化过程：

1. **加载配置** — 超时、输出限制、工作目录等默认约束
2. **注册内置工具** — 当前只有 bash
3. **就绪** — 返回 Executor 实例

Executor 初始化不会失败（不依赖外部资源），因此返回值不是 Result。

### 为什么只初始化一次

- Executor 本身无状态，工具注册表在初始化后不变
- 每次 `execute` 调用都是独立的，子进程的生命周期绑定在单次调用上
- 不需要连接池等有状态资源

### 外部使用方式

```
// 进程启动时，初始化一次
let executor = Executor::new(config);

// 获取工具定义，传给 Brain 构造请求时使用
let tools = executor.tool_definitions();

// 执行工具调用
let output = executor.execute("bash", json!({"command": "df -h"})).await?;
```

Executor 实例是 `Send + Sync` 的，可通过 `Arc<Executor>` 在多个 task 间共享。

## 工具扩展

当前只有 bash，但 Executor 的内部结构支持注册多个工具。每个工具实现一个内部 trait：

```
trait ToolImpl: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn run(&self, input: serde_json::Value) -> Result<ToolOutput, ExecutorError>;
}
```

此 trait 是 Executor 模块内部抽象，不对外暴露。新增工具只需实现此 trait 并在初始化时注册。

## 配置

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| default_timeout_secs | 30 | 单次执行默认超时 |
| max_output_bytes | 1048576 | 输出采集上限（1MB） |
| working_dir | None | 默认工作目录 |
| shell | /bin/sh | shell 路径 |

## 内部日志

每次 `execute` 调用，Executor 记录一条结构化日志：

| 字段 | 说明 |
|------|------|
| timestamp | 执行时间 |
| tool_name | 工具名 |
| command_summary | 命令摘要（截断到 200 字符） |
| duration_ms | 执行耗时 |
| exit_code | 退出码（如有） |
| output_bytes | 输出大小 |
| status | success / error / timeout |

通过 tracing 输出，当前阶段打印到 stdout。

## 与 Brain 模块的关系

Executor 和 Brain 之间没有直接依赖。它们共享 `ToolDefinition` 类型（来自共享类型模块），但不互相引用。上层编排的典型流程：

```
1. 从 executor.tool_definitions() 获取工具定义
2. 通过 RequestBuilder 将工具定义传入 MessageRequest
3. brain.infer(request) 获取响应
4. 从响应中提取 ToolUse { id, name, input }
5. executor.execute(name, input) 获取 ToolOutput
6. 将 ToolOutput 包装为 tool_result，通过 RequestBuilder 追加到 messages
7. 回到步骤 3（如果 stop_reason == ToolUse）
```

步骤 1-7 的编排逻辑不属于 Brain 也不属于 Executor，由上层负责。

## 不做的事情（显式排除）

- **不做 MCP**：进程内调用，不需要协议层。
- **不做任务队列**：每次调用同步等待完成，不做排队。
- **不做并发控制**：不限制同时执行的工具调用数量（初期场景为单次串行调用）。
- **不做重试**：执行失败直接返回，重试策略由上层决定。
- **不做沙箱**：bash 命令以 daemon 进程的权限直接执行，不做额外隔离（这是设计选择，daemon 本身就需要系统级权限）。
- **不做结果缓存**：相同命令不缓存结果。