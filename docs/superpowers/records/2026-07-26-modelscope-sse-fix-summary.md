# ModelScope SSE 兼容性修复记录

## 问题描述

ModelScope 魔搭 API (`api-inference.modelscope.cn/v1`) 的 SSE 流式响应中，每个 chunk 的 `choices[]` 内同时包含 `delta` 和 `message` 两个对象，而 maki 的 `ChunkChoice` 结构体用 `#[serde(alias = "message")]` 将两者映射到同一个字段，导致 `message`（空值）覆盖 `delta`（有值），`reasoning_content` 丢失，触发空流检测。

## 根因

### 响应格式

ModelScope 返回的 SSE chunk 格式（非标准）：

```json
{
  "choices": [{
    "delta":    {"role":"assistant","content":"","reasoning_content":"User"},
    "finish_reason": null,
    "message":  {"role":null,"content":"","reasoning_content":""}
  }]
}
```

`delta` 有 `reasoning_content`，但 `message` 全部为空。

### 错误代码

```rust
// 修复前：serde alias 映射到同一字段，message 覆盖 delta
struct ChunkChoice {
    #[serde(alias = "message")]  // ← 问题在这儿
    delta: Option<ChunkDelta>,
    finish_reason: Option<String>,
}
```

### 调用链

```
ModelScope SSE chunk → serde 反序列化 → ChunkChoice
  → delta 字段: {"reasoning_content":"User"} ← 被覆盖
  → message 字段: {"reasoning_content":""}    ← 覆盖 delta
  → reasoning_text 为空 → reasoning_len=0
  → 空流检测 → 502 → retry 循环 → 用户看到"卡住无响应"
```

## 修复

### 修复后代码

```rust
struct ChunkChoice {
    delta: Option<ChunkDelta>,
    #[serde(default)]
    message: Option<ChunkDelta>,  // 独立字段，不覆盖 delta
    finish_reason: Option<String>,
}

// 解析时优先使用 delta，fallback 到 message
let Some(delta) = choice.delta.or(choice.message) else { continue; };
```

### 涉及文件

| 文件 | 改动 |
|------|------|
| `maki-providers/src/providers/openai_compat.rs` | `ChunkChoice` 分离 delta 和 message 字段 + 解析优先使用 delta |

## 验证

```bash
# 修复前
$ ./maki -m cust07-mscope/Qwen/Qwen3.5-35B-A3B --print "hi"
server error (502)  # reasoning_len=0, 空流检测触发

# 修复后
$ ./maki -m cust07-mscope/Qwen/Qwen3.5-35B-A3B --print "hi"
Hi! I'm here to help with your Rust project. What would you like to work on?
```

## 排查过程（精简）

| 步骤 | 方法 | 结论 |
|------|------|------|
| 1 | 逐层 `eprintln!` 定位挂起点 | 挂起在 `stream_with_retry` 重试循环 |
| 2 | 添加 `[RESPONSE] raw body` 日志 | ModelScope 返回 200 + SSE 数据，但 `reasoning_len=0` |
| 3 | Python 对比测试 | 请求体结构、字段名、大小均非根因 |
| 4 | curl 测试 | libcurl 本身正常 |
| 5 | isahc-sse-test 独立测试 | isahc + BufReader 均正常 |
| 6 | 检查 `ChunkChoice` 反序列化 | `#[serde(alias = "message")]` 导致 `message` 覆盖 `delta` |

## 相关提交

| 提交 | 分支 | 描述 |
|------|------|------|
| `25449798` | feat/ai-0723 | fix: separate delta and message fields in ChunkChoice |
| `4aca0312` | feat/ai-0726 | fix: apply ModelScope compatibility fixes |

---

记录日期：2026-07-26