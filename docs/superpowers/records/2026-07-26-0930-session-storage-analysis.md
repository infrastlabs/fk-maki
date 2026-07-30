# 会话存储与输入历史记录分析

## 会话文件（Session）

### 文件路径

```
~/.local/state/maki/sessions/{uuid}.jsonl
```

每个 session 一个 `.jsonl` 文件，UUID 命名，扁平目录，无子目录。

### 目录结构

```
~/.local/state/maki/
├── sessions/
│   ├── cwd_latest.json        ← 项目目录 → session UUID 映射索引
│   ├── {uuid1}.jsonl          ← session 文件
│   ├── {uuid2}.jsonl
│   └── ...
└── input_history.json          ← 输入历史（独立）
```

### 文件格式：JSONL（JSON Lines）

每行一个 JSON 对象，追加写入，崩溃安全（末尾不完整行自动丢弃）：

```json
{"t":"header","d":{"v":1,"id":"uuid","model":"...","cwd":"/path/to/project","created_at":...}}
{"t":"msg","d":{"role":"user","content":[{"type":"text","text":"hello"}]}}
{"t":"msg","d":{"role":"assistant","content":[...]}}
{"t":"out","id":"tool_call_id","d":"tool output text"}
{"t":"sub_msg","sub":"subagent_id","d":{...}}
{"t":"meta","d":{"title":"...","token_usage":{...},"updated_at":...}}
```

记录类型（`t` 字段）：
- `header` — 文件头，第一条记录
- `msg` — 对话消息（user/assistant）
- `out` — 工具输出
- `sub_msg` — 子代理消息
- `meta` — 元数据（标题、token用量、更新时间等）

### 记录维度：Session 维度

- 所有 session 文件平铺在 `sessions/` 目录下
- 每个 session 文件内部记录了自己的 `cwd`（项目路径）
- `cwd_latest.json` 是索引，映射"项目目录 → 最近 session UUID"
- 按项目目录列出时，通过 `cwd_latest.json` 找到该目录下的 session

### TUI 上翻历史时的数据来源

**上翻历史时，数据来自内存**，不是从磁盘读取。

```
磁盘 .jsonl 文件 → Session::load() → history_to_display() → MessagesPanel::messages (Vec<DisplayMessage>)
  ↓ 上翻时
调整 scroll_top 偏移量，从内存中渲染，不涉及磁盘读取
```

两种加载路径：
- **启动/恢复**：一次性从 `.jsonl` 文件加载全部消息到内存
- **实时流式**：agent 推送增量追加到 `MessagesPanel::messages`

---

## 输入历史文件（Input History）

### 文件路径

```
~/.local/state/maki/input_history.json
```

### 维度：**全局的**

只有一个 `input_history.json` 文件，所有项目共享，不区分 session 或项目目录。

### 格式

纯 JSON 数组，最多 100 条，去重（连续重复合并）：

```json
[
  "修复go-build脚本",
  "先kill",
  "用go-build.sh构建",
  "窗口名wXX不要变动",
  "明天继续吧"
]
```

### 代码位置

`maki-storage/src/input_history.rs`

- `load(dir: &StateDir)` → `{state_dir}/input_history.json`
- 最多 100 条，超出丢弃最早的
- 连续重复的条目自动合并（只保留一条）
- 空字符串跳过

---

## 总结

| 维度 | session | input_history |
|------|---------|---------------|
| 文件 | `sessions/{uuid}.jsonl` | `input_history.json` |
| 范围 | 按项目目录索引 | 全局共享 |
| 格式 | JSONL（多行，多种记录类型） | JSON 数组（纯字符串列表） |
| 容量 | 无限制 | 最多 100 条 |
| 用途 | 保存完整对话历史 | 保存用户输入的命令行历史 |

---

记录日期：2026-07-26