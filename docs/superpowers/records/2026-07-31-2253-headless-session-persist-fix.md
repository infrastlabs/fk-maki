# Headless（--print）模式会话未持久化：问题与解决

日期：2026-07-31
分支：feat/ai-0726
相关提交：`41f64d17`（测试工具）、`29c6fc82`（修复）

## 问题现象

用 `maki -m <model> --print "hi"` 以 headless 模式运行后：

1. 命令正常输出结果，退出码 0。
2. 终端打印了 `session_id`（如 `CdaGfcc14BsvZbyUVQDvn`）。
3. 但 `~/.local/state/maki/sessions/` 目录下**找不到**对应的 `.jsonl` 文件。
4. 全盘文件系统搜索也查不到任何匹配该 session_id 的文件。

与此对比：TUI 交互模式下会话能正常持久化，`.jsonl` 文件会写入磁盘。因此问题被怀疑集中在 headless 代码路径。

## 排查过程

### 1. 假设：StateDir 解析失败

最初怀疑 `StateDir::resolve()` 在 headless 环境下解析失败，导致会话静默丢弃。

- 在 `SessionStore::open` / `open_in` / `save` 中加了 `eprintln!` 调试日志。
- 编译完整二进制遇到 OOM：本机仅 1.9GiB 内存 / 2 核，默认并行编译被 kill。
  - **解决**：`CARGO_BUILD_JOBS=1 cargo build -j 1` 单任务编译成功。
- 运行后日志显示：
  ```
  [DEBUG] SessionStore::open session_id=CdaGfcc14BsvZbyUVQDvn
  [DEBUG] StateDir::resolve OK: /_ext/home/headless/.local/state/maki
  [DEBUG] session not found, creating new
  [DEBUG] session.save OK
  ```
- 结论：`StateDir::resolve()` 正常，`save` 也返回 OK，且文件确实写入了磁盘。
- 但此时发现：**这段调试代码本身新增了会话写入逻辑** —— 日志能出现，恰恰是因为我们加了调用。

### 2. 根因确认：headless 路径从未调用 SessionStore

对照 `git show HEAD:maki-agent/src/headless.rs`（修复前基线）：

- 基线 `spawn()` 只做了三件事：
  1. `MakiId::generate()` 生成 session_id（所以终端会打印）；
  2. 构造 `Agent` 并 `run()`；
  3. `mcp_shutdown`。
- **从头到尾没有 `SessionStore::open` / `record_turn` / `save` 的任何调用**。
- 会话数据随进程结束直接丢弃，`.jsonl` 永远不会写入。

也就是说：headless（--print）模式**从未实现会话持久化**，而非"写入失败被静默吞掉"。TUI 路径的持久化逻辑（`src/cmd/tui.rs` 的 `resolve_session` + `AppSession`）在 headless 分支根本不会执行。

## 解决方案

### 修复内容（提交 `29c6fc82`）

在 `maki-agent/src/headless.rs` 的 `spawn()` 中，agent 运行结束后新增会话持久化：

```rust
if !no_session
    && let Some(mut store) =
        SessionStore::open(session_id, &session_working_dir, &model.spec())
{
    store.record_turn(history.as_slice(), model.spec());
}
```

要点：

- `SessionStore::open` 内部逻辑复用已有代码：`StoredSession::load` 不存在则 `new` 并立即 `save`（落盘 header），随后 `record_turn` 追加消息与 meta。
- `model` 改为 `model.clone()` 传参，避免 move 后后续仍需使用 `model.spec()`。
- 新增 `HeadlessParams.no_session: bool` 字段，配合 `--no-session` CLI 标志（`src/cli.rs` → `src/cmd/tui.rs` → `src/print.rs` 三级透传），供 CI/脚本场景跳过落盘。
- 会话工作目录使用 `working_dir.clone()` 提前保存，防止闭包内 move 冲突。

### 验证

- 修复后运行 `maki -m cust07-mscope/deepseek-ai/DeepSeek-V4-Flash --print "hi"`：
  - 正常输出 `Hi! I'm Maki, an AI coding agent. How can I help you today?`（同时验证了 ModelScope SSE 修复仍生效）。
  - `~/.local/state/maki/sessions/CdaGfcc14BsvZbyUVQDvn.jsonl` 成功写入，内容含 `header` / user `msg` / assistant `msg` / `meta` 四条记录。
- `cargo test -p maki-agent session`：9 个会话相关测试全部通过。

### 测试工具（提交 `41f64d17`）

新增独立探针工具 `maki-mock/session-test/`，不依赖 maki 二进制，直接调用 `maki_storage::StateDir::resolve()` 与 `Session::save()`，用于隔离验证状态目录解析与会话落盘：

```rust
match StateDir::resolve() {
    Ok(dir) => {
        let sessions_dir = dir.ensure_subdir("sessions").unwrap();
        let mut session = Session::<Message, TokenUsage, serde_json::Value>::new("test/model", "/_ext/home/headless");
        session.id = "CdZuiZUNCf1c715Qh1etW".parse().unwrap();
        session.save(&dir)?;
    }
    ...
}
```

注意：该工具需复用主 workspace 的 `target/`（`CARGO_TARGET_DIR`）编译，独立 `target/` 会占用额外约 130M 磁盘（本机磁盘常接近满）。

## 经验总结

1. **"session_id 有打印但文件不存在"未必是写入失败**，也可能是该代码路径根本没有写入逻辑。排查时应先对照基线确认调用链是否存在。
2. headless（`--print` / SDK 模式）与 TUI 是两套独立的会话处理代码路径，改动 TUI 的持久化不会自动覆盖 headless。
3. 内存受限环境（1.9GiB / 2 核）编译大型 Rust workspace 必须用 `-j 1`，否则 OOM 被 kill；磁盘满时可删除 `target/debug/incremental` 释放约 1.9G。
