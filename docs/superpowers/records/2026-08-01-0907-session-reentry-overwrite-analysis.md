# 会话重入并发覆盖：问题分析与方案

日期：2026-08-01
分支：feat/ai-0726
状态：仅分析，未开工

## 问题描述

`maki -s <session_id>` 可以重入同一个会话。当两个（或多个）进程先后进入同一 session_id 时，**先启动的会话会被后进入的会话覆盖**（后写的进程抹掉先写进程的内容）。

## 涉及路径

- `maki -s <id>` 走 TUI 恢复路径：`src/cmd/tui.rs:196-200` `resolve_session()` → `AppSession::load(id)` → 内存会话，后续落盘全部经 `StorageWriter`（`maki-ui/src/storage_writer.rs`）。
- `--print`（headless）路径的 `HeadlessParams` **没有 session_id 字段**（`src/print.rs:170-184`），永远 `MakiId::generate()`，自身不会重入；但若未来给 headless 加 `-s` 支持，同样暴露此问题。
- 全仓库唯一的进程级文件锁是日志的 `maki.log.lock`（`maki-storage/src/log.rs:12`）。**session 文件没有任何锁**。

## 根因：同一 jsonl 上的"读-改-写"竞争

### 1) 全量重写路径（File::create 截断）— 直接覆盖

`Session::save_to`（`maki-storage/src/sessions.rs:1179-1189`）→ `write_session_file`（645-657）：
用 `File::create` **截断整个文件**，以内存快照重写 header + 全部消息。

典型时序（A 先启动，B 后进入同一 id）：

```
文件初始: H + M1..MN
A: load → 内存 (N)          B: load → 内存 (N)
A: 对话 → 快照 N+1 → save → 文件 = H + M1..MN + msgA
B: 对话 → 快照 N+1 → save → 文件 = H + M1..MN + msgB   ← msgA 被整体覆盖
```

B 若在 A 保存前 load 了旧状态，B 的写入会把 A 刚加的内容整个抹掉 —— 正是"先启动被后进入覆盖"。

`compact`（555-583，`write_full_session` + tmp + rename）与 `migrate_to_jsonl` 走同一全量重写机制。

### 2) 追加路径（SessionLog::append）— 表面安全，compact 兜底覆盖

TUI 的 `StorageWriter` 用追加写（`sessions.rs:472-553`，游标增量），两进程各自追加时消息通常保留。但 `append_or_compact`（`storage_writer.rs:150-165`）在 `CursorAhead` 时回退 **`compact` 全量重写**（用本进程内存快照）：一旦 A 触发 compact（/compact 命令或游标异常），文件被 A 的旧快照重写，B 已追加记录丢失。

另外 `SessionLog::open`（448-453）打开时会截断尾部不完整行：A 写入中途 B 打开，可能误切。

### 3) 附带：update_cwd_index（cwd_latest.json）

`save_to` 每次写"项目目录 → 最新 session"索引，后写者覆盖先写者，选择器里"最近会话"被后来者顶掉。

## 方案选项

**A. 每会话 flock 独占锁（推荐）**
- 沿用 `log.rs` 已有的 flock 模式（`flock_exclusive`），新增 `sessions/{id}.lock`；TUI 持有会话期间加独占锁，`Session::load`/`save`/`SessionLog::open`/`compact` 全部在锁内执行。
- 第二个 `maki -s X` 检测到占用 → 提示"会话正被其他进程打开"，可选项：fork 新 id / 只读进入。
- 优点：直接消除截断竞争与 compact 覆盖；改动集中（sessions.rs + tui 入口）；零新依赖。
- 注意：flock 是 advisory，需所有写路径统一遵守。

**B. 写前重读合并（无锁兜底）**
- `save`/`compact` 前重读磁盘文件，按消息/游标合并再写，而非盲写内存快照。
- 缺点：消息交错合并语义复杂（无可靠顺序）；"检查-写入"之间仍有 TOCTOU，不配锁只能降概率，不根治。

**C. 仅把 headless SessionStore 从全量 save 改为 SessionLog 追加**
- 与 TUI 的 append 机制统一，写增量而非整文件。
- 只能消除 headless 路径的整文件覆盖；TUI 的 compact 竞争仍需 A。

**D. 会话级单实例守卫（PID 心跳）**
- `{id}.lock` 写 PID + 时间戳，第二进程发现存活 PID 即拒绝/转 fork。
- 比纯 flock 多友好提示，但本质仍需 flock 保证原子性。

## 推荐组合

**A（核心）+ C（headless 对齐 append）+ 可选 D（交互提示）**；B 不做（复杂度高、不根治）。

## 相关提交背景

- `41f64d17` 新增 `maki-mock/session-test/` 独立探针工具。
- `29c6fc82` headless（--print）模式补齐会话持久化，新增 `--no-session` 标志。
- `286ecf18` 记录 headless 会话持久化问题与解决文档。

## 待办（未开工）

- [ ] 按 A 实现每会话 flock 锁（含 TUI 入口占用提示 / fork 选项）
- [ ] 按 C 将 headless SessionStore 改为 SessionLog 追加
- [ ] 视需要加 D（PID 心跳 + 友好提示）
- [ ] 补并发回归测试（两个进程同时 append / 同时 save）
