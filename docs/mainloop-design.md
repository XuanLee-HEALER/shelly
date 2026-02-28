# Agent Loop Design — Shelly

## 概述

Agent Loop 是 Shelly 的核心编排层，运行在主线程中。它是所有输入的汇聚点和所有决策的发起点。Agent Loop 不自己做推理，不自己执行命令，不自己处理通信——它编排 brain、executor、memory、comm 和 event engine，驱动整个系统运行。

## 生命周期

Agent Loop 的生命周期分为两个阶段，顺序执行：

### 阶段一：初始化推理

在进入主循环之前，同步执行一次预定义 prompt 的推理。通过 inference_loop 完成。

```
1. 组装 messages：
   - system prompt（身份定义）
   - tool definitions
   - 初始化 prompt
2. inference_loop(messages)
3. 将结果写入 memory
4. 初始化完成
```

初始化 prompt：

```
You just started. You know nothing about this machine.
Explore your environment and report what you find.
```

LLM 在 inference_loop 内部通过 tool call 自主探索环境，loop 结束时返回最终报告。

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

## Inference Loop（推理单元）

Inference Loop 是 agent loop 的最小推理单元。所有需要调用 brain 的地方都通过这个单元执行。它是一个独立的、可复用的循环：

```
fn inference_loop(messages: &mut Vec<Message>) -> Result<String, InferenceError>:

    loop {
        1. brain.infer(messages)
        2. 检查 response.stop_reason：

           ToolUse →
               从 response.content 提取 tool calls
               executor.execute(name, input) 获取结果
               将 assistant message（含 tool_use）追加到 messages
               将 tool_result 追加到 messages
               → 继续循环（回到 1）

           EndTurn →
               将 assistant message 追加到 messages
               提取文本内容作为结果
               → 返回 Ok(result)

           MaxTokens / Error →
               → 返回 Err(InferenceError)

        3. 安全限制：循环次数超过 max_tool_rounds → 返回 Err
    }
```

每一轮循环是一个完整的 query → think → end_reason 单元。end_reason 决定分支：tool call 则执行工具后继续循环，无 tool call 则结束返回结果，错误则抛出给上层。

Inference Loop 不知道自己被谁调用、为什么调用。它只接收 messages，驱动 brain + executor 循环，返回最终结果或错误。

## 核心处理流程

不管输入来自 comm 还是 event engine，核心处理逻辑是同一个。流程分为三个阶段：认知循环 → 执行循环 → 记忆写入。每个阶段通过调用 inference_loop 完成推理工作。

```
fn handle(input: &str) -> Result<String>:

    ┌─────────────────────────────────────────────┐
    │ 阶段一：认知循环（我知不知道）               │
    │                                             │
    │ 目的：在执行前确定 LLM 是否拥有足够的信息    │
    │                                             │
    │ 1. 组装 messages：                           │
    │    - system prompt（身份定义）                │
    │    - tool definitions                       │
    │    - 前置 prompt + 当前输入：                 │
    │      "Given this input, can you respond      │
    │       directly or act with tools?            │
    │       If not, describe what information      │
    │       you need."                             │
    │                                             │
    │ 2. inference_loop(messages)                  │
    │                                             │
    │ 3. 检查结果：                                │
    │    a. inference_loop 正常返回（LLM 在循环中   │
    │       通过 tool call 行动或直接给出回答）     │
    │       → 跳出认知循环，进入阶段三              │
    │    b. 返回内容是信息需求描述                  │
    │       → memory.recall(需求描述, top_k)       │
    │       → 找到记忆：注入 messages，回到步骤 2   │
    │       → 未找到：告知 LLM 无此记忆，回到步骤 2 │
    │         （LLM 此时应转向直接回答或 tool call） │
    │                                             │
    │ 4. 安全限制：max_cognition_rounds            │
    │    超过则以当前 messages 进入阶段二           │
    └─────────────────────────────────────────────┘
                         │
                         ▼
    ┌─────────────────────────────────────────────┐
    │ 阶段二：补充执行（可选）                     │
    │                                             │
    │ 仅当认知循环达到 max_cognition_rounds 被强制 │
    │ 退出时进入。以当前积累的 messages 调用        │
    │ inference_loop，让 LLM 基于已有信息完成任务。 │
    │                                             │
    │ inference_loop(messages) → result            │
    └─────────────────────────────────────────────┘
                         │
                         ▼
    ┌─────────────────────────────────────────────┐
    │ 阶段三：记忆写入                             │
    │                                             │
    │ 1. 将本轮完整 messages 发给 LLM 生成摘要     │
    │ 2. 对摘要生成 embedding                      │
    │ 3. memory.store(entry)                      │
    │ 4. 返回响应文本                              │
    └─────────────────────────────────────────────┘
```

### 认知循环如何使用 inference_loop

认知循环内部每轮调用 inference_loop，inference_loop 可能在内部就完成了整个任务（LLM 直接 tool call 并得出结果）。此时认知循环拿到最终结果，直接跳到阶段三。

只有当 inference_loop 返回的结果是信息需求描述时，认知循环才进入下一轮：检索记忆，补充 context，再次调用 inference_loop。

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

| 配置项 | 默认值 | 层级 | 说明 |
|--------|--------|------|------|
| max_tool_rounds | 20 | inference_loop | 单次 inference_loop 内 tool call 的最大循环次数 |
| max_cognition_rounds | 3 | handle | 认知循环最大轮次（每轮内部调用一次 inference_loop） |
| init_timeout_secs | 120 | 生命周期 | 初始化推理的最大超时 |
| shutdown_timeout_secs | 30 | 生命周期 | 退出收尾推理的最大超时 |
| handle_timeout_secs | 300 | handle | 单次请求处理的最大超时（含认知循环 + 记忆写入） |

max_tool_rounds 作用于 inference_loop 内部，限制单次推理单元的工具调用次数。max_cognition_rounds 作用于 handle 的认知循环，限制记忆检索的轮次。两个限制独立生效。

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