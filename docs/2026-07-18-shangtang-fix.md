# 2026-07-18 商汤 API 兼容性修复记录

## 背景

商汤 API 平台的部分模型在 SSE 流式响应中，tool call 字段的位置与标准 OpenAI Chat Completions 格式存在偏差。Maki 的 SS e 解析器 `openai_compat.rs` 未能正确解析，导致两个问题：

1. 工具调用失败，报 `maki_unknown_tool`（工具名称为空）
2. UI 中 bash 命令持续显示"⠹"运行中状态

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
// ToolCallDelta 新增顶层 name 字段
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

- **提交**: `3c33d9f`
- **改动**: `maki-providers/src/providers/openai_compat.rs` +7/-1 行

### 问题

UI 通过 tool_call `id` 来跟踪工具状态：
- `ToolUseStart(id, name)` → 标记为"运行中"
- `ToolDone(id)` → 标记为"已完成"，移除 spinner

商汤 API 分批发 name 和 id：
1. Delta 1: `{"name":"bash"}` → ToolUseStart(id="", name="bash") → UI 用空 id 跟踪
2. Delta 2: `{"id":"call_xxx"}` → 不发 ToolUseStart（name 已非空）→ UI 没更新 id
3. 工具完成 → ToolDone(id="call_xxx") → 不匹配 id="" → spinner 永远转

### 修复

`ToolUseStart` 的触发条件从仅 name 变化扩展为 name 或 id 任一首次出现：

```rust
// 原来
if was_unnamed && !acc.name.is_empty() { ... }

// 改为
let is_named = !acc.name.is_empty();
let has_id = !acc.id.is_empty();
if (was_unnamed && is_named) || (was_idless && has_id && is_named) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),   // ← 这次可能是真实 id
        name: acc.name.clone(),
    }).await?;
}
```

修改后 SSE 流式时序：
1. Delta 1: `{"name":"bash"}` → ToolUseStart(id="", name="bash") ✅ UI 开始跟踪
2. Delta 2: `{"id":"call_xxx"}` → ToolUseStart(id="call_xxx", name="bash") ✅ UI 更新 id
3. 工具完成 → ToolDone(id="call_xxx") ✅ 匹配，UI 停止 spinner

---

## 文件变更汇总

| 文件 | 改动 |
|------|------|
| `maki-providers/src/providers/openai_compat.rs` | +28/-4 行（3 次提交累计） |

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
if acc.name.is_empty() && let Some(name) = tc.name.as_ref() {
    acc.name = name.clone();
}
let is_named = !acc.name.is_empty();
let has_id = !acc.id.is_empty();
if (was_unnamed && is_named) || (was_idless && has_id && is_named) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),
        name: acc.name.clone(),
    }).await?;
}
```
