# Memory Module Design — Shelly

## 概述

Memory 是 Shelly 的记忆模块。它持久化存储所有历史交互产生的信息，并在需要时通过语义检索提供相关上下文。Memory 不参与推理、不解释内容、不做决策。它是一个存储和检索服务。

## 认知循环

Memory 的设计基于一个认知前提：对于任何输入，LLM 首先评估自身是否可知。如果不可知，它描述需要什么信息，系统从记忆中检索并补充。如果记忆中也没有，才进入工具调用获取全新信息。

这个循环改变了 agent loop 的处理流程（详见"对 Agent Loop 的改动"一节）。

## 记忆条目

### 写入内容

每次 handle 结束后，将本次交互写入记忆。初期采用最简方案：

```rust
struct MemoryEntry {
    id: String,              // 唯一标识（UUID）
    timestamp: DateTime,     // 写入时间
    content: String,         // 交互摘要，由 LLM 生成
    embedding: Vec<f32>,     // content 的向量表示
}
```

写入流程：

1. handle 结束后，将本轮交互的完整 messages 发给 LLM，要求生成简洁摘要
2. 对摘要文本生成 embedding 向量
3. 存储 MemoryEntry

### 摘要生成

通过一次额外的推理调用完成。prompt：

```
Summarize this interaction in a concise factual statement.
Focus on: what was requested, what was done, what was the outcome,
and any important details (file paths, service names, configurations)
that might be needed in the future.
```

示例输出：

```
Deployed 3-node redis cluster (redis-node-1/2/3) using docker compose.
Config at /opt/redis-cluster/docker-compose.yml.
Each node exposes ports 6379/6380/6381. Cluster initialized with redis-cli --cluster create.
```

### 全量保留

所有记忆条目永久保留，不做淘汰、不做过期删除。磁盘空间是廉价资源，文本摘要的存储开销极小。保留全量记忆保证了检索的完整性——不会因为淘汰策略丢失关键信息。

## 检索

### 语义检索

LLM 在认知循环中描述"我需要知道什么"，这段自然语言文本作为检索 query。对 query 生成 embedding，与记忆库中所有条目的 embedding 计算相似度，返回 top-k 条目。

```
fn recall(&self, query: &str, top_k: usize) -> Vec<MemoryEntry>
```

### Embedding 生成

初期使用推理后端的 embedding 能力（如果可用），或使用独立的 embedding 模型。具体选型取决于部署环境。

备选方案：如果 embedding 服务不可用，退化为关键词匹配（BM25）。不如语义检索精确，但零依赖。

## 存储

### 初期方案

纯文件存储：

- 记忆条目序列化为 JSON，每条一个文件或全量一个 JSON 文件
- Embedding 向量存储在同一文件中
- 检索时全量加载到内存，暴力计算相似度
- 适用于记忆条目数量在千级别以下的场景

目录结构：

```
~/.shelly/
└── memory/
    └── entries.json      # 全量记忆条目（含 embedding）
```

### 后续演进

当条目数量增长到暴力检索不可接受时，迁移到向量数据库（如 qdrant、milvus）或嵌入式向量索引（如 hnsw）。接口不变，只替换存储后端。

## 对 Agent Loop 的改动

Memory 模块引入后，agent loop 的 handle 流程变为：

```
fn handle(input: &str) -> Result<String>:

    认知循环：
    1. 构造前置请求：system prompt + 当前输入
       "Given this input, can you respond directly?
        If not, describe what information you need."
    2. brain.infer(前置请求)
    3. 检查 LLM 响应：
       a. LLM 直接给出回答或发起 tool call
          → 跳出认知循环，进入正常 agent loop（推理 + 工具调用循环）
       b. LLM 描述了需要的信息
          → memory.recall(需求描述) 检索相关记忆
          → 如果找到：将记忆条目注入 context，回到步骤 2
          → 如果未找到：告知 LLM 记忆中无此信息，回到步骤 2
             （LLM 此时应转向 tool call 或直接回答）
    4. 安全限制：认知循环最大轮次（防止无限检索）

    正常 agent loop：
    5. brain.infer(完整 context)
    6. stop_reason == ToolUse → executor.execute() → 结果追加 → 回到 5
    7. stop_reason == EndTurn → 提取响应

    记忆写入：
    8. 将本轮完整交互发给 LLM 生成摘要
    9. 生成 embedding
    10. memory.store(entry)
    11. 返回响应
```

### 认知循环与正常 agent loop 的关系

认知循环是 agent loop 的前置阶段，不是替代。认知循环解决"我知不知道"的问题，agent loop 解决"我怎么做"的问题。认知循环的出口有两个：LLM 认为自己已经可知（有足够信息），或者循环次数达到上限。之后进入正常的推理 + 工具调用循环。

## 技术栈

| 用途 | 库 | 说明 |
|------|-----|------|
| 序列化 | serde + serde_json | 记忆条目的持久化 |
| 文件操作 | std::fs / tokio::fs | 读写记忆文件 |
| 向量计算 | 手写 cosine similarity | 初期不引入额外依赖，暴力检索 |
| 唯一标识 | uuid | 生成 entry id |
| 错误处理 | thiserror | 定义 MemoryError |
| 结构化日志 | tracing | 记录存储和检索操作 |

## 对外表面积

Memory 对外暴露三个方法：

### `store`

写入一条记忆。

```
async fn store(&self, entry: MemoryEntry) -> Result<(), MemoryError>
```

### `recall`

语义检索，返回最相关的 top-k 条记忆。

```
async fn recall(&self, query: &str, top_k: usize) -> Result<Vec<MemoryEntry>, MemoryError>
```

### `load`

启动时从磁盘加载全量记忆到内存。

```
fn load(config: &MemoryConfig) -> Result<Memory, MemoryError>
```

## 初始化与生命周期

### 初始化

```
Memory::load(config: MemoryConfig) -> Result<Memory, MemoryError>
```

1. 读取记忆文件，反序列化所有条目到内存
2. 如果文件不存在，初始化为空记忆（不报错）
3. 返回 Memory 实例

### 持久化策略

每次 store 后立即写入磁盘（append 或全量重写）。初期全量重写最简单，条目数量大了之后改为 append-only log + 定期 compaction。

### 线程安全

Memory 实例通过 `Arc<RwLock<Memory>>` 共享。store 获取写锁，recall 和 load 获取读锁。在当前串行处理模型下不会有竞争，但接口设计上保留并发安全性。

## 错误处理

### MemoryError

| 错误变体 | 含义 | 说明 |
|----------|------|------|
| LoadFailed | 无法读取或解析记忆文件 | 启动时，可 fallback 为空记忆 |
| StoreFailed | 无法写入记忆文件 | 磁盘满、权限等 |
| EmbeddingFailed | 无法生成 embedding | 模型不可用时退化为无检索 |

记忆模块的失败不应阻塞主流程。store 失败 → 日志警告，本次记忆丢失但不影响当前响应。recall 失败 → 返回空结果，认知循环跳过记忆检索，直接进入工具调用。

## 配置

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| storage_dir | ~/.shelly/memory | 记忆文件目录 |
| top_k | 5 | 检索返回的最大条目数 |
| max_cognition_rounds | 3 | 认知循环最大轮次 |
| embedding_model | 与推理后端一致 | embedding 模型标识 |

## 验证

所有测试放在 `tests/test_memory.rs` 中。

### 一、存储测试

#### T-MEM-STORE-01：写入单条记忆

- 调用 `memory.store(entry)`
- 验证 `memory.recall()` 能检索到该条目
- 验证持久化文件已写入磁盘

#### T-MEM-STORE-02：写入多条记忆

- 依次写入 10 条不同内容的记忆
- 验证全部可检索
- 验证持久化文件包含所有条目

#### T-MEM-STORE-03：持久化与重新加载

- 写入若干条记忆
- 销毁 Memory 实例
- 通过 `Memory::load()` 重新加载
- 验证所有条目完整恢复（content、timestamp、embedding）

#### T-MEM-STORE-04：空记忆初始化

- 记忆文件不存在时调用 `Memory::load()`
- 应成功返回空记忆实例，不报错
- 后续 store 和 recall 正常工作

#### T-MEM-STORE-05：记忆文件损坏

- 手动写入非法 JSON 到记忆文件
- `Memory::load()` 应返回 `MemoryError::LoadFailed`
- 或 fallback 为空记忆（取决于策略选择）

#### T-MEM-STORE-06：并发写入安全

- 多个 task 同时调用 `memory.store()`
- 所有条目都应成功写入，无数据丢失或损坏

#### T-MEM-STORE-07：UTF-8 内容

- content 包含中文、emoji、特殊字符
- 写入后重新加载，内容无损

### 二、检索测试

#### T-MEM-RECALL-01：语义相关性

- 存入："Deployed 3-node redis cluster, config at /opt/redis/"
- 检索 query："redis 集群的配置在哪里"
- 应返回该条目且排名靠前

#### T-MEM-RECALL-02：top_k 限制

- 存入 20 条记忆
- recall(query, top_k=3)
- 返回恰好 3 条

#### T-MEM-RECALL-03：空记忆检索

- 记忆为空时调用 recall
- 应返回空列表，不报错

#### T-MEM-RECALL-04：无关 query

- 存入若干关于 redis 的记忆
- 检索 query："天气怎么样"
- 返回的条目相似度应很低（验证不会胡乱匹配）

#### T-MEM-RECALL-05：时间顺序区分

- 存入两条相似内容但时间不同的记忆：
  - "Redis cluster deployed with 3 nodes"（旧）
  - "Redis cluster expanded to 5 nodes"（新）
- 检索 "redis cluster 有几个节点"
- 两条都应返回，LLM 根据 timestamp 判断哪个是最新状态

#### T-MEM-RECALL-06：大量记忆检索性能

- 存入 1000 条记忆
- recall 应在合理时间内返回（暴力检索下 < 1 秒）

### 三、Embedding 测试

#### T-MEM-EMB-01：相同文本生成相同向量

- 对同一段文本调用两次 embedding 生成
- 两次结果应完全一致

#### T-MEM-EMB-02：相似文本生成相近向量

- "deployed redis cluster" 和 "set up redis cluster"
- cosine similarity 应较高（> 0.8）

#### T-MEM-EMB-03：不相关文本生成远离向量

- "deployed redis cluster" 和 "went to the grocery store"
- cosine similarity 应较低（< 0.3）

#### T-MEM-EMB-04：Embedding 服务不可用

- 模拟 embedding 生成失败
- store 应返回 `MemoryError::EmbeddingFailed`
- 或 fallback 到无 embedding 存储（后续检索退化为关键词匹配）

### 四、Cosine Similarity 测试

#### T-MEM-COS-01：相同向量

- cosine_similarity([1,0,0], [1,0,0]) == 1.0

#### T-MEM-COS-02：正交向量

- cosine_similarity([1,0,0], [0,1,0]) == 0.0

#### T-MEM-COS-03：反向向量

- cosine_similarity([1,0,0], [-1,0,0]) == -1.0

#### T-MEM-COS-04：零向量处理

- cosine_similarity([0,0,0], [1,0,0]) 应返回 0.0 或 NaN，不 panic

#### T-MEM-COS-05：高维向量

- 1536 维向量（常见 embedding 维度）计算结果正确

## 不做的事情（显式排除）

- **不做记忆淘汰**：全量保留，不做 LRU、TTL、容量限制。
- **不做记忆分层**：初期不区分短期/长期记忆。所有记忆平等存储、平等检索。
- **不做图结构**：不建模实体关系。初期纯向量检索，结构化知识图谱是后续演进方向。
- **不做记忆修改**：记忆条目写入后不可变。不做更新、不做合并、不做冲突解决。新的事实通过新条目覆盖旧信息（检索时新条目的 timestamp 更近，LLM 自行判断哪个更准确）。
- **不做实时索引**：暴力检索，不建增量索引。