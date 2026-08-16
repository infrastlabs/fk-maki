# TurnEnd 钩子机制与消息内容获取分析

> 更新时间：2026-08-17_06-27-08

## 问题原文（用户描述）

> maki 交互式模式，每次执行完一轮问答，如何让外部的程序知道它完成了，有 hook 机制吗，或是否可以通过 lua 脚本实现？

后续追问：

> TurnEnd 回执内容有些什么？比方这一轮的输入与输出内容

> 有其他的 lua 接口能取得输入输出吗？参考 https://maki.sh/docs/lua-api/

> 完整信息，新保存到记录文档，参考子目录下已有文档命名，日期加时间开头 如当前时间

## 代码理解

### Autocmd 事件系统

Maki 内置了类似 Neovim 的 autocmd 事件系统，定义在 `maki-lua/src/api/autocmd.rs`。

**内置事件列表**（`autocmd.rs:130-139`）：

| 事件 | 触发时机 | 携带数据 |
|------|---------|---------|
| `TurnStart` | 新回合开始 | `session_id` |
| `TurnEnd` | 回合完成 | `session_id` |
| `TurnError` | 回合出错 | `session_id`, `message` |
| `ToolStart` | 工具开始执行 | `session_id`, `tool_id`, `tool` |
| `ToolDone` | 工具执行完成 | `session_id`, `tool_id`, `tool` |
| `SessionReset` | 会话重置 | `session_id` |
| `SessionFocusChanged` | 焦点切换 | `session_id`, `previous_session_id` |
| `SessionStatusChanged` | 状态变化 | — |

### TurnEnd 触发链路

```
Agent 循环完成
  → AgentEvent::Done (maki-agent/src/agent/run.rs:406)
    → UI chat.rs:123 转为 ChatEventResult::Done
      → app/mod.rs:1130-1139 处理
        → fire_session_autocmd("TurnEnd", json!({}))
          → 注入 session_id (app/mod.rs:311-318)
            → Lua 运行时 dispatch (runtime.rs:2702)
              → 执行所有 create_autocmd("TurnEnd", ...) 回调
```

关键代码 `maki-ui/src/app/mod.rs:1136`：

```rust
self.fire_session_autocmd("TurnEnd", serde_json::json!({}));
```

`fire_session_autocmd` 只注入 `session_id`（`app/mod.rs:311-318`）：

```rust
fn fire_session_autocmd(&self, event: &str, mut data: serde_json::Value) {
    if let Some(map) = data.as_object_mut() {
        map.insert(
            "session_id".into(),
            serde_json::Value::String(self.state.session.id.to_string()),
        );
    }
    self.lua_event_handle.fire_autocmd(event, data);
}
```

### Lua 侧接收到的数据

```lua
maki.api.create_autocmd("TurnEnd", {
  callback = function(ev)
    -- ev.event      = "TurnEnd"
    -- ev.match      = nil
    -- ev.data       = { session_id = "..." }
    -- 没有输入/输出内容
  end
})
```

### 现有 `maki.session` Lua API

定义在 `maki-lua/src/api/session.rs`，通过 `SessionRequest` 枚举（`maki-lua/src/api/util/command.rs:396`）与 UI 层通信：

| 函数 | 返回内容 |
|------|---------|
| `maki.session.list()` | 持久化会话列表（元数据） |
| `maki.session.live()` | `{id, title, status, updated_at}` — 无消息内容 |
| `maki.session.current()` | 当前焦点会话 ID |
| `maki.session.focus(id)` | 切换焦点 |
| `maki.session.new(opts)` | 创建新会话 |
| `maki.session.prompt(text, opts)` | 向会话发消息 |
| `maki.session.set_title(opts)` | 设置标题 |
| `maki.session.notify(text, opts)` | 发送通知 |
| `maki.session.delete(id)` | 删除会话 |

**没有 `get_messages()`、`get_history()` 或类似接口。**

### 为什么拿不到消息内容

会话消息历史存储在 UI 层的 `Chat` 结构体（`maki-ui/src/chat.rs:43`）中，从未暴露到 Lua 层。`SessionRequest` 枚举也没有对应的获取消息变体。`SessionRequest::Live` 响应（`maki-ui/src/event_loop.rs:724`）只返回 `id, title, status, updated_at`。

### 内置插件示例

- `plugins/todo_write/init.lua:176` — 监听 `TurnEnd` 和 `SessionReset` 清除 todo 列表
- `plugins/sessions/init.lua:547` — 监听 `SessionStatusChanged` 闪烁提示

## 解决方案

### 方案 A：TurnEnd 事件携带消息内容

改 `maki-ui/src/app/mod.rs:1136`，从 `main_chat()` 取最后一条用户消息和助手回复：

```rust
// 在 TurnEnd 时携带输入输出
let chat = self.main_chat();
let last_user = chat.messages_panel.last_user_message();
let last_assistant = chat.messages_panel.last_assistant_message();
self.fire_session_autocmd("TurnEnd", serde_json::json!({
    "user_message": last_user,
    "assistant_message": last_assistant,
}));
```

### 方案 B：新增 `maki.session.get_messages(id)` Lua API

1. `SessionRequest` 加 `GetMessages { id: String }` 变体
2. UI 层 `event_loop.rs` 响应时从 `Chat` 取消息列表
3. Lua 侧 `maki.session.get_messages(id)` 函数

### 方案 C：ACP 协议

通过 `maki-acp`（ACP ndjson stdio 协议）运行 Maki，每个回合完成后通过 stdin/stdout 返回 `PromptResponse`，包含完整消息交互。这是目前唯一能拿到完整输入输出的方式，但需要外部程序通过 ACP 协议通信。

## 结论

TurnEnd 当前只传 `session_id`，没有输入输出内容。Lua API 也没有获取会话消息的接口。需要修改 Rust 代码（方案 A 或 B）才能从 Lua 侧拿到输入输出内容。

## 相关文件

| 文件 | 关键行 | 说明 |
|------|--------|------|
| `maki-ui/src/app/mod.rs` | 311-318, 1136 | TurnEnd 触发点 |
| `maki-lua/src/api/autocmd.rs` | 130-139 | 事件定义 |
| `maki-lua/src/api/session.rs` | 全文 | session Lua API |
| `maki-lua/src/api/util/command.rs` | 396-405 | SessionRequest 枚举 |
| `maki-ui/src/event_loop.rs` | 724-741 | Live/Current 响应 |
| `maki-lua/src/runtime.rs` | 94, 2702-2705 | TurnEnd 事件常量及 dispatch |
| `maki-ui/src/chat.rs` | 43 | Chat 结构体（消息存储） |
| `plugins/todo_write/init.lua` | 176 | TurnEnd 使用示例 |
| `plugins/sessions/init.lua` | 547 | SessionStatusChanged 使用示例 |
| `maki-storage/src/input_history.rs` | 10-68 | InputHistory 结构体 |
| `maki-ui/src/components/input.rs` | 72, 446 | InputBox 持有 InputHistory |
| `maki-lua/src/api/util/command.rs` | 396-405 | SessionRequest 枚举（无输入历史接口） |

---

---

## 追加记录（2026-08-17_06-27-08）

### 问题：输入框上翻的输入历史，Lua 有办法取得吗？

**结论：目前没有 Lua API 能获取输入框的历史记录。**

### 输入历史的数据结构

`maki-storage/src/input_history.rs:10`：

```rust
pub struct InputHistory {
    entries: VecDeque<String>,  // 历史记录列表
    max_entries: usize,         // 最大条目数
}
```

提供的方法：`load()`, `save()`, `push()`, `get()`, `len()`, `iter()`。

### 输入历史的归属

```
InputHistory → InputBox (maki-ui/src/components/input.rs:72)
                  → App (maki-ui/src/app/mod.rs:215)
                     → UI 事件循环 (maki-ui/src/event_loop.rs)
```

- `InputBox` 有一个 `pub fn history(&self) -> &InputHistory` 方法（`input.rs:446`）
- 每次退出时调用 `save_input_history()` 持久化到 JSON 文件（`session.rs:156`）
- **从未暴露到 Lua 层**

### 为什么拿不到

输入历史是 UI 组件 `InputBox` 的内部状态，不在 `SessionRequest` 枚举（`maki-lua/src/api/util/command.rs:396`）中，也没有对应的 Lua API 可以访问。

### 如果要实现

需要新增一个 Lua API，走 `SessionRequest` 路由到 UI 层取数据：

1. `SessionRequest` 加 `GetInputHistory` 变体
2. UI 层 `event_loop.rs` 响应时从 `self.sessions[self.focused].app.input_box.history()` 取 entries
3. Lua 侧 `maki.session.input_history()` 或 `maki.fn.input_history()` 函数

### 作用域：全局，非项目目录粒度

`InputHistory` 存储在 `StateDir` 中，解析为 XDG 标准目录：

- Linux: `~/.local/state/maki/input_history.json`
- 文件名固定为 `input_history.json`（`input_history.rs:6`）
- **所有项目共享同一个文件**，不是项目目录粒度

### 持久化时机：仅退出时保存

`save_input_history()` 只在 `quit()` 中调用（`app/mod.rs:871`）：

```rust
pub fn quit(&mut self) -> Vec<Action> {
    self.save_input_history();   // ← 仅此处调用
    self.exit_request = req;
    vec![]
}
```

运行过程中，每次提交输入时只推到内存中的 `VecDeque`（`input.rs:240`）：

```rust
pub fn submit(&mut self) -> Option<Submission> {
    let text = self.buffer.value().trim().to_string();
    // ...
    self.history.push(text.clone());  // ← 仅内存操作
    self.discard();
    Some(Submission { text, images })
}
```

**不保存到磁盘，退出时一次性写入。**