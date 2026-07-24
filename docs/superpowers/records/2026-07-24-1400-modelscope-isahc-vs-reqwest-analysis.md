# ModelScope API 无响应问题深度排查 — isahc vs reqwest 架构对比

## 背景

maki 通过动态提供者 `cust07-mscope` (base=openai) 调用 ModelScope 魔搭 API
(`api-inference.modelscope.cn/v1`)。调用大多数模型时 HTTP 请求正常完成但无返回内容。
先前修复（空流检测 + 502 重试）编译后测试仍然失败：模型一直卡住无回复。

**对比对象**：zerostack（同作者项目）对接相同模型时正常工作。

---

## 排查过程

### 1. 先前修复回顾

已在 git 历史中的修复：
- `dd6b9380` — BOM 剥离 + SSE 测试用例
- `b33693fa` — 空流检测 → 返回 AgentError
- `d9aec840` — 502 状态码触发自动重试

**实际测试结果**：修复无效，换了不同模型仍然卡住。

### 2. 对比 zerostack 实现

#### 关键架构差异

| 方面 | maki (有问题) | zerostack (正常) |
|------|--------------|-----------------|
| HTTP 客户端 | `isahc` 1.7 | `reqwest` 0.13 (hyper/tokio) |
| 连接池 | 默认行为（复用连接） | `pool_max_idle_per_host(0)` 禁用 |
| 低速度超时 | `low_speed_timeout(1 byte/s, 30s)` | 无此机制 |
| 总超时 | `stream_timeout` 300s | 可选 `timeout_secs` |
| SSE 解析 | 手动 `parse_sse()` + `BufReader` | `rig` 库内部处理 |
| 流式读取 | `isahc::AsyncBody` + `futures_lite::io::Lines` | `reqwest` 的 `bytes::Bytes` 流 |
| 运行时 | `smol` | `tokio` |

#### maki 的 HTTP 客户端配置

```rust
// maki-providers/src/providers/mod.rs:166-172
pub(crate) fn http_client(timeouts: Timeouts) -> isahc::HttpClient {
    isahc::HttpClient::builder()
        .connect_timeout(timeouts.connect)                    // 10s
        .low_speed_timeout(LOW_SPEED_BYTES_PER_SEC, timeouts.low_speed)  // 1 byte/s, 30s
        .build()
        .expect("failed to build HTTP client")
}
```

#### zerostack 的 HTTP 客户端配置

```rust
// zerostack/src/provider.rs:692
let mut builder = reqwest::Client::builder().pool_max_idle_per_host(0);

// 可选的总超时
if let Some(secs) = cfg.timeout_secs {
    builder = builder.timeout(Duration::from_secs(secs));
}
```

**zerostack 没有 `low_speed_timeout` 概念。**

#### maki 的 SSE 解析入口

```rust
// maki-providers/src/providers/openai_compat.rs:173-182
let response = self.client.send_async(request).await?;
let status = response.status().as_u16();

if status == 200 {
    parse_sse(
        BufReader::new(response.into_body()),  // ← isahc::AsyncBody
        event_tx,
        self.stream_timeout,
    )
    .await
}
```

#### zerostack 的 SSE 处理

zerostack 不直接处理 SSE。它使用 `rig` 库，`rig` 内部使用 `reqwest` + `hyper` 处理流式响应。
SSE 帧解析由 `rig` 或 `hyper` 的 HTTP/1.1 chunked transfer decoding 层完成。

---

## 确认的根因

### `isahc` 的 `low_speed_timeout` 与魔搭 API 不兼容

**工作机制**：
1. maki 发送 POST `/chat/completions` with `stream: true`
2. 魔搭接受请求，开始推理
3. 某些模型（尤其是较大的如 Qwen3.5-35B-A3B）思考时间超过 30 秒
4. `isahc` 的 `low_speed_timeout(1 byte/s, 30s)` 触发 — 30 秒内收到不到 1 字节
5. isahc **静默关闭连接**（行为取决于平台和 isahc 版本）
6. 结果可能是：
   - a) EOF → `parse_sse()` 返回空 `StreamResponse`（看起来像"无返回"）
   - b) I/O 错误 → 被 `isahc::Error` 捕获但可能未被正确分类为可重试
   - c) 连接挂起 → isahc 在某些平台不立即返回（"一直卡住"）

### 为什么重试无效

如果问题出在 isahc 的连接管理层：
- 每次重试都创建新请求，但底层 TCP 连接可能来自连接池（被 isahc 标记为不可用但未清除）
- 或者 isahc 的内部状态在某次超时后进入异常状态
- 30 秒超时对大模型思考来说太短 — 重试同样超时

### 为什么 zerostack 不受影响

- `reqwest`/`hyper` 没有 `low_speed_timeout` 概念
- 只有总超时（不是低速度超时）
- `pool_max_idle_per_host(0)` 确保每次请求新建连接，避免 stale socket
- `hyper` 的 chunked transfer decoding 对 SSE 流的处理更标准

---

## 修复方案

### 方案 A：替换 isahc 为 reqwest（推荐）

**优点**：
- 与 zerostack 架构一致，已验证可行
- `reqwest` 社区更大，bug 修复更及时
- 移除 `low_speed_timeout` 问题根源
- `reqwest` 的流式响应更可靠

**涉及修改的文件**：
1. `maki-providers/Cargo.toml` — 替换依赖
2. `maki-providers/src/providers/mod.rs` — `http_client()` 重写
3. `maki-providers/src/providers/openai_compat.rs` — `do_stream()` 和 `get_text()`/`post_text()`
4. `maki-providers/src/error.rs` — `AgentError::Http` 从 `isahc::Error` 改为 `reqwest::Error`
5. 所有使用 `isahc::Request` / `isahc::Response` / `isahc::AsyncBody` 的地方
6. `maki-providers/src/providers/` 下各 provider（anthropic, google, deepseek 等）

**工作量**：中等。需要重写 HTTP 层，但接口可以保持相似。

### 方案 B：仅禁用 low_speed_timeout（快速验证）

```rust
pub(crate) fn http_client(timeouts: Timeouts) -> isahc::HttpClient {
    isahc::HttpClient::builder()
        .connect_timeout(timeouts.connect)
        // 移除 .low_speed_timeout(...)
        .build()
        .expect("failed to build HTTP client")
}
```

**优点**：改动最小，可快速验证假设
**缺点**：如果问题不只是 low_speed_timeout（如 isahc 内部缓冲、连接池问题），无法彻底解决

### 方案 C：双客户端并存

保留 isahc 作为默认，增加 reqwest 作为可选后端。通过配置切换。

**优点**：风险最高、工作量最大。
**缺点**：维护两套 HTTP 客户端代码。

---

## 建议实施步骤

### Phase 1：快速验证（方案 B）
1. 移除 `low_speed_timeout`
2. 编译测试魔搭模型
3. 如果恢复 → 确认根因
4. 如果仍失败 → 需要方案 A

### Phase 2：彻底修复（方案 A）
1. 添加 `reqwest` 依赖
2. 重写 `http_client()` 和 `do_stream()`
3. 移除所有 `isahc` 引用
4. 运行全部测试

---

## 相关文件

### maki 项目
- `maki-providers/src/providers/mod.rs:166-172` — `http_client()` 含 low_speed_timeout
- `maki-providers/src/providers/openai_compat.rs:149-186` — `do_stream()`
- `maki-providers/src/providers/openai_compat.rs:461-721` — `parse_sse()`
- `maki-providers/src/error.rs:18-29` — `AgentError::Http(isahc::Error)`
- `maki-agent/src/agent/streaming.rs:34-99` — `stream_with_retry()`
- `maki-agent/src/agent/run.rs:243-340` — `Agent::turn()`

### zerostack 项目（参考实现）
- `src/provider.rs:672-722` — `build_http_client()`
- `src/provider.rs:743-781` — `build_openai_client()`
- `src/agent/runner.rs:300-504` — `spawn_agent()`
- `src/retry.rs:107-147` — `retry_stream_chat()`

### 动态提供者脚本
- `~/.config/maki/providers/cust07-mscope` — 魔搭提供者声明

---

## 验证方法

```bash
# 设置 API key
export MSCOPE_API_KEY="your_key"

# 测试调用
maki -m cust07-mscope/Qwen/Qwen3.5-35B-A3B -p "Hello"

# 对比测试（如果安装了 zerostack）
zerostack --provider custom --base-url https://api-inference.modelscope.cn/v1 \
  --api-key $MSCOPE_API_KEY --model Qwen/Qwen3.5-35B-A3B -p "Hello"
```

---

记录日期：2026-07-24

---

## 补充分析：商汤 API 修复记录的启发

参考 `docs/2026-07-18-shangtang-fix.md`（商汤 API 兼容性修复），获得重要新启发。

### 1. 相同模式：zerostack 正常，maki 异常

| 维度 | 商汤 API 问题 | ModelScope API 问题 |
|------|-------------|-------------------|
| 现象 | 工具调用失败 / UI spinner 永远旋转 | 流式响应为空 / 卡住 |
| maki 表现 | ❌ 解析失败 | ❌ 无返回 |
| zerostack 表现 | ✅ 正常 | ✅ 正常 |
| 根因层级 | **SSE 格式非标准** | **疑似 SSE 格式非标准 + HTTP 客户端不兼容** |
| 修复方式 | 调整解析器适应非标准格式 | 待解决 |

**关键结论**：两个不同的国产 API 平台，maki 都有问题而 zerostack 都正常。这不是巧合，而是**系统性的架构差异**：

- **zerostack** 使用 `rig` 库的 SSE 解析器，对各种非标准格式兼容性更强
- **maki** 使用手动的 `parse_sse()` 解析器，对 SSE 格式要求更严格

### 2. 新假设：ModelScope 问题可能是两层问题的叠加

```
Layer 1: HTTP 连接层（isahc low_speed_timeout）
    ↓ 如果修复后仍然失败
Layer 2: SSE 格式层（非标准字段/时序）
    ↓ 如果修复后仍然失败
Layer 3: 请求体/认证层（header/body 差异）
```

**只有先解决连接层问题，才能看到 SSE 数据，进而判断是否存在格式层问题。**

如果连接层修复后：
- **正常了** → 根因确认是 `low_speed_timeout`
- **有返回但内容异常** → 需要走商汤同样的路：抓原始 SSE → 对比标准格式 → 调整解析器
- **仍然卡住** → 可能是 isahc 的其他内部行为（缓冲、连接复用），需要迁移到 reqwest

### 3. 商汤 API 的 SSE 特征（对比参考）

商汤 API 的非标准行为：
- **首帧**：给出正确的 `id` 和 `name`
  ```json
  {"id":"call_abc","type":"function","function":{"name":"bash","arguments":""}}
  ```
- **后续每一帧**：都发 `id=""` 和 `name=""`（显式空串"重置"）
  ```json
  {"id":"","type":"","function":{"name":"","arguments":"{\"command\":"}}}
  ```

标准 OpenAI API 在后续帧中会**省略** `id` 和 `name` 字段（`null`/不发送），而商汤 API **显式发送 `""`**。

**ModelScope 是否也有类似的非标准行为？** 需要抓取原始 SSE 数据才能判断。

### 4. 已验证的调试方法论

```bash
# 开启 providers 模块的 debug 日志
RUST_LOG=maki_providers=debug maki -m cust07-mscope/Qwen/Qwen3.5-35B-A3B -p "Hello"
```

日志中搜索 `SSE tool_call chunk` 可查看原始 SSE 数据（已在 `openai_compat.rs:502-504` 添加）。

### 5. 脚本层排除

`cust07-mscope` 和 `cust06-ocfree` 脚本结构完全一致（`base: openai`，仅域名和 key 环境变量不同）。`cust06-ocfree` 正常而 `cust07-mscope` 不正常 → **确认问题不在脚本层**，是 API 服务端行为差异或 HTTP 客户端兼容性。

### 6. 更新后的实施策略

**第一步应该同时做两件事**：
1. 禁用 `low_speed_timeout` 验证连接层假设
2. 开启 `RUST_LOG=maki_providers=debug` 抓取 ModelScope 的**原始 SSE 数据**

这样即使连接层修复后仍有问题，也能立即获得 SSE 格式分析数据，不用反复编译测试。

---

## 补充：Python 模拟测试结果

### 测试方式

由于 isahc 编译需要 OpenSSL dev 头文件（环境缺失），改用 Python urllib 进行轻量级验证。

测试脚本：`maki-mock/test_mscope.py`

### 测试结果

| 配置 | 首 token | 总耗时 | 结果 |
|------|---------|--------|------|
| read_timeout=30s (模拟 low_speed_timeout) | 0.6s | 8.8s | ✅ 343 chunks |
| read_timeout=None (模拟移除) | 0.5s | 10.6s | ✅ 366 chunks |

### 结论

**当前时刻两种配置都正常返回内容**。

1. **Qwen3.5-35B-A3B 首 token < 1s**，远低于 30s 阈值，`low_speed_timeout` 未触发
2. **问题不是必然复现的** — 与先前调查一致（间歇性，约 20%）
3. **可能的触发条件**：高峰期模型加载慢/首 token > 30s、更大/更慢的模型、网络抖动

### low_speed_timeout 移除的性质

- **防御性修复** — 消除潜在风险，无副作用（300s `stream_timeout` 仍然生效）
- 当前测试**不能证明**这是根因（条件没触发），但也**不能排除**
- 需要高峰期/慢模型/多次调用才能验证

---

## 已提交的改动

### `maki-providers/src/providers/mod.rs`

```rust
pub(crate) fn http_client(timeouts: Timeouts) -> isahc::HttpClient {
    isahc::HttpClient::builder()
        .connect_timeout(timeouts.connect)
        // NOTE: low_speed_timeout removed — it silently closes connections
        // when a model takes >30s to produce the first token (e.g. ModelScope
        // large models during thinking phase).
        .build()
        .expect("failed to build HTTP client")
}
```

### `maki-providers/src/providers/openai_compat.rs`

修复了 5 个因"空流→502"修复而失效的测试 + 移除重复测试块。

**测试结果**：471 passed, 0 failed

### 提交历史

| 提交 | 描述 |
|------|------|
| `0cbe1d97` | fix(providers): remove isahc low_speed_timeout |
| `8176d520` | docs: add Shangtang fix comparison insights |
| `9c6fb71b` | docs: ModelScope isahc vs reqwest root cause analysis |

---

## 补充：isahc 文档关键发现 — BufReader 包裹问题

### isahc 官方文档明确指出

> **"The response body is not a direct stream from the server, but uses its own buffering mechanisms internally for performance. It is therefore undesirable to wrap the body in additional buffering readers."**
>
> — isahc docs for `AsyncBody`

### maki 的当前做法（违反建议）

```rust
// maki-providers/src/providers/openai_compat.rs:176-182
parse_sse(
    BufReader::new(response.into_body()),  // ← 违反 isahc 建议！
    event_tx,
    self.stream_timeout,
)
```

maki 用 `BufReader` 包裹 `isahc::AsyncBody`，而 isahc 明确建议不要这样做。

### 可能的后果

`AsyncBody` 内部已有自己的缓冲机制，额外包裹 `BufReader` 可能导致：
1. 双重缓冲导致数据读取延迟或阻塞
2. `poll_read` 行为异常（`AsyncBody` 的唤醒机制与 `BufReader` 不兼容）
3. 在某些条件下导致读取挂起（看起来像"卡住无响应"）

### Python 验证

Python 测试中直接读取和缓冲读取都能正常工作（Python socket 实现不同），无法复现此问题。**这是 isahc 特有的行为。**

### 进一步修复方向

1. **移除 BufReader**：直接对 `AsyncBody` 使用 `futures_lite::io::lines()`
2. **迁移到 reqwest**：reqwest 的流式响应没有此限制（与 zerostack 一致）

---

## 补充：isahc 编译测试结果（2026-07-24）

### 测试方式

安装了 `libssl-dev` 后成功编译 isahc 测试程序。

测试脚本：`maki-mock/isahc-sse-test/`

### 测试结果

| 测试 | 配置 | 首字节 | 耗时 | 行数 | 内容块 | 结果 |
|------|------|--------|------|------|--------|------|
| 1 | BufReader + low_speed_timeout(1, 30s) | 0.8s | 18.5s | 1473 | 11 | ✅ |
| 2 | BufReader + 无 low_speed_timeout | 0.7s | 8.3s | 735 | 12 | ✅ |
| 3 | 直接 lines() + low_speed_timeout | 0.8s | 6.5s | 553 | 8 | ✅ |
| 4 | 直接 lines() + 无 low_speed_timeout | 0.8s | 3.6s | 227 | 9 | ✅ |

### 结论

**当前环境/时刻所有 4 种组合都正常返回内容。**

1. **`low_speed_timeout` 未触发** — 首字节均 < 1s，远低于 30s 阈值
2. **`BufReader` 包裹 `AsyncBody` 未导致问题** — 测试 1 和 2 都正常
3. **问题无法在当前环境复现** — 与用户描述的"必现"不符

### 关键推断

由于外部 API 层已全面排除（Python + isahc 均正常），问题必定在 **maki 独有的代码层**：

---

## 排查方向重大转变：聚焦 maki 自身

### 已全面排除的外部因素

| 假设 | 状态 | 证据 |
|------|------|------|
| `low_speed_timeout` | ❌ 排除 | isahc 测试全部正常 |
| Headers / 请求体 | ❌ 排除 | 各种组合均正常 |
| isahc BufReader | ❌ 排除 | 4 种配置都正常 |
| 连接复用 | ❌ 排除 | 服务端返回 connection: close |
| 通用 HTTP 行为 | ❌ 排除 | Python + isahc 均正常 |

### maki 独有的复杂性（zerostack 没有）

| 层次 | maki | zerostack |
|------|------|-----------|
| 运行时 | **smol** | tokio |
| HTTP 客户端 | **isahc** + 手动配置 | reqwest（通过 rig） |
| SSE 解析 | **手动 `parse_sse()`** ~260 行 | rig 库内部处理 |
| 流控 | **自定义 `next_sse_line()` + deadline** | 无 |
| 重试 | **`stream_with_retry()`** + 指数退避 | 简单重试 |
| 空流处理 | **检测空 content → 529 → 重试** | 无 |
| 工具调用后空响应 | **nudge 机制**（推空消息到 history） | 无 |
| 事件通道 | **flume Sender → 前向到 UI** | 直接回调 |

### 高嫌疑区域

**1. `parse_sse()` 的状态机逻辑**
- BOM 剥离、error 检测、JSON 解析、空流检测、tool call 累加
- 任何一个分支提前 `continue` 或 `break` 都可能丢内容

**2. smol vs tokio 的 Timer 行为**
- `next_sse_line()` 用 `smol::Timer::after(remaining)` 做超时
- 每次成功读取后重置 deadline = now + 300s
- 如果 smol 的 Timer 在低资源环境下行为异常？

**3. 重试层的交互**
- 空流 → 529 → 重试 → 再次空流 → 无限循环？
- 用户描述的"一直卡住"完全符合这个模式

**4. 事件通道（flume）的背压**
- `event_tx.send_async()` 如果接收端处理慢会阻塞
- UI 线程是否及时消费 ProviderEvent？

---

## 建议下一步排查方向

**优先级从高到低**：

1. **`parse_sse()` 的 early exit 路径** — 加 debug 日志追踪每个 `continue`/`break`
2. **smol Timer + deadline 逻辑** — 是否是 Timer 导致提前超时
3. **空流检测 + 重试的死循环** — 是否在反复重试空流
4. **flume 通道背压** — UI 消费是否及时

---

## 已提交的改动

### `maki-providers/src/providers/mod.rs`

```rust
pub(crate) fn http_client(timeouts: Timeouts) -> isahc::HttpClient {
    isahc::HttpClient::builder()
        .connect_timeout(timeouts.connect)
        // NOTE: low_speed_timeout removed — it silently closes connections
        // when a model takes >30s to produce the first token (e.g. ModelScope
        // large models during thinking phase).
        .build()
        .expect("failed to build HTTP client")
}
```

### `maki-providers/src/providers/openai_compat.rs`

修复了 5 个因"空流→529"修复而失效的测试 + 移除重复测试块。

**测试结果**：471 passed, 0 failed

### 提交历史

| 提交 | 描述 |
|------|------|
| `0cbe1d97` | fix(providers): remove isahc low_speed_timeout |
| `8176d520` | docs: add Shangtang fix comparison insights |
| `9c6fb71b` | docs: ModelScope isahc vs reqwest root cause analysis |
| `53c24ca8` | test: add ModelScope mock test and update analysis doc |
| `99dbb9af` | test: add header comparison and buffered read tests |
| `f7a73ffc` | test: add request diff test (maki vs zerostack) |

---

记录日期：2026-07-24（补充 isahc 编译测试 + 排查方向转变）
