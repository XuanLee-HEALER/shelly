# Comm Module Design — Shelly

## 概述

Comm 是 Shelly 的通信模块。它负责通过 UDP 自定义协议与外部客户端（人类）进行通信。Comm 不参与任何决策逻辑，不解释消息内容的语义。它只做传输：接收外部消息、解码、通过 channel 发给主 loop；从主 loop 收到响应后编码、通过 UDP 发出去。

## 技术栈

| 用途 | 库 | 说明 |
|------|-----|------|
| UDP socket | tokio::net::UdpSocket | 异步 UDP，与项目统一的 tokio runtime 一致 |
| 消息序列化 | rmp-serde + serde | MessagePack 格式，二进制紧凑，serde 生态原生支持 |
| 跨 task 通信 | tokio::sync::mpsc + oneshot | mpsc 送消息给主 loop，oneshot 接收响应 |
| 错误处理 | thiserror | 定义 CommError 类型化错误 |
| 结构化日志 | tracing | 记录收发包、协议事件 |

## 职责边界

### Comm 负责

- 绑定 UDP 端口，持续监听
- 接收 UDP 数据包，解码为内部协议消息
- 协议层处理：ACK 发送、seq 去重、超时重传（响应侧）
- 将用户请求通过 channel 发给主 loop
- 从主 loop 收到响应后编码，通过 UDP 发回客户端
- 记录通信日志（收发包、来源地址、seq、延迟）

### Comm 不负责

- 消息内容的语义解释（那是主 loop 的事）
- 决定如何响应用户请求（那是主 loop 的事）
- 与 event engine 的交互（event engine 有自己的 channel）
- 认证、加密（初期不做）
- 会话管理（无 session，按来源地址区分客户端）

## 协议设计

### 设计原则

- 无 session、无握手、无状态
- 请求-响应模式，seq 做匹配和去重
- 三种消息类型，最小化协议复杂度
- 客户端通过来源 `addr:port` 区分（未来如需多设备识别，扩展 client_id 字段即可）

### 消息类型

| type 值 | 名称 | 方向 | 说明 |
|---------|------|------|------|
| 0x01 | REQUEST | Client → Shelly | 客户端发送请求 |
| 0x02 | REQUEST_ACK | Shelly → Client | Shelly 确认收到请求，正在处理 |
| 0x03 | RESPONSE | Shelly → Client | Shelly 返回处理结果 |

### 包格式

```
┌──────────┬──────┬─────────────┐
│ type (1B)│seq(4B)│ payload(var)│
└──────────┴──────┴─────────────┘
```

| 字段 | 大小 | 说明 |
|------|------|------|
| type | 1 字节 | 消息类型枚举（0x01 / 0x02 / 0x03） |
| seq | 4 字节 | 序列号，big-endian u32，客户端生成，单调递增 |
| payload | 可变 | MessagePack 编码的消息体，REQUEST_ACK 无 payload |

### 通信流程

```
Client                          Shelly
  │                               │
  │── REQUEST { seq=1, payload }──▶│
  │                               │── 解码，去重检查
  │◀── REQUEST_ACK { seq=1 } ─────│── 立即回复 ACK
  │                               │── 通过 channel 发给主 loop
  │                               │── 主 loop 处理（可能耗时较长）
  │◀── RESPONSE { seq=1, payload }│── 收到主 loop 响应，编码发回
  │                               │
```

### Seq 去重

Shelly 维护一个有限大小的已处理 seq 集合（per 客户端地址）。收到 REQUEST 时：

- seq 已存在：丢弃，重发上次的 RESPONSE（如果有）或重发 REQUEST_ACK
- seq 不存在：正常处理，记录 seq

集合按时间淘汰旧条目，避免无限增长。

### 超时与重传

**客户端侧**（不是 comm 的职责，但协议需要定义预期行为）：

- 发送 REQUEST 后，若 N 秒内未收到 REQUEST_ACK，重传 REQUEST
- 收到 REQUEST_ACK 后，等待 RESPONSE，超时时间应较长（推理可能耗时）
- 重传使用相同 seq

**Shelly 侧（comm 负责）**：

- 收到 REQUEST 后立即发送 REQUEST_ACK，不等主 loop 处理
- RESPONSE 发出后不主动重传（客户端未收到会重发 REQUEST，comm 通过去重机制重发 RESPONSE）

### Payload 格式

REQUEST payload：

```rust
struct RequestPayload {
    content: String,    // 用户输入的文本
}
```

RESPONSE payload：

```rust
struct ResponsePayload {
    content: String,    // Shelly 的回复文本
    is_error: bool,     // 是否为错误响应
}
```

初期只有文本交互。后续扩展（比如文件传输、结构化命令）通过增加 payload 字段实现，不影响协议层。

### 分包

UDP 单包大小受 MTU 限制（通常 ~1400 字节安全值）。当 payload 超过单包容量时：

初期方案：限制单条消息最大长度（比如 64KB），超出报错。LLM 的单次响应文本通常不会超过这个限制。

后续如有需要，扩展分片机制：在包头增加 fragment 字段（total_fragments + fragment_index），接收方拼装。但初期不实现。

## 对外表面积

Comm 对外暴露两样东西：

1. **启动方法** — `run`
2. **channel 端** — 主 loop 侧的 receiver

### `run`

Comm 的主循环，作为独立的 tokio task 运行。

```
async fn run(self, sender: mpsc::Sender<UserRequest>) -> Result<(), CommError>
```

启动后持续监听 UDP socket，收到合法 REQUEST 后构造 `UserRequest`（包含 payload + oneshot sender），通过 mpsc channel 发给主 loop。当主 loop 通过 oneshot 回复后，comm 编码 RESPONSE 发回客户端。

### UserRequest

Comm 发给主 loop 的消息类型：

```rust
struct UserRequest {
    content: String,
    reply: oneshot::Sender<UserResponse>,
    source_addr: SocketAddr,
}
```

### UserResponse

主 loop 回复给 comm 的消息类型：

```rust
struct UserResponse {
    content: String,
    is_error: bool,
}
```

这两个类型定义在共享类型模块中。Comm 不关心 content 里是什么，主 loop 不关心消息从哪来。

## 与主 Loop 的交互

```
                    mpsc::channel
Comm ──────────────────────────────────▶ Main Loop
  │   UserRequest { content, reply_tx }       │
  │                                           │
  │◀──────────────────────────────────── reply │
      oneshot  UserResponse { content }       │
```

Comm task 和主 loop 通过 channel 解耦。Comm 发出 UserRequest 后 await oneshot receiver，拿到 UserResponse 后编码发回客户端。如果 oneshot receiver 被 drop（主 loop 崩溃），comm 向客户端发送一个错误 RESPONSE。

## 初始化与生命周期

### 初始化

```
Comm::new(config: CommConfig) -> Result<(Comm, mpsc::Receiver<UserRequest>), CommInitError>
```

初始化过程：

1. **加载配置** — 监听地址、端口、buffer 大小
2. **绑定 UDP socket** — 失败返回 CommInitError
3. **创建 channel** — mpsc channel，返回 receiver 给调用方（主 loop 持有）
4. **就绪** — 返回 Comm 实例

调用方拿到 Comm 实例后 spawn 为独立 task：

```
let (comm, user_rx) = Comm::new(config)?;
tokio::spawn(comm.run());

// 主 loop 从 user_rx 接收消息
loop {
    tokio::select! {
        Some(req) = user_rx.recv() => { /* 处理用户请求 */ }
        // 未来：Some(evt) = event_rx.recv() => { /* 处理系统事件 */ }
    }
}
```

### 生命周期

Comm task 在 shelly 进程存活期间持续运行。它通过 UDP socket 的生命周期绑定——socket 关闭则 task 结束。正常关闭通过 tokio CancellationToken 或 drop channel 触发。

## 错误处理

### CommError

| 错误变体 | 含义 | 说明 |
|----------|------|------|
| BindFailed | UDP socket 绑定失败 | 初始化阶段，端口被占用等 |
| RecvError | 接收数据包失败 | 运行时 socket 错误 |
| SendError | 发送数据包失败 | 运行时 socket 错误 |
| DecodeError | 数据包解码失败 | 格式不合法，丢弃该包，不中断运行 |
| PayloadTooLarge | 消息超过最大限制 | 回复错误 RESPONSE，不中断运行 |
| ChannelClosed | 主 loop 侧 channel 关闭 | 主 loop 已退出，comm 应停止运行 |

DecodeError 和 PayloadTooLarge 是包级别的错误，不影响 comm 整体运行。BindFailed 是致命错误。ChannelClosed 触发 comm 优雅退出。

## 配置

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| listen_addr | 0.0.0.0 | 监听地址 |
| listen_port | 9700 | 监听端口 |
| max_payload_bytes | 65536 | 单条消息最大 payload（64KB） |
| recv_buffer_size | 65536 | UDP 接收缓冲区大小 |
| dedup_capacity | 256 | 每客户端 seq 去重表容量 |
| dedup_ttl_secs | 300 | 去重表条目过期时间（5 分钟） |

## 内部日志

每次收发包，Comm 记录结构化日志：

| 字段 | 说明 |
|------|------|
| timestamp | 时间 |
| direction | recv / send |
| msg_type | REQUEST / REQUEST_ACK / RESPONSE |
| seq | 序列号 |
| source_addr | 客户端地址 |
| payload_bytes | payload 大小 |
| is_duplicate | 是否为重复包 |

通过 tracing 输出，当前阶段打印到 stdout。

## CLI 客户端（shelly-cli）

CLI 客户端是 shelly 项目中的一个独立 bin crate，与 daemon 共享协议层代码。

### 项目结构

```
shelly/
├── Cargo.toml
├── src/
│   ├── main.rs            # shelly daemon
│   ├── comm/              # 协议层（编解码、消息类型），daemon 和 CLI 共享
│   └── ...
├── src/bin/
│   └── shelly-cli.rs      # CLI 客户端
```

Cargo.toml 中定义两个 bin target：

```toml
[[bin]]
name = "shelly"
path = "src/main.rs"

[[bin]]
name = "shelly-cli"
path = "src/bin/shelly-cli.rs"
```

`cargo run --bin shelly` 启动 daemon，`cargo run --bin shelly-cli` 启动客户端。

### 职责

- 从 stdin 逐行读取用户输入
- 分配 seq（本地 u32 计数器，从 1 单调递增）
- 使用共享的协议编码层构造 REQUEST 包，UDP 发送给 shelly
- 等待 REQUEST_ACK，超时重传
- 等待 RESPONSE，解码后打印 content 到 stdout
- 回到读取输入，等待下一轮交互

### 运行时行为

```
$ cargo run --bin shelly-cli
> df -h
[waiting...]
Filesystem      Size  Used Avail Use% Mounted on
/dev/vda1        30G   12G   18G  40% /
>
```

`>` 是输入提示符。`[waiting...]` 表示已收到 REQUEST_ACK，正在等待 RESPONSE。RESPONSE 到达后打印内容，回到提示符。

### 配置

通过命令行参数或环境变量：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| --target | 127.0.0.1:9700 | shelly daemon 的 UDP 地址 |
| --timeout | 5 | REQUEST_ACK 等待超时秒数 |
| --max-retries | 3 | REQUEST 最大重传次数 |

### 错误处理

- REQUEST_ACK 超时且重试耗尽：打印 `[error] shelly not responding` 回到提示符
- RESPONSE 解码失败：打印 `[error] invalid response` 回到提示符
- UDP send 失败：打印 `[error] network error` 回到提示符
- 用户输入 Ctrl+C 或 EOF：优雅退出

## 不做的事情（显式排除）

- **不做 session**：无握手、无状态，按来源地址区分客户端。
- **不做加密**：初期明文传输。后续可在协议层叠加 DTLS 或自定义对称加密，不影响上层消息格式。
- **不做认证**：初期不验证客户端身份。后续可通过 pre-shared key 或 challenge-response 机制扩展。
- **不做分片**：初期限制单条消息最大 64KB。后续如需大消息传输，扩展分片机制。
- **不做多路复用**：一个 REQUEST 对应一个 RESPONSE，不支持流式响应。后续如需流式输出，可扩展 STREAM_CHUNK 消息类型。
- **不做消息语义解释**：comm 不看 payload 里的内容，只做传输。