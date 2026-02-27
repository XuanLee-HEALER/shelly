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

不管输入来自 comm 还是 event engine，核心处理逻辑是同一个。流程分为三个阶段：认知循环 → 执行循环 → 记忆写入。

```
fn handle(input: &str) -> Result<String>:

    ┌─────────────────────────────────────────────┐
    │ 阶段一：认知循环（我知不知道）               │
    │                                             │
    │ 目的：在执行前确定 LLM 是否拥有足够的信息    │
    │                                             │
    │ 1. 构造前置请求：                            │
    │    - system prompt（身份定义）                │
    │    - 当前输入                                │
    │    - tool definitions                       │
    │    - prompt：                                │
    │      "Given this input, can you respond      │
    │       directly or act with tools?            │
    │       If not, describe what information      │
    │       you need."                             │
    │                                             │
    │ 2. brain.infer(前置请求)                     │
    │                                             │
    │ 3. 检查 LLM 响应：                           │
    │    a. LLM 直接回答或发起 tool call            │
    │       → 跳出认知循环，进入阶段二              │
    │    b. LLM 描述了需要的信息                    │
    │       → memory.recall(需求描述, top_k)       │
    │       → 找到记忆：注入 context，回到步骤 2    │
    │       → 未找到：告知 LLM 无此记忆，回到步骤 2 │
    │         （LLM 此时应转向直接回答或 tool call） │
    │                                             │
    │ 4. 安全限制：max_cognition_rounds            │
    │    超过则强制进入阶段二                       │
    └─────────────────────────────────────────────┘
                         │
                         ▼
    ┌─────────────────────────────────────────────┐
    │ 阶段二：执行循环（我怎么做）                 │
    │                                             │
    │ 1. 构造完整请求：                            │
    │    - system prompt                          │
    │    - 认知循环中积累的 context（含记忆检索结果）│
    │    - tool definitions                       │
    │                                             │
    │ 2. brain.infer(request)                     │
    │                                             │
    │ 3. 检查 response.stop_reason：               │
    │    - ToolUse → 从 content 提取 tool calls    │
    │              → executor.execute(name, input) │
    │              → 将 ToolResult 追加到 messages  │
    │              → 回到 2                        │
    │    - EndTurn → 提取最终文本响应，跳出循环     │
    │    - MaxTokens → 记录警告，以当前内容作为响应 │
    │                                             │
    │ 4. 安全限制：max_tool_rounds                 │
    └─────────────────────────────────────────────┘
                         │
                         ▼
    ┌─────────────────────────────────────────────┐
    │ 阶段三：记忆写入                             │
    │                                             │
    │ 1. 将本轮完整交互发给 LLM 生成摘要           │
    │ 2. 对摘要生成 embedding                      │
    │ 3. memory.store(entry)                      │
    │ 4. 返回响应文本                              │
    └─────────────────────────────────────────────┘
```

### 认知循环与执行循环的关系

认知循环和执行循环是两个不同的阶段，解决不同的问题。认知循环解决"我知不知道"——LLM 评估自身对当前任务的信息充分性，不足时通过记忆检索补充。执行循环解决"我怎么做"——LLM 在信息充分的前提下，通过工具调用完成任务。

认知循环可能直接跳过：如果 LLM 在第一轮前置请求中就直接回答了或发起了 tool call，说明它认为信息已经足够，认知循环在第一轮就退出。

认知循环也可能与执行循环合并：如果 LLM 在认知循环中发起 tool call（而非描述信息需求），这意味着它跳过了记忆检索直接进入行动。此时认知循环退出，tool call 交由执行循环处理。

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

System prompt 是 agent loop 在每次推理调用时注入的，定义了 shelly 的身份。它是静态文本，从配置文件加载。

```
[identity]       # 你是 shelly，进程身份、权限范围、执行模型
```

System prompt 不包含动态记忆内容。跨交互的上下文通过认知循环中的记忆检索按需注入，而不是每次推理前全量拼接。

## 安全限制

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| max_cognition_rounds | 3 | 认知循环最大轮次（防止无限记忆检索） |
| max_tool_rounds | 20 | 执行循环中 tool call 的最大轮次 |
| init_timeout_secs | 120 | 初始化推理的最大超时 |
| shutdown_timeout_secs | 30 | 退出收尾推理的最大超时 |
| handle_timeout_secs | 300 | 单次用户请求/系统事件处理的最大超时（含认知循环 + 执行循环 + 记忆写入） |

认知循环超过 max_cognition_rounds 时，强制进入执行循环，以当前已有的 context 继续。执行循环超过 max_tool_rounds 时，中止推理，以当前已有的信息作为响应。

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
- **memory**：认知循环中通过 `memory.recall()` 检索相关记忆，handle 结束后通过 `memory.store()` 写入新记忆
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
- **不做对话历史**：每次 handle 调用是独立的。跨请求的上下文通过认知循环中的记忆检索按需获取，不通过对话历史追加。