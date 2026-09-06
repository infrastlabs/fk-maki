# Maki Monitor 提交审查（2026-09-06）

**日期**: 2026-09-06 22:40
**主题**: monitor 相关提交改动解析、编译判断与代码审查；修正此前将空壳提交误判为真实改动的分析

## 一、提交范围与真实内容

monitor pick 的这几个提交中，**只有最后两个承载真实改动，前面全是空壳**（内容已通过 PR 合并进主历史，本分支 pick 时是 no-op）：

```
e8625909  fix(monitor)   ← 真实内容（+150 行：on_stderr、match 校验、stop_session）
fbd5ea28  feat(monitor)  ← 真实内容（+430 行：插件本体、loader/config 注册、测试）
45702d27  mailbox        ← 空壳：内容已 PR 合并进主历史
988d3832  observation    ← 空壳
0f71b238  plugin jobs    ← 空壳
90366136  subagent fix   ← 空壳
```

`git show` 看到的 diff 只是提交对象自身记录，但这些提交的内容早已通过 PR 进入主历史，本分支 pick 它们时是 no-op。

**修正**：此前"9bd78ee0 误删了 45702d27 的 mailbox 字段"的判断不成立——删的就是空壳提交的字段，不影响任何实际功能。9bd78ee0 是精确 revert，一次撤掉了两个不想保留的提交（90366136 subagent fix + 45702d27 mailbox），tools/mod.rs 删的 2 行正是 45702d27 的字段，目的就是让 mailbox 不挂在 ToolContext 上。

## 二、代码审查结论（monitor 真实改动 = fbd5ea28 + e8625909）

**编译判断：能通过**（静态分析，未编译）

| 检查点 | 状态 |
|--------|------|
| ToolContext 无 mailbox 字段 | ✓ 无任何 ctx.mailbox 残留引用（Lua session.notify 走静态 SessionMailbox::notify，不依赖 ctx） |
| agent.rs 不再用 Emit | ✓ grep 为空，import 恢复为 use maki_agent::agent::tool_dispatch;，callable/run 都存在 |
| Message::observation | ✓ types.rs:258/267，MessageKind::Observation 齐全 |
| mailbox.rs 自身 | ✓ 独立完整（register/notify/drain/claim_wake/Drop），不依赖被删字段 |
| headless.rs 的 mailbox: Some(...) | ✓ 是 AgentParams 字段（run.rs:76），与被删的 ToolContext 字段无关 |

**monitor 插件逻辑（plugins/monitor/init.lua）**：
- owner = "plugin"（follow-up 要求，job 随插件而非会话存活）✓
- caller_session 用 ctx:session_id()（#687 已实现，ctx.rs:189）✓
- stop_session 读 ev.data.session_id——fire_session_autocmd（app/mod.rs:444-451）会注入 session_id，修复正确（旧版 stop_all 的注释抱怨"SessionReset 不带 session id"，现在带了）✓
- match 模式用 pcall 包住，非法模式只报一次错不崩 ✓
- 边缘点（非 bug）：jobstop 抛错路径下 monitors 表可能残留，但符合 maki"抛错=程序员错误"约定

## 三、与熄灯设计的关系

- monitor 工具 = 熄灯设计"job→会话"异步投递通道的落地版
- session mailbox = 熄灯设计"信箱通知"的 agent 侧原语（SessionMailbox::notify/drain/claim_wake）
- 两者互补：monitor 是后台 watcher，mailbox 是投递通道，goal 是会话内循环

## 四、结论

当前 HEAD（e8625909）就是干净目标态：ToolContext 无 mailbox 字段、mailbox 走独立静态通道，均符合预期。无需恢复任何被删字段。

## 五、空壳提交 → 主历史（prs/main）对应项

monitor 相关空壳提交的内容均已在主历史中存在，对应关系如下：

| 空壳提交（feat/ai-0903 上） | 主历史对应提交 | 说明 |
|---------------------------|--------------|------|
| 0f71b238 feat(lua): add plugin-owned jobs (07-28) | 91852e22 feat(lua): add plugin-owned jobs (07-28) | 同标题同内容，最直接对应 |
| 988d3832 feat(providers): observation (07-26) | 并入 00a2959e feat(agent): add a bounded session mailbox (08-01) | main 上 observation 与 mailbox 合并成一个提交实现（MessageKind::Observation 首次出现在 00a2959e 的 types.rs） |
| 45702d27 feat(agent): shared session mailbox (07-28) | 00a2959e feat(agent): add a bounded session mailbox (08-01) | main 上名为 bounded mailbox（加了容量上限，见 mailbox.rs:217 行），而非分支上的 shared |
| 90366136 fix(agent): subagent cancel (07-25) | 0f67054c fix(agent): stop a second subagent cancelling the first (07-25) | 同标题同内容，同一天 |

关键发现：
1. 三个空壳提交都有主历史来源：0f71b238→91852e22、90366136→0f67054c 是逐字对应的；988d3832+45702d27 在 main 上被合并压缩成 00a2959e（所以分支上拆成两个 pick，main 里是一个 bounded mailbox 提交）
2. main 版本更先进：00a2959e 是 bounded mailbox（带容量/压缩处理，compaction.rs +32 行），比分支上的 shared mailbox 版本完整——分支上的空壳只是历史快照，真实代码以 main 为准
3. 这也再次印证：9bd78ee0 revert 掉的字段在 main 上本来就不存在该形态——main 的 00a2959e 才是 mailbox 的权威实现
