# AxonHub 代理模式下 thinking 内容丢失分析

## 问题描述

- 渠道：AxonHub（自建代理，`mHub` 动态 provider）
- 模型：`mHub/st/deepseek-v4-flash` 等所有通过 AxonHub 代理的模型
- 症状：TUI 中 thinking 内容不显示，其他输入/响应正常
- 对比：直接连商汤原渠道 `st/deepseek-v4-flash` → thinking 正常
- 对比：ZeroStack 接 AxonHub 同模型 → thinking 正常

## 确认排除项

- AxonHub 自身配置正常（页面验证通过，ZeroStack 接也正常）

## 架构路径

```
mHub (动态 provider, base="openai")
  → ManifestRegistry::for_slug("mHub") → dynamic::base_for_slug("mHub") → ProviderKind::OpenAi
  → dynamic::create() → OpenAi::with_auth()
  → OpenAi::stream_message()
    → OpenAiCompatProvider::do_stream()  [openai_compat.rs:164]
      → parse_sse()                      [openai_compat.rs:480]
```

mHub 动态 provider 脚本信息：

```bash
# ~/.config/maki/providers/mHub
info → {"display_name": "mHub", "base": "openai", "has_auth": true}
resolve → {"base_url": "http://sam-dev.local:8090/v1", "headers": {"Authorization": "Bearer $AXONHUB_API_KEY"}}
```

## 根因分析

### SSE 解析关键代码

`maki-providers/src/providers/openai_compat.rs:546`

```rust
let Some(delta) = choice.delta.or(choice.message) else {
    continue;
};
```

`ChunkChoice` 结构体（第 441-447 行）：

```rust
struct ChunkChoice {
    delta: Option<ChunkDelta>,
    #[serde(default)]
    message: Option<ChunkDelta>,
    finish_reason: Option<String>,
}
```

`ChunkDelta` 结构体（第 406-412 行）：

```rust
struct ChunkDelta {
    content: Option<ContentDelta>,
    #[serde(alias = "reasoning")]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}
```

Thinking 解析位置（第 550-556 行）：

```rust
if let Some(reasoning) = delta.reasoning_content
    && !reasoning.is_empty()
{
    reasoning_text.push_str(&reasoning);
    event_tx
        .send_async(ProviderEvent::ThinkingDelta { text: reasoning })
        .await?;
}
```

### 直接原因

`choice.delta.or(choice.message)` 是**二选一**逻辑。当 AxonHub 返回的 SSE 块中 `delta` 和 `message` 同时存在且信息分散时：

```json
// AxonHub 代理后的 SSE 格式（推测）
{
  "choices": [{
    "delta": {"content": "Hello"},
    "message": {"reasoning_content": "思考过程..."}
  }]
}
```

- `delta` 有值（`content: "Hello"`）→ `or` 短路，直接返回 `delta`
- `message` 中的 `reasoning_content` 被丢弃
- 解析器认为 `delta.reasoning_content` 为 None → 不发 `ThinkingDelta` 事件 → TUI 不显示

### 为什么直接连商汤正常

商汤原渠道把 `reasoning_content` 直接放在 `delta` 中：

```json
{
  "choices": [{
    "delta": {"reasoning_content": "思考过程...", "content": "Hello"}
  }]
}
```

`delta` 单独即可提供所有字段，无需 `message` 补全。

### 为什么 ZeroStack 正常

ZeroStack 使用 `rig` 库的 SSE 解析器，rig 内部做字段合并（而非 `or` 取舍），能正确合并 `delta` 和 `message` 中的分散信息。

## 修复方案

### 文件

`maki-providers/src/providers/openai_compat.rs`

### 修改

将 `delta.or(message)` 二选一逻辑替换为 `merge_delta()` 合并逻辑：

```rust
/// Merge `delta` and `message` fields from the same SSE chunk, preferring
/// `delta` values but filling in missing fields from `message`. Some proxies
/// (AxonHub) split `reasoning_content` and `content` across the two fields.
fn merge_delta(delta: Option<ChunkDelta>, message: Option<ChunkDelta>) -> Option<ChunkDelta> {
    match (delta, message) {
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(mut d), Some(m)) => {
            if d.content.is_none() {
                d.content = m.content;
            }
            if d.reasoning_content.is_none() {
                d.reasoning_content = m.reasoning_content;
            }
            if d.tool_calls.is_none() {
                d.tool_calls = m.tool_calls;
            }
            Some(d)
        }
        (None, None) => None,
    }
}
```

调用处替换为：

```rust
let Some(delta) = merge_delta(choice.delta, choice.message) else {
    continue;
};
```

### 影响范围

- **原渠道（商汤、OpenAI、DeepSeek 等）**：只发 `delta`，`message` 为 None，走 `(Some(d), None)` 分支，行为不变，零影响。
- **AxonHub 代理渠道**：`delta` 和 `message` 同时存在时，从 `message` 补全 `delta` 缺失的字段（`reasoning_content`、`content`、`tool_calls`），优先保留 `delta` 已有值。
- **其他可能拆分字段的代理**：同样受益，兼容性提升。

## 相关历史修复

| 提交 | 说明 |
|------|------|
| `25449798` | 首次分离 `delta`/`message` 字段，避免 `serde(alias)` 冲突 |
| `89d48e0f` | 应用 ModelScope 兼容性修复 |
| 当前 | 从二选一改为合并，适配 AxonHub 等代理转发场景 |

## 补充验证（2026-08-05 编译调试）

### Debug 日志关键发现

编译后通过 `RUST_LOG=maki_providers=debug` + `--output-format stream-json` 对比测试：

**商汤直连 (`cust05-st-sense/deepseek-v4-flash`) stream-json 输出：**
```json
{"content":[
  {"type":"thinking","thinking":"The user just said hi. Let me be concise..."},
  {"type":"text","text":"Hi. How can I help you today?"}
]}
```

**AxonHub (`mHub/st/deepseek-v4-flash`) stream-json 输出：**
```json
{"content":[
  {"type":"text","text":"Hi! I'm Maki, the coding agent..."}
]}
```
→ 没有 thinking block！

### 实际 SSE 数据对比

通过日志中 `"SSE thinking chunk"` 的输出确认，**两个渠道返回的 SSE 格式完全一致**，`reasoning_content` 都在 `delta` 字段中，且**都没有 `message` 字段**：

```json
// 商汤直连
{"delta":{"role":"assistant","reasoning_content":"The"}}
{"delta":{"reasoning_content":" user is"}}

// AxonHub（同样格式）
{"delta":{"role":"assistant","reasoning_content":"The"}}
{"delta":{"reasoning_content":" user is"}}
```

因此 `merge_delta()` 修复对于此场景不生效（SSE 解析本身没问题）。

### 请求体关键发现

通过日志中 `"OpenAi stream request body"` 确认，**请求体中没有 `thinking` 或 `reasoning_effort` 字段**：

```json
{
  "model": "deepseek-v4-flash",
  "messages": [...],
  "stream": true,
  "max_completion_tokens": 16384,
  "stream_options": {"include_usage": true}
  // ← 没有 thinking 或 reasoning_effort！
}
```

原因：`--print` 模式使用 `thinking: Default::default()` = `ThinkingConfig::Off`，`opts.thinking.is_enabled()` 为 false，`apply_reasoning_effort()` 不生效。

### 根本原因修正

| 路径 | 请求体 thinking 信号 | API 默认行为 | 结果 |
|------|---------------------|-------------|------|
| 商汤直连 | 无（`thinking: Off`） | 默认开启 thinking | ✅ 有 |
| AxonHub | 无（`thinking: Off`） | 不默认开启 | ❌ 无 |
| ZeroStack + AxonHub | 有（rig 库实现） | — | ✅ 有 |

**商汤 API 对 DeepSeek 模型默认开启 thinking**，所以即使请求体没有 `thinking`/`reasoning_effort`，响应也会包含 `reasoning_content`。**AxonHub 代理不会默认开启 thinking**，需要请求体明确告知。

### 修正后的修复方案

#### 文件

`maki-providers/src/providers/openai/platform.rs`

#### 修改

在 `OpenAi::stream_message()` 中，当 `opts.thinking.is_enabled()` 为 true 时，添加 `thinking: {"type": "enabled"}` 到请求体——与 `DeepSeek` provider 做法一致：

```rust
// openai/platform.rs:220-222
if opts.thinking.is_enabled() {
    body["thinking"] = json!({"type": "enabled"});
}
opts.thinking
    .apply_reasoning_effort(&mut body, &dialect::STANDARD, model);
```

#### 效果

TUI 中用户开启 thinking 后（`opts.thinking` 变为 `Adaptive`/`Effort`/`Budget`），请求体变为：

```json
{
  "thinking": {"type": "enabled"},
  "reasoning_effort": "medium"
}
```

- **AxonHub**：识别 `thinking` 字段 → 启用 thinking ✅
- **商汤直连**：双向兼容 ✅
- **OpenAI 官方**：忽略未知字段 `thinking`，使用 `reasoning_effort` ✅

#### 保留的 `merge_delta()` 修复

`openai_compat.rs` 中的 `merge_delta()` 合并函数作为防御性措施保留，未来若有其他代理拆分 `delta`/`message` 字段时可自动兼容。

## 相关提交

| 提交 | 说明 |
|------|------|
| `25449798` | 首次分离 `delta`/`message` 字段，避免 `serde(alias)` 冲突 |
| `89d48e0f` | 应用 ModelScope 兼容性修复 |
| `6dd146f6` | `fix(openai_compat): merge delta and message` — 防御性合并 |
| `0351d3f6` | `fix(openai): add thinking: {type: enabled} to request body` — **核心修复** |

## 验证方法

1. TUI 中开启 thinking（`thinking = "adaptive"` 或热键切换）
2. 使用 `mHub/st/deepseek-v4-flash` 模型
3. 发送消息，观察 thinking 内容是否显示
4. 日志确认：`grep "request_body" ~/.local/logs/maki/maki.log | grep -o '"thinking"'`
5. 回归测试：`maki -m cust05-st-sense/deepseek-v4-flash --print "hi"`

---

## 最终根因确认（2026-08-06_07-42-37）

### 调试过程

通过 `RUST_LOG=maki_providers=debug` 日志 + `--output-format stream-json` 逐步排查：

| 步骤 | 发现 | 结论 |
|------|------|------|
| 1. 对比 SSE 响应 | 两渠道 `reasoning_content` 都在 `delta` 中，**无 `message` 字段** | `merge_delta()` 不生效 |
| 2. 检查请求体 | 请求体无 `thinking`/`reasoning_effort` | `thinking` 默认 `Off` |
| 3. 在 headless 启用 `thinking: Adaptive` | 请求体多了 `thinking: {type: enabled}`，但响应仍无 thinking | 不是请求体问题 |
| 4. 捕获解析失败日志 | `"duplicate field reasoning_content"` | 某字段重复导致 chunk 被跳过 |
| 5. 查看原始 SSE | `{"delta":{"reasoning_content":"The","reasoning":"The"}}` | **AxonHub 同时发送 `reasoning_content` 和 `reasoning`** |

### 最终根因

`ChunkDelta` 结构体上有 `#[serde(alias = "reasoning")]`：

```rust
struct ChunkDelta {
    content: Option<ContentDelta>,
    #[serde(alias = "reasoning")]    // ← 问题所在
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}
```

`#[serde(alias = "reasoning")]` 让 serde 将 `reasoning` 视为 `reasoning_content` 的别名。当 AxonHub 返回的 SSE 中同时出现两个字段时：

```json
{"delta":{"reasoning_content":"The","reasoning":"The"}}
```

serde 认为重复字段 → 解析失败 → `warn!("failed to parse SSE chunk")` → `continue` → 整个 chunk 被跳过 → reasoning_text 为空 → 最终 message 无 `ContentBlock::Thinking`。

### 修复

**文件**：`maki-providers/src/providers/openai_compat.rs`

**改动**：删除 `#[serde(alias = "reasoning")]`，serde 忽略 `reasoning` 字段，只解析 `reasoning_content`。

```rust
// 修改前
#[serde(alias = "reasoning")]
reasoning_content: Option<String>,

// 修改后
reasoning_content: Option<String>,
```

**影响**：`reasoning` 作为字段名在主流 API 中极少使用（已知 none），删除别名不影响现有渠道。若未来有 API 只用 `reasoning` 而非 `reasoning_content`，可单独处理。

### 最终验证

```bash
$ maki -m mHub/st/deepseek-v4-flash --print --output-format stream-json "hi"
{"content":[
  {"type":"thinking","thinking":"The user is saying hello. I'll respond concisely."},
  {"type":"text","text":"Hi! I'm Maki, a CLI coding agent. How can I help you today?"}
]}
```

✅ thinking 内容正常显示，`"duplicate field"` 错误消失。

## 最终提交记录

| 提交 | 说明 | 有效性 |
|------|------|--------|
| `6dd146f6` | `merge_delta` 防御性合并 | 保留，备而不用 |
| `0351d3f6` | 请求体加 `thinking: {type: enabled}` | 仅 TUI 开启 thinking 时生效 |
| `6371213e` | **删除 `#[serde(alias = "reasoning")]`** | ✅ **核心修复，已验证** |

---

## 兼容性分析（2026-08-06_07-58-08）

### 别名来源

`#[serde(alias = "reasoning")]` 在提交 `8767fb0c` (`fix(providers): alias reasoning field for vLLM compat`) 中为 **vLLM 兼容**而添加。vLLM 使用 `reasoning` 字段名（而非 `reasoning_content`）返回 thinking 内容。

### 删除别名的影响面

所有走 `openai_compat::parse_sse` 的 provider 均受影响：

OpenAI, DeepSeek, Mistral, OpenRouter, Copilot, TensorX, Z.AI, Local, Custom, Synthetic, Catalog, OpenCode, 以及所有 `base="openai"` 的动态 provider（含 mHub/AxonHub）。

| 场景 | 当前（已删除别名） | 影响 |
|------|-------------------|------|
| 标准 API（只发 `reasoning_content`） | 正常 | 无影响 ✅ |
| **AxonHub**（同时发 `reasoning_content` + `reasoning`） | 正常 | 修复完成 ✅ |
| **vLLM**（只发 `reasoning`） | **thinking 丢失** | ❌ 回归 |

### 推荐方案：自定义 Deserialize

用自定义 `Deserialize` 实现，将 `reasoning_content` 和 `reasoning` 作为独立字段解析，`reasoning_content.or(reasoning)` 优先取前者：

```rust
impl<'de> Deserialize<'de> for ChunkDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            content: Option<ContentDelta>,
            reasoning_content: Option<String>,
            reasoning: Option<String>,  // vLLM 兼容
            tool_calls: Option<Vec<ToolCallDelta>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(ChunkDelta {
            content: raw.content,
            reasoning_content: raw.reasoning_content.or(raw.reasoning),
            tool_calls: raw.tool_calls,
        })
    }
}
```

三种场景全覆盖：

| 场景 | JSON 字段 | `reasoning_content` | `reasoning` | 结果 |
|------|-----------|---------------------|-------------|------|
| 标准 API | `{"reasoning_content":"..."}` | `Some(...)` | `None` | 取 `reasoning_content` ✅ |
| vLLM | `{"reasoning":"..."}` | `None` | `Some(...)` | `or` 回退到 `reasoning` ✅ |
| AxonHub | `{"reasoning_content":"...","reasoning":"..."}` | `Some(...)` | `Some(...)` | `or` 短路，取 `reasoning_content` ✅ |

**优势**：无 `#[serde(alias)]`，serde 不会将两个字段视为同一字段，彻底避免 duplicate field 冲突。