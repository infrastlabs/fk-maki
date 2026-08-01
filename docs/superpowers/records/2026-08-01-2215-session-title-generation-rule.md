# 会话标题提炼规则：从首条消息生成

日期：2026-08-01
分支：feat/ai-0726
类型：代码问答记录

## 问题

新 session 的名字（标题）是如何从首条消息取得的？有没有提炼规则？

## 答案：有，规则集中在 `generate_title()`

核心代码位置：
- `maki-storage/src/sessions.rs:338-356` — `generate_title(messages)`（提炼规则本体）
- `maki-storage/src/sessions.rs:1252-1256` — `update_title_if_default()`（触发点）
- `maki-providers/src/types.rs:240-247` — `TitleSource for Message`（取文本来源）
- `maki-providers/src/types.rs:211-224` — `user_text()` / `first_text_content()`（文本优先级）

## 提炼规则

### 1. 候选文本来源（TitleSource::first_user_text）

- 只取**第一条 user 消息**的文本；非 user 消息（assistant / system / tool）直接跳过。
- `user_text()` 取文本优先级：
  1. `display_text`（若非空）— 展示层覆盖文本；
  2. 否则取 `content` 里**第一个非空 Text 块**（`first_text_content`）。

### 2. 规范化与截断（generate_title）

```
取首条 user 文本 → trim，空则返回默认 "New session"
→ normalize_title（split_whitespace 折叠所有空白/换行为单空格）
→ 长度 ≤ 60（MAX_TITLE_LEN，sessions.rs:36）→ 原样返回
→ 超过 60：
   floor_char_boundary(60) 截到字符边界（不切多字节 UTF-8）
   → 最后一个空格位置 > 30（MAX_TITLE_LEN/2）→ 在该空格词边界截断 + "…"
   → 否则 → 硬截 + "…"
```

细节：
- `normalize_title`（sessions.rs:334-336）折叠空白，也避免粘贴代码块把标题撑歪（影响单行 UI 的宽度对齐）。
- 词边界截断只在空格位置 > 30 时启用，否则回退硬切，保证标题不会过短。

### 3. 触发时机（update_title_if_default）

- 仅当标题仍为默认值 `"New session"`（sessions.rs:35）时才生成；
- 已重命名或已生成过的标题**不会被覆盖**（幂等）；
- 调用点：`SessionStore::record_turn`（headless 每次记录回合后）与 TUI 保存路径。

## 摘要表

| 步骤 | 规则 |
|---|---|
| 取哪条消息 | 第一条 user 消息（跳过 assistant/system/tool） |
| 取哪段文本 | `display_text` 优先，否则第一个非空 Text 内容块 |
| 空白 | trim + 折叠所有空白为单空格 |
| 上限 | 60 字符，优先词边界截断加 `…`，否则硬截加 `…` |
| 幂等 | 仅标题仍为 "New session" 时生效 |

## 相关常量

- `DEFAULT_TITLE = "New session"`（sessions.rs:35）
- `MAX_TITLE_LEN = 60`（sessions.rs:36）
