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

商汤 API 在首帧之后，后续每一帧都发送 `function.name=""`（空字符串），导致已解析的正确名称被覆盖。

时序：
1. Delta 1: `{"function":{"name":"bash","arguments":""}}` → acc.name = "bash"
2. Delta 2: `{"function":{"name":"","arguments":"{\"command\":"}}` → acc.name = "" (覆盖!)

### 修复

```rust
// 原来：无条件覆盖
if let Some(name) = func.name {
    acc.name = name;
}

// 改为：仅当新名称非空或旧名称为空时覆盖
if let Some(name) = func.name.as_ref() {
    if !name.is_empty() || acc.name.is_empty() {
        acc.name = name.clone();
    }
}
```

同时添加 SSE 原始数据日志（`data.contains("tool_call")` 时 debug 打印），便于诊断。

---

## 修复 3：等待 id 和 name 就绪才通知 UI

- **提交 1**: `3c33d9f` — 第一次尝试（有 bug）
- **提交 2**: `a384a2d` — 最终修复

### 问题

`ToolUseStart` 可能在 id 为空时发送，UI 用空 id 创建 pending 条目。后续 `ToolDone` 带真实 id 无法匹配，条目永远旋转。

### 修复

```rust
// 只在 id 和 name 都非空时才通知 UI
if !acc.id.is_empty() && !acc.name.is_empty() && (was_idless || was_unnamed) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),
        name: acc.name.clone(),
    }).await?;
}
```

---

## 修复 4：UI 层去重

- **提交**: `aab2c9d`

### 问题

`ToolPending`（SSE 前向器）和 `ToolStart`（tool dispatch）来自不同 async 任务，到达 UI 的顺序不确定。当 `ToolStart` 先到时创建条目，随后 `ToolPending` 又创建一条重复条目，`ToolDone` 通过 `rfind` 只解决后一条。

### 修复

```rust
pub fn tool_pending(&mut self, id: String, name: &str) {
    if self.messages.iter().any(|m| matches!(&m.role, DisplayRole::Tool(t) if t.id == id)) {
        return;  // 跳过重复
    }
    // ... 创建新条目
}
```

---

## 修复 5（真正的根因）：防止 id 被空字符串覆盖

- **提交**: `e7a1278`
- **改动**: `maki-providers/src/providers/openai_compat.rs` +1/-1 行
- **测试**: `sse_shangtang_subsequent_empty_id_and_name`

### 问题

**这是整个问题链的最终根因。** 商汤 API 的 SSE 行为：

- **首帧**：给出正确的 `id` 和 `name`
  ```json
  {"id":"call_abc","type":"function","function":{"name":"bash","arguments":""}}
  ```
- **后续每一帧**：都发 `id=""` 和 `name=""`（重置）
  ```json
  {"id":"","type":"","function":{"name":"","arguments":"{\"command\":"}}
  ```

修复 2 已经保护了 `name` 不被覆盖，但 `id` 没有保护——第 2 帧的 `id=""` 把首帧的正确 id 覆盖了。

### 连锁反应

1. 首帧 → `acc.id = "call_abc"` → `ToolUseStart(id="call_abc")` 发送 ✅
2. 第 2 帧 → `acc.id = ""`（被覆盖）❌
3. 流结束 → `ContentBlock::ToolUse.id = "maki_unnamed_0"`（占位符）
4. `ToolStart(id="maki_unnamed_0")` → 创建新条目
5. `ToolDone(id="maki_unnamed_0")` → 解决新条目
6. **旧条目 `call_abc` 永远旋转** ⠹

### 修复

```diff
-                if let Some(id) = tc.id {
-                    acc.id = id;
-                }
+                if let Some(id) = tc.id {
+                    // Same guard as name
+                    if !id.is_empty() || acc.id.is_empty() {
+                        acc.id = id;
+                    }
+                }
```

---

## 文件变更汇总

| 文件 | 改动 |
|------|------|
| `maki-providers/src/providers/openai_compat.rs` | 约 +60/-8 行（6 次提交，含 6 个新测试） |
| `maki-ui/src/components/messages/mod.rs` | +12 行（去重 + 调试日志） |
| `docs/2026-07-18-shangtang-fix.md` | 本文件 |

### 最终代码（关键部分：`parse_sse` 中的 tool call 累加器逻辑）

```rust
let acc = &mut tool_accumulators[tc.index];
let was_unnamed = acc.name.is_empty();
let was_idless = acc.id.is_empty();

// 保护 id：后续 delta 的 id="" 不能覆盖正确的 id
if let Some(id) = tc.id {
    if !id.is_empty() || acc.id.is_empty() {
        acc.id = id;
    }
}
// 保护 name：后续 delta 的 name="" 不能覆盖正确的 name
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
// Fallback：部分 API 把 name 放 tool_calls[i].name 顶层
if acc.name.is_empty() && let Some(name) = tc.name.as_ref() {
    acc.name = name.clone();
}
// 只在 id 和 name 都已知时才通知 UI
if !acc.id.is_empty() && !acc.name.is_empty() && (was_idless || was_unnamed) {
    event_tx.send_async(ProviderEvent::ToolUseStart {
        id: acc.id.clone(),
        name: acc.name.clone(),
    }).await?;
}
```
