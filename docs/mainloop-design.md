# Agent Loop Design — Shelly

## 概述

Agent Loop 是 Shelly 的核心编排层，运行在主线程中。它是所有输入的汇聚点和所有决策的发起点。Agent Loop 不自己做推理，不自己执行命令，不自己处理通信——它编排 brain、executor、memory、comm 和 event engine，驱动整个系统运行。

## 生命周期

Agent Loop 的生命周期分为两个阶段，顺序执行：

### 阶段一：初始化推理

在进入主循环之前，同步执行一次预定义 prompt 的推理调用。目的是让 LLM 通过工具调用自主了解当前环境，将结果写入 memory。

```
1. 组装初始化 prompt（预定义内容，描述身份和初始任务）
2. 进入 agent loop 内循环：
   a. 构造 MessageRequest（system prompt + memory context + 初始化 prompt + tool definitions）
   b. brain.infer(request)
   c. 如果 stop_reason == ToolUse → executor.execute() → 结果追加到 messages → 回到 b
   d. 如果 stop_reason == EndTurn → 提取结果，更新 memory
3. 初始化完成
```

初始化 prompt 示例：

```
You just started. You are shelly, a system daemon running on this machine.
Explore your environment: check system metadata, disk usage, network status,
and running services. Use the tools available to you.
```

LLM 自行决定调用哪些工具、调用几次。初始化行为不固定，由 LLM 根据 prompt 自主推理。

### 阶段二：主循环

初始化完成后，进入主循环，同时监听 comm 和 event engine 两个输入源：

```
loop {
    tokio::select! {
        Some(user_req) = user_rx.recv() => {
            handle_user_request(user_req).await;
        }
        Some(sys_event) = event_rx.recv() => {
            handle_system_event(sys_event).await;
        }
    }
}
```

两个输入源独立运行，主循环通过 select! 响应先到达的输入。

## 核心处理流程

不管输入来自 comm 还是 event engine，核心处理逻辑是同一个：

```
fn handle(input: &str) -> Result<String>:
    1. 从 memory 加载当前 context
    2. 从 executor 获取 tool_definitions
    3. 通过 RequestBuilder 组装 MessageRequest：
       - system prompt（身份 + 行为准则）
       - memory context（identity + topology + journal）
       - 当前输入
       - tool definitions
    4. 进入推理循环：
       a. brain.infer(request)
       b. 检查 response.stop_reason：
          - ToolUse → 从 content 提取 tool calls
                     → 逐个调用 executor.execute(name, input)
                     → 将 ToolResult 追加到 messages
                     → 回到 a
          - EndTurn → 提取最终文本响应，跳出循环
          - MaxTokens → 记录警告，以当前内容作为响应
       c. 安全限制：推理循环最大轮次（防止无限 tool call）
    5. 更新 memory（将本次交互的关键信息写入 journal 层）
    6. 返回响应文本
```

### 用户请求处理

```
async fn handle_user_request(req: UserRequest):
    let response = handle(&req.content).await;
    let _ = req.reply.send(UserResponse {
        content: response.unwrap_or_else(|e| format!("error: {e}")),
        is_error: response.is_err(),
    });
```

通过 oneshot channel 将响应发回 comm，comm 编码后 UDP 发给客户端。

### 系统事件处理

```
async fn handle_system_event(event: SystemEvent):
    let input = event.to_prompt();  // 将事件转化为自然语言描述
    let _ = handle(&input).await;
    // 无需回复，结果通过 memory 更新和 executor 操作体现
```

系统事件没有回复通道。LLM 的响应可能触发工具调用（比如发现磁盘满了，自动清理日志），也可能只是更新 memory（记录事件，不采取行动）。

## 退出处理

进程收到 SIGTERM/SIGINT 时，通过 tokio signal handler 注入一个退出事件：

```
tokio::select! {
    Some(user_req) = user_rx.recv() => { ... }
    Some(sys_event) = event_rx.recv() => { ... }
    _ = shutdown_signal() => {
        handle_shutdown().await;
        break;
    }
}
```

`handle_shutdown` 的实现与普通事件处理相同——给 LLM 一个 prompt 说"系统即将关闭"，让它自己决定做什么收尾工作（持久化重要状态、完成关键操作等）。完成后主循环退出，各模块随进程终止。

如果收尾推理超时（配置一个上限），强制退出。

## System Prompt

System prompt 是 agent loop 在每次推理调用时注入的，定义了 shelly 的身份和行为准则。它由以下部分拼接：

```
[identity]       # 你是 shelly，一个系统 daemon
[principles]     # 行为准则（谨慎操作、记录日志等）
[memory context] # 从 memory 模块加载的当前状态
```

identity 和 principles 是静态文本（可从配置文件加载），memory context 每次推理前动态生成。

## 推理循环安全限制

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| max_tool_rounds | 20 | 单次 handle 中 tool call 的最大轮次 |
| init_timeout_secs | 120 | 初始化推理的最大超时 |
| shutdown_timeout_secs | 30 | 退出收尾推理的最大超时 |
| handle_timeout_secs | 300 | 单次用户请求/系统事件处理的最大超时 |

超过 max_tool_rounds 时，中止推理循环，以当前已有的信息作为响应（或返回错误）。这防止 LLM 陷入无限工具调用。

## 与各模块的关系

```
                comm ──── user_rx ────▶┐
                                       │
                event engine ─ event_rx▶├──▶ Agent Loop
                                       │       │
                shutdown signal ───────▶┘       │
                                                │
                           ┌────────────────────┤
                           │                    │
                           ▼                    ▼
                        brain              executor
                           │                    │
                           │                    ▼
                           │              system operations
                           ▼
                        memory
```

- **brain**：agent loop 调用 `brain.infer()` 做推理，brain 不知道调用方是谁
- **executor**：agent loop 调用 `executor.execute()` 执行工具，executor 不知道调用方是谁
- **memory**：agent loop 在推理前读取 memory 组装 context，推理后更新 memory
- **comm**：通过 mpsc + oneshot channel 通信，agent loop 不直接操作 UDP
- **event engine**：通过 mpsc channel 通信，agent loop 不直接操作系统事件源

Agent loop 是唯一知道所有模块存在的地方，但它只通过各模块的公共接口交互。

## 对外表面积

Agent loop 没有对外的公共 API。它是 `main.rs` 中的顶层逻辑，持有所有模块的实例，驱动整个系统。它不被其它模块引用。

## 进程启动顺序

```
main():
    1. 加载配置
    2. 初始化 tracing subscriber（stdout）
    3. 初始化 brain
    4. 初始化 executor
    5. 初始化 memory
    6. 初始化 comm → 获取 user_rx
    7. spawn comm task
    8. （未来）初始化 event engine → 获取 event_rx
    9. （未来）spawn event engine task
    10. 执行初始化推理
    11. 进入主循环
```

各模块初始化失败视为致命错误，进程直接退出。

## 不做的事情（显式排除）

- **不做并发处理**：主循环串行处理每个输入。同一时刻只有一个推理在进行。如果用户请求和系统事件同时到达，先到先处理，后到排队。初期这足够了。
- **不做优先级调度**：所有输入平等，不区分紧急事件和普通请求。
- **不做请求取消**：一旦开始处理，必须完成（或超时）。客户端不能取消正在处理的请求。
- **不做对话历史**：每次 handle 调用是独立的。跨请求的上下文通过 memory 模块传递，不通过对话历史追加。