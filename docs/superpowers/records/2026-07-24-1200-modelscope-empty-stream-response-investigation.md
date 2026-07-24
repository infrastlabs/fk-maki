# ModelScope (魔搭) API 流式响应为空问题排查记录

## 背景

ModelScope 魔搭 API (`api-inference.modelscope.cn/v1`) 通过动态提供者脚本 `cust07-mscope` 集成到 maki 中。声明 `"base": "openai"`，使用 `OpenAiCompatProvider` 发送 SSE 流式请求。

## 问题描述

- 调用魔搭上大多数模型（如 `Qwen/Qwen3.5-35B-A3B`）时，HTTP 请求正常完成（200 OK），但无返回内容
- 官方 Python SDK / curl 测试能正常返回
- maki v0.3.27 和 v0.4.2 均受影响
- 当前代码基于官方 main 最新分支

## 请求体结构

动态提供者走 `OpenAi::with_auth()`，使用 OpenAI 静态配置（`openai/platform.rs:17-24`）：

```json
{
  "model": "Qwen/Qwen3.5-35B-A3B",
  "messages": [...],
  "stream": true,
  "max_completion_tokens": 16384,
  "stream_options": {"include_usage": true}
}
```

## 排查过程

### 架构路径

`DynamicProvider`(`cust07-mscope`, base=openai) →
`OpenAi::with_auth(auth, timeouts)` →
`OpenAiCompatProvider::new(&CONFIG, timeouts)` →
`do_stream()` POST `/chat/completions` →
`parse_sse()` 解析 SSE 流

**相关文件：**

- `maki-providers/src/providers/dynamic.rs` — 动态提供者框架
- `maki-providers/src/providers/openai/platform.rs` — `OpenAi` struct（使用 `OpenAiCompatConfig` 静态配置）
- `maki-providers/src/providers/openai_compat.rs` — 核心 SSE 解析逻辑 `parse_sse()`
- `maki-providers/src/providers/mod.rs` — `next_sse_line()`, `low_speed_timeout`
- `_ext/home/headless/.config/maki/providers/cust07-mscope` — 魔搭动态提供者脚本

### SSE 解析逻辑 (`openai_compat.rs:461-703`)

```rust
// 只处理 data: 开头的行
let data = match line.strip_prefix("data:") {
    Some(d) => d.trim(),
    None => continue,
};

// [DONE] 检测
if data == STREAM_DONE { break; }

// JSON 错误检测
if data.contains("\"error\"") && let Ok(ev) = serde_json::from_str::<SseErrorPayload>(data) {
    return Err(ev.into_agent_error());
}

// 解析 SseChunk
let chunk: SseChunk = match serde_json::from_str(data) { ... };

// 跳过空 choices
let Some(choice) = chunk.choices.into_iter().next() else { continue; };

// 捕获 finish_reason
if let Some(reason) = choice.finish_reason { stop_reason = ...; }

// 跳过无 delta 的 chunk
let Some(delta) = choice.delta else { continue; };

// 处理 reasoning_content（思考）
if let Some(reasoning) = delta.reasoning_content && !reasoning.is_empty() {
    reasoning_text.push_str(&reasoning);
    // 发送 ThinkingDelta 事件
}

// 处理 content（文本）
match delta.content {
    Some(ContentDelta::String(content_str)) if !content_str.is_empty() => {
        text.push_str(&content);
        // 发送 TextDelta 事件
    }
    Some(ContentDelta::Array(content_array)) => { ... }
    _ => {} // content 为 None 或空字符串时静默跳过
}
```

### 已知的 SSE 数据流

从魔搭/百炼 API 文档得到的 Qwen 模型流式输出格式：

1. **思考阶段**：`{"delta":{"reasoning_content":"...","content":null}}`
2. **回答阶段**：`{"delta":{"reasoning_content":null,"content":"..."}}`
3. **结束阶段**：`{"delta":{},"finish_reason":"stop"}`
4. **用量 chunk**：`{"choices":[],"usage":{...}}` （当 `stream_options.include_usage=true`）
5. **结束信号**：`data: [DONE]`

## 可能的根因

### 1. `max_completion_tokens` 字段名不兼容（高概率）

**问题：** `OpenAi` 配置 (`openai/platform.rs:21`) 使用 `"max_completion_tokens"`（OpenAI o-series 字段），但魔搭期待标准的 `"max_tokens"`。

**影响：** 魔搭不识别 `max_completion_tokens` 时可能：
- 忽略该字段，使用默认值（可能很低）
- 静默截断或返回空

**修复方向：** 为动态提供者创建独立的 `OpenAiCompatConfig` 实例或用 `"max_tokens"`。

### 2. `low_speed_timeout`（30s）在模型长思考时触发

**问题：** `LOW_SPEED_BYTES_PER_SEC = 1`（`mod.rs:32`），`low_speed` 默认超时 30 秒。当 Qwen 模型在输出第一个 token 前思考超过 30 秒时，isahc 静默关闭连接。

**影响：** 连接被关闭但不一定是显式超时错误（可能是 I/O error），给用户感觉像是"请求完成但无返回"。

**相关代码：** `maki-providers/src/providers/mod.rs:32,166-172`

### 3. DashScope/ModelScope 服务端偶发空流问题

**外部证据：** 从多个开源项目的 issue 确认 DashScope/ModelScope 有已知的服务端空响应问题：
- `finish_reason: "stop"` 但 `content` 和 `reasoning_content` 均为空
- 流在推理阶段中断（只有 `reasoning_content` chunks，无 `content` chunks）
- 返回 `choices: []` + 仅 usage 元数据

### 4. Qwen 模型停在思考阶段不输出实际内容

如果模型始终只输出 `reasoning_content` 而不输出 `content`，maki 会捕获思考内容（`ThinkingDelta`/`Thinking` block），但若 UI 不展示 thinking 块则用户看不到任何文本回答。

## 排查建议

### 用 curl 直接测试 API 返回的 SSE 原始内容

```bash
curl -s -N -X POST "https://api-inference.modelscope.cn/v1/chat/completions" \
  -H "Authorization: Bearer $MSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Qwen/Qwen3.5-35B-A3B",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true,
    "max_tokens": 16384
  }'
```

关键观察点：
1. 是否每行有 `data:` 前缀
2. `content` 字段是否存在且非空
3. 结束信号是 `[DONE]` 还是其他格式
4. 是否长时间无响应后才开始输出

### 对比测试：用 `max_tokens` 替换 `max_completion_tokens`

```bash
# 测试 max_tokens（标准字段）
curl -s -N -X POST "https://api-inference.modelscope.cn/v1/chat/completions" \
  -H "Authorization: Bearer $MSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Qwen/Qwen3.5-35B-A3B",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true,
    "max_tokens": 16384
  }' | head -20

# 对比测试：不加 stream_options
curl -s -N -X POST "https://api-inference.modelscope.cn/v1/chat/completions" \
  -H "Authorization: Bearer $MSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Qwen/Qwen3.5-35B-A3B",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true,
    "max_tokens": 16384
  }' | head -20
```

## 代码修复方向

### 方案 A：为动态提供者创建独立配置

在 `OpenAi::with_auth()` 中检测调用来源，若来自动态提供者则使用 `max_tokens` 字段。

### 方案 B：`build_body()` 同时发送两个字段

```rust
// 兼容性：非 OpenAI 平台只认 max_tokens
body["max_tokens"] = json!(max_output);
```

### 方案 C：增加低速度超时时间

为动态/自定义提供者增加更大的 `low_speed` 超时（如 120s）以应对长思考模型。

---

记录日期：2026-07-24
