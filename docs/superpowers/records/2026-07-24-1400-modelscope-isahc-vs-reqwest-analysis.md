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
