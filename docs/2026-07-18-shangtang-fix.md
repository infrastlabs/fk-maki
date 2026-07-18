# 2026-07-18 商汤 API 兼容性修复记录

## 背景

商汤 API 平台的部分模型在 SSE 流式响应中，tool call 字段的位置与标准 OpenAI Chat Completions 格式存在偏差。Maki 的 SSE 解析器 `openai_compat.rs` 未能正确解析，导致两个问题：

1. 工具调用失败，报 `maki_unknown_tool`（工具名称为空）
2. UI 中 bash 命令持续显示"⠹/⠋/⠧"运行中状态

Zerostack (rig 框架) 的 SSE 解析器兼容性更宽松，未出现此问题。

---

## 修复 1：支持顶层 tool name

- **提交**: `649afcb`
- **改动**: `maki-providers/src/providers/openai_compat.rs` +8 行

### 问题

`ToolCallDelta` 结构体只定义了 `function.name` 作为工具名称来源：

```rust
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<FunctionDelta>,  // ← name 仅在此处
}
```

商汤 API 在 SSE delta 中把 `name` 放在 `tool_calls[i].name` 顶层而非 `tool_calls[i].function.name`，serde 静默丢弃。

### 修复

添加 `name: Option<String>` 字段到 `ToolCallDelta`，并在 `parse_sse` 中增加 fallback：

```rust
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    name: Option<String>,          // ← 新增
    function: Option<FunctionDelta>,
}

// parse_sse 中 fallback 逻辑
if acc.name.is_empty() && let Some(name) = tc.name.as_ref() {
    acc.name = name.clone();
}
```

---

## 修复 2：防止空字符串覆盖 & 添加 SSE 调试日志

- **提交**: `33f53d5`
- **改动**: `maki-providers/src/providers/openai_compat.rs` +13/-3 行

### 问题

商汤 API 可能先发顶层 `name`，后续 delta 又发了空的 `function.name = ""`，导致已解析的正确名称被覆盖。

时序：
1. Delta 1: `{"index":0,"name":"bash"}` → acc.name = "bash" (通过 fallback)
2. Delta 2: `{"index":0,"function":{"name":"","arguments":"{}"}}` → acc.name = "" (覆盖!)

### 修复 2.1：防空字符串覆盖

```rust
// 原来
if let Some(name) = func.name {
    acc.name = name;  // 无条件覆盖
}

// 改为
if let Some(name) = func.name.as_ref() {
    if !name.is_empty() || acc.name.is_empty() {
        acc.name = name.clone();  // 仅当新名称非空或旧名称为空时覆盖
    }
}
```

### 修复 2.2：SSE 原始数据日志

```rust
Ok(c) => {
    if data.contains("tool_call") {
        debug!(raw_sse = %data, "SSE tool_call chunk");
    }
    c
}
```

运行 `RUST_LOG=maki_providers=debug maki` 可查看原始 SSE 数据。

---

## 修复 3：id 分批发来时通知 UI

- **提交 1**: `3c33d9f` — 第一次尝试（有 bug）
- **提交 2**: `a384a2d` — 最终修复
- **改动**: `maki-providers/src/providers/openai_compat.rs` +5/-6 行

### 问题

UI 通过 tool_call `id` 来跟踪工具状态：
- `ToolUseStart(id, name)` → 标记为"运行中"
- `ToolDone(id)` → 标记为"已完成"，移除 spinner

商汤 API 分批发 name 和 id：
1. Delta: `{"name":"bash"}` + Delta: `{"id":"call_xxx","function":{"arguments":"{}"}}`

第一次修复（v3）在 name 出现时发一次 ToolUseStart(id="")，id 出现时再发一次
ToolUseStart(id="call_xxx")。UI 创建了两个 pending 条目，只有第二个被
ToolDone 清除，第一个空 id 条目永远旋转。

### 最终修复

**只在 id 和 name 都已知时才通知 UI**：

```rust
// 最终版本
if !acc.id.is_empty() && !acc.name.is_empty() && (was_idless || was_unnamed) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),
        name: acc.name.clone(),
    }).await?;
}
```

这样无论 id 和 name 以什么顺序到达，都只发一次 ToolUseStart，且 id 必非空。

---

## 文件变更汇总

| 文件 | 改动 |
|------|------|
| `maki-providers/src/providers/openai_compat.rs` | 约 +28/-6 行（4 次提交累计） |

### 最终代码（关键部分）

**结构体**（约 364-370 行）：
```rust
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    name: Option<String>,
    function: Option<FunctionDelta>,
}
```

**解析逻辑**（约 594-627 行）：
```rust
let acc = &mut tool_accumulators[tc.index];
let was_unnamed = acc.name.is_empty();
let was_idless = acc.id.is_empty();

if let Some(id) = tc.id {
    acc.id = id;
}
if let Some(func) = tc.function {
    if let Some(name) = func.name.as_ref() {
        if !name.is_empty() || acc.name.is_empty() {
            acc.name = name.clone();
        }
    }
    if let Some(args) = func.arguments {
        acc.arguments.push_str(&args);
    }
}
// Fallback: some APIs put name at top level (tool_calls[i].name)
if acc.name.is_empty() && let Some(name) = tc.name.as_ref() {
    acc.name = name.clone();
}
// Only notify UI when both id and name are known, otherwise the
// pending entry would have an empty id and never match ToolDone.
if !acc.id.is_empty() && !acc.name.is_empty() && (was_idless || was_unnamed) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),
        name: acc.name.clone(),
    }).await?;
}
```

### 文件位置

`maki-providers/src/providers/openai_compat.rs`
