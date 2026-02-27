# Comm Protocol Verification — Shelly

## 概述

本文档定义 comm 模块协议层的测试用例，用于验证编解码、通信流程、异常处理的正确性。测试分为三个层次：单元测试（编解码）、集成测试（单机 UDP 通信）、端到端测试（daemon + CLI 交互）。

## 一、编解码测试

验证包格式的序列化与反序列化正确性。纯内存测试，不涉及网络。

### T-CODEC-01：REQUEST 编码与解码

- 输入：`type=0x01, seq=1, payload=RequestPayload { content: "hello" }`
- 编码为字节序列
- 解码回结构体
- 断言：所有字段与原始输入一致

### T-CODEC-02：REQUEST_ACK 编码与解码

- 输入：`type=0x02, seq=42`
- REQUEST_ACK 无 payload
- 编码后字节长度应为 5（1 + 4）
- 解码后 seq 一致，payload 为空

### T-CODEC-03：RESPONSE 编码与解码

- 输入：`type=0x03, seq=1, payload=ResponsePayload { content: "result", is_error: false }`
- 编码解码后所有字段一致

### T-CODEC-04：RESPONSE is_error=true

- 输入：`type=0x03, seq=1, payload=ResponsePayload { content: "command not found", is_error: true }`
- 解码后 is_error 为 true

### T-CODEC-05：空 payload REQUEST

- 输入：`type=0x01, seq=1, payload=RequestPayload { content: "" }`
- 应正常编解码，content 为空字符串

### T-CODEC-06：大 payload

- 输入：content 为 60000 字节的字符串（接近 64KB 限制）
- 应正常编解码

### T-CODEC-07：超大 payload 拒绝

- 输入：content 为 70000 字节（超过 64KB 限制）
- 编码阶段或发送阶段应返回 PayloadTooLarge 错误

### T-CODEC-08：非法 type 值

- 输入：字节序列首字节为 0xFF
- 解码应返回 DecodeError

### T-CODEC-09：截断的包

- 输入：只有 3 字节（不足 type + seq 的最小长度 5 字节）
- 解码应返回 DecodeError

### T-CODEC-10：seq 边界值

- seq=0：应正常处理
- seq=u32::MAX：应正常处理
- 验证 big-endian 编码正确性（seq=256 应编码为 `[0x00, 0x00, 0x01, 0x00]`）

### T-CODEC-11：payload 含特殊字符

- content 包含 UTF-8 多字节字符（中文、emoji）
- content 包含 `\n`, `\0`, `\r\n`
- 应正常编解码，内容无损

### T-CODEC-12：MessagePack 兼容性

- 手动构造合法的 MessagePack payload 字节
- 验证解码结果正确
- 确保 rmp-serde 的序列化结果可被其它语言的 MessagePack 实现反序列化（为未来非 Rust 客户端做准备）

## 二、通信流程测试

单机环境，daemon 侧 comm 和测试用客户端在同一进程或同一机器上通过 localhost UDP 通信。

### T-FLOW-01：正常请求-响应

- 客户端发送 REQUEST { seq=1, content="test" }
- 预期收到 REQUEST_ACK { seq=1 }
- 预期收到 RESPONSE { seq=1, content=..., is_error=false }
- 验证 seq 匹配，收到顺序为 ACK 先于 RESPONSE

### T-FLOW-02：连续多次请求

- 客户端依次发送 seq=1, seq=2, seq=3
- 每次都应收到对应 seq 的 REQUEST_ACK 和 RESPONSE
- 验证无串扰：seq=2 的 RESPONSE 不会携带 seq=1 的内容

### T-FLOW-03：REQUEST_ACK 及时性

- 客户端发送 REQUEST
- REQUEST_ACK 应在 comm 收到包后立即发出（不等主 loop 处理完）
- 验证 REQUEST_ACK 到达时间远早于 RESPONSE（在主 loop 有人为延迟的情况下）

### T-FLOW-04：重复 REQUEST 去重

- 客户端发送 REQUEST { seq=1 } 两次
- comm 应只将第一次转发给主 loop
- 第二次收到时应直接回复 REQUEST_ACK（如果尚未有 RESPONSE）或重发已有的 RESPONSE
- 主 loop 侧验证只收到一次 UserRequest

### T-FLOW-05：重复 REQUEST 重发已有 RESPONSE

- 客户端发送 REQUEST { seq=1 }，等待 RESPONSE 返回
- 客户端再次发送 REQUEST { seq=1 }（模拟客户端未收到 RESPONSE 的重传）
- comm 应重发之前缓存的 RESPONSE，不再次提交给主 loop

### T-FLOW-06：乱序 seq

- 客户端依次发送 seq=3, seq=1, seq=2
- 所有三个请求都应被正常处理（seq 不要求连续或有序）

### T-FLOW-07：多客户端并发

- 两个客户端（不同 addr:port）同时发送 REQUEST
- 各自收到各自的 REQUEST_ACK 和 RESPONSE
- 验证无串扰

### T-FLOW-08：多客户端相同 seq

- 客户端 A 发送 seq=1，客户端 B 也发送 seq=1
- 应分别处理（去重是 per 客户端地址的）
- 各自收到各自的响应

### T-FLOW-09：主 loop channel 关闭

- 主 loop 侧 drop mpsc receiver
- comm 尝试发送 UserRequest 时发现 channel 关闭
- comm 向客户端发送错误 RESPONSE
- comm task 退出

### T-FLOW-10：oneshot 被 drop

- 主 loop 收到 UserRequest 后 drop oneshot sender（不回复）
- comm 侧 oneshot receiver 收到 RecvError
- comm 向客户端发送错误 RESPONSE { is_error: true, content: "internal error" }

## 三、异常与边界测试

### T-EDGE-01：空包

- 客户端发送 0 字节 UDP 包
- comm 应 DecodeError，丢弃，不影响后续包的处理

### T-EDGE-02：垃圾数据

- 客户端发送 1024 字节随机数据
- comm 应 DecodeError，丢弃

### T-EDGE-03：只有 header 无 payload

- 发送 5 字节（type=0x01, seq=1），无 payload
- 对于 REQUEST，payload 反序列化应失败（缺少 content 字段）
- 应返回 DecodeError

### T-EDGE-04：非法 REQUEST_ACK 来自客户端

- 客户端发送 type=0x02（REQUEST_ACK）给 shelly
- comm 应忽略（REQUEST_ACK 是 shelly → client 方向的消息类型）

### T-EDGE-05：非法 RESPONSE 来自客户端

- 客户端发送 type=0x03（RESPONSE）给 shelly
- comm 应忽略

### T-EDGE-06：高频发送

- 客户端在 1 秒内发送 1000 个不同 seq 的 REQUEST
- comm 应全部正常 ACK，按顺序转发给主 loop
- 验证无丢包、无崩溃

### T-EDGE-07：去重表容量上限

- 客户端发送超过 dedup_capacity（默认 256）个不同 seq 的请求
- 旧 seq 条目被淘汰后，重发该旧 seq 的 REQUEST
- 应作为新请求处理（去重表已过期，不再识别为重复）

### T-EDGE-08：去重表 TTL 过期

- 客户端发送 REQUEST { seq=1 }，等待处理完成
- 等待超过 dedup_ttl_secs（5 分钟）
- 再次发送 REQUEST { seq=1 }
- 应作为新请求处理

### T-EDGE-09：UDP 源端口变化

- 同一个客户端 IP，先从端口 50000 发送 seq=1，再从端口 50001 发送 seq=1
- 应视为两个不同客户端，分别处理（去重是 per addr:port 的）

### T-EDGE-10：daemon 未启动时客户端发送

- CLI 客户端向未启动 shelly 的地址发送 REQUEST
- 客户端应在超时重试后报告 `[error] shelly not responding`
- 客户端不崩溃，回到提示符

## 测试实现建议

### 单元测试

放在 `src/comm/` 模块内，`#[cfg(test)]` 块中。直接测试编解码函数，不需要网络。

### 集成测试

放在 `tests/test_comm.rs`。在测试中启动 comm task（绑定 localhost 随机端口），用 tokio UdpSocket 模拟客户端发包收包。通过 mpsc channel 检查 comm 转发给主 loop 的消息。