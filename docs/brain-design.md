# Brain Module Design — Shelly

## 概述

Brain 是 Shelly 的推理模块。它是一个无状态的 LLM 推理客户端，职责是将结构化请求发送到推理后端，返回结构化响应。它不参与任何编排逻辑，不持有对话历史，不解释响应内容的语义。

## 技术栈

| 用途 | 库 | 说明 |
|------|-----|------|
| 异步运行时 | tokio | 项目统一使用 tokio，Brain 的 async 方法运行在 tokio runtime 上 |
| HTTP 客户端 | reqwest | 基于 tokio，内置连接池、TLS、超时、重试基础设施 |
| 序列化 | serde + serde_json | 请求/响应的 JSON 序列化与反序列化 |
| 错误处理 | thiserror | 定义 BrainError、BrainInitError 等类型化错误 |
| 结构化日志 | tracing | 输出结构化日志，当前阶段 subscriber 配置为 stdout（通过 tracing-subscriber 的 fmt layer） |
| 异步 trait | async-trait | 后端抽象 trait 中的 async 方法需要 Send bound；原生 async fn in trait（1.75+）不自动保证 Send，tokio 多线程场景下需要 async-trait 或 trait_variant 解决 |

### 说明

- **tokio** 作为整个 Shelly 项目的统一异步运行时，不仅 Brain 使用，后续 event engine、comm 等模块也会基于 tokio。Brain 模块本身不负责创建 runtime，由 main 统一初始化。
- **reqwest** 使用 `Client` 级别的连接池，Brain 初始化时创建一个 `reqwest::Client` 实例，生命周期内复用。
- **tracing** 当前阶段通过 `tracing_subscriber::fmt()` 输出到 stdout，后续接入 chronicle 模块时可替换或叠加 subscriber，不影响 Brain 内部的日志调用代码。

## 职责边界

### Brain 负责

- 接收符合 Anthropic Messages API 格式的请求，发送到推理后端
- 管理 HTTP 连接（连接池、keep-alive）
- 处理透明错误（网络抖动重试、限流退避、瞬时超时）
- 记录每次推理调用的内部日志（model、token usage、延迟、重试次数、最终状态）
- 返回结构化响应或不可恢复错误

### Brain 不负责

- 对话状态 / 消息历史的维护
- Tool 的注册、调度、执行
- 响应内容的语义解释（是否为 tool_use 等）
- Agent loop 编排
- 任何业务逻辑

## 对外表面积

Brain 对外暴露三样东西：

1. **一个接口** — `infer`
2. **一个 helper** — `RequestBuilder`
3. **一组错误类型** — `BrainError`

除此之外，Brain 没有其它公共 API。

## 接口定义

### `infer`

唯一的公共方法。接收一个完整的 `MessageRequest`，返回 `MessageResponse` 或 `BrainError`。

```
async fn infer(&self, request: MessageRequest) -> Result<MessageResponse, BrainError>
```

调用方不需要关心底层的 HTTP 细节、重试策略、连接管理。对调用方来说，这是一个"请求进去、响应出来"的黑盒。

### 输入：`MessageRequest`

对齐 Anthropic Messages API 的请求结构：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| model | String | 是 | 模型标识符 |
| system | Option\<String\> | 否 | System prompt |
| messages | Vec\<Message\> | 是 | 对话消息列表 |
| tools | Option\<Vec\<ToolDefinition\>\> | 否 | 可用工具定义 |
| max_tokens | u32 | 是 | 最大输出 token 数 |
| temperature | Option\<f32\> | 否 | 采样温度 |
| stop_sequences | Option\<Vec\<String\>\> | 否 | 停止序列 |

### 输出：`MessageResponse`

对齐 Anthropic Messages API 的响应结构：

| 字段 | 类型 | 说明 |
|------|------|------|
| id | String | 响应 ID |
| content | Vec\<ContentBlock\> | 响应内容块列表 |
| stop_reason | StopReason | 停止原因（end_turn / tool_use / max_tokens / stop_sequence） |
| usage | Usage | Token 使用统计（input_tokens、output_tokens） |

## 数据类型

### Message

```
role: Role (user / assistant)
content: Vec<ContentBlock>
```

### ContentBlock

枚举类型，对应 Anthropic API 的 content block：

- **Text** — `{ text: String }`
- **ToolUse** — `{ id: String, name: String, input: serde_json::Value }`
- **ToolResult** — `{ tool_use_id: String, content: String, is_error: Option<bool> }`

### ToolDefinition

工具的元信息，供模型推理时参考：

```
name: String
description: String
input_schema: serde_json::Value   // JSON Schema
```

注意：ToolDefinition 只是声明式描述。Brain 不知道也不关心这些工具怎么执行。

### StopReason

```
EndTurn
ToolUse
MaxTokens
StopSequence
```

### Usage

```
input_tokens: u32
output_tokens: u32
```

## Helper：RequestBuilder

上层不应该手动构造 `MessageRequest` 的 JSON。RequestBuilder 提供类型安全的链式构建：

```
RequestBuilder::new(model)
    .system("You are a system administrator.")
    .user_text("Check disk usage.")
    .assistant_text("I'll run df -h for you.")
    .user_tool_result(tool_use_id, content, is_error)
    .tools(vec![tool_def])
    .max_tokens(4096)
    .temperature(0.0)
    .build() -> MessageRequest
```

RequestBuilder 不持有状态，每次 `build()` 产出一个独立的 `MessageRequest` 值。它只做构建和基本校验（比如 messages 不能为空、第一条必须是 user role）。

## 错误处理

### Brain 内部消化的错误（调用方不感知）

| 错误 | 处理策略 |
|------|----------|
| 网络连接失败 | 指数退避重试，最多 N 次 |
| HTTP 5xx | 指数退避重试 |
| 限流（429） | 按 Retry-After 头退避，或默认退避 |
| 连接超时 | 重试 |

### Brain 返回给调用方的错误：`BrainError`

| 错误变体 | 含义 | 调用方应做的 |
|----------|------|-------------|
| AuthenticationFailed | API key 无效或过期 | 中止 / 通知用户 |
| InvalidRequest | 请求格式不合法（模型拒绝） | 检查请求构造逻辑 |
| InsufficientBalance | 余额不足 | 中止 / 通知用户 |
| Exhausted | 重试次数耗尽仍失败 | 中止 / 降级 / 切换后端 |
| ModelError | 模型返回了无法解析的响应 | 记录日志 / 重试 / 中止 |
| Timeout | 单次请求超过最大允许时间 | 重试 / 中止 |

所有 BrainError 变体都携带足够的上下文信息（原始 HTTP 状态码、响应体摘要、重试次数等），便于上层记录和诊断。

## 内部日志

每次 `infer` 调用，Brain 内部记录一条结构化日志：

| 字段 | 说明 |
|------|------|
| timestamp | 调用时间 |
| model | 使用的模型 |
| input_tokens | 输入 token 数 |
| output_tokens | 输出 token 数 |
| latency_ms | 总延迟（含重试） |
| retries | 重试次数 |
| status | success / error(variant) |

日志的消费方式由外部决定（写文件、发到 chronicle 模块等），Brain 通过标准的 tracing 机制输出，不直接写文件。

## 后端抽象

Brain 的 HTTP 交互逻辑对应一个内部 trait：

```
async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>
```

当前唯一实现是 MiniMax 的 Anthropic 兼容端点。未来切换推理后端只需要新增实现，不影响 Brain 的公共接口和上层代码。

这个 trait 是 Brain 模块的内部抽象，不对外暴露。外部只看到 `infer` 方法。

## 配置

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| endpoint | — | 推理后端 URL（必填） |
| api_key | — | API key（必填） |
| default_model | — | 默认模型标识符（必填） |
| max_retries | 3 | 最大重试次数 |
| base_retry_delay_ms | 1000 | 重试基础延迟 |
| request_timeout_secs | 120 | 单次请求超时 |
| max_output_tokens | 4096 | 默认最大输出 token |

## 初始化与生命周期

### 初始化

Brain 在进程启动时初始化一次，之后在整个进程生命周期内复用同一个实例。

初始化过程：

1. **加载配置** — 从配置文件或环境变量读取 endpoint、api_key、default_model 及其它参数
2. **创建 HTTP 客户端** — 初始化连接池（keep-alive、最大连接数、超时设置）
3. **验证连通性（可选）** — 发送一个轻量请求验证 endpoint 可达且 api_key 有效。失败时返回初始化错误，由调用方决定是中止进程还是延迟重试
4. **就绪** — 返回 Brain 实例

```
Brain::new(config: BrainConfig) -> Result<Brain, BrainInitError>
```

`BrainInitError` 与运行时的 `BrainError` 是不同的类型。初始化错误意味着 Brain 无法工作，包括：配置缺失/不合法、连通性检查失败。

### 为什么只初始化一次

- HTTP 连接池是有状态的资源，复用连接避免每次推理都做 TLS 握手
- 配置在运行期间不变（如果需要更换后端，应该创建新的 Brain 实例）
- Brain 本身不持有对话状态，所以不会因为复用而产生数据污染

### 外部使用方式

Brain 实例创建后，调用方通过 `infer` 方法使用，典型模式如下：

```
// 进程启动时，初始化一次
let brain = Brain::new(config)?;

// 之后任意位置、任意次数调用
let request = RequestBuilder::new(&brain.default_model())
    .system("You are a system administrator.")
    .user_text("Check disk usage.")
    .max_tokens(4096)
    .build()?;

let response = brain.infer(request).await?;
```

Brain 实例是 `Send + Sync` 的，可以安全地在多个 tokio task 之间共享（通过 `Arc<Brain>`）。并发调用 `infer` 是安全的，各请求之间互不影响。

### 运行时后端切换

Brain 实例与一个推理后端绑定。如果需要切换后端（比如 MiniMax 不可用，临时切到另一个服务），应该创建一个新的 Brain 实例。上层持有 `Arc<Brain>` 的场景下，可以通过 `ArcSwap` 或类似机制做热替换，但这不是 Brain 模块的职责。

## 不做的事情（显式排除）

- **不做 streaming**：初期只做非 streaming 的请求-响应模式。streaming 作为未来增强，不影响当前接口设计（可以新增一个 `infer_stream` 方法）。
- **不做对话管理**：Brain 不记住上一次调用的内容。对话历史由调用方维护并在每次请求中完整传入。
- **不做 tool 执行**：Brain 返回的 `ToolUse` content block 由调用方自行处理。
- **不做缓存**：相同请求不做响应缓存。推理结果的复用由上层决定。