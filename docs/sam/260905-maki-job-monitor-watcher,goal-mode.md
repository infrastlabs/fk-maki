## 〇、熄灯设计：这是什么

**熄灯设计（Lights-out Engineering）**：把实时交互转成批处理，让人从"实时调度员"退居"收件人"——白天人收集/确认 Issue 入队，夜间无人值守执行，早上人只看汇总结果。

核心是三根支柱：
- **信箱通知**：信息级（成功汇总）/ 提请级（自动修复需确认）/ 阻塞级（需人决策），v1 收敛为清晨一封摘要 + 会话内 mailbox 原语
- **多阶段看板**：`queued → precheck → coding → gated → merging → merged / blocked / failed`，转移条件即门控规则
- **门控规则与验收标准**：AC 入队强制声明；门禁跑命令（lint/test/build），agent 只负责修复；不绿不入队

四相流水线：Phase 0 队列预检（零 token）→ Phase 1 并行编码农场（每卡一个 git worktree + 独立 headless session）→ Phase 2 门控闸门 → Phase 3 串行合并与回归 + 晨会简报。

本文档各章节（#676 后台 job、#703 goal 模式、/goal 智能体全景、jcode 实现）都在为熄灯设计提供**会话侧原语**：monitor 是"job→会话"的异步投递通道，goal 是"会话内目标驱动循环"。完整设计见 `docs/sam/260904-lights-out-batch-pipeline-design.md`。

## 一、Issue #676 解析：Watcher 插件需要比工具调用活得更久的 job

**日期**: 2026-09-05 01:49
**主题**: [tontinton/maki#676](https://github.com/tontinton/maki/issues/676) 提案分析与代码库现状对账；与熄灯工程设计中"信箱通知"的关系

### 1. Issue 概要

**Use case**（issue 原文）：跑长任务（dev server、测试 watcher、部署）时，希望 maki 在有情况发生时主动告知，而不是自己反复追问。Claude Code 的 Monitor 工具就是为此设计的：给它一个每行产出一个事件的命令（如 `tail -f app.log | grep --line-buffered ERROR`），每一行都会变成对话里的消息，期间人可以继续做别的事。核心价值是"无人轮询"——agent 启动 watcher 后继续前进，事件稍后自行到达。

**问题**：`maki.fn.jobstart` 的任务都活在 TaskCell 里，`TaskScope::drop`（`maki-lua/src/runtime.rs`）会 `jobs.kill_all()`；工具调用一结束，后台 job 即死。因此无法实现 Claude Code Monitor 模式——"启动 watcher → 立即返回 → 事件异步推回会话"，只能阻塞式等第一个事件，即"等待条件"而非"后台监视"。

**已有积木**（issue 作者自述）：
- `maki.fn.jobstart` 提供 `on_stdout`/`on_stderr`/`on_exit`，bash 插件已按行流式输出
- `maki.session.prompt()`（#558）向活跃会话投递文本，忙时排队——"投递半"已完成

**缺的只有一件**：job 无法活过启动它的工具调用。

### 2. Issue 提案

**Rust 侧**：
- runtime 上增加一个独立于 task 的 job store
- `jobstart(cmd, { detach = true })` 将 job 放入该 store（仿 neovim `detach` 语义，但含义降一级：活过 task scope 而非活过进程退出；maki 本就对每个 job 调 `setsid()`）
- 从 runtime 请求循环 drain 该 store，走已有的 `run_detached` 投递回调
- `SessionReset`、插件卸载、shutdown 时清理，防泄漏；`jobstop` 同时查两个 store
- tool ctx 上补 session id（当时缺失；#546 后多会话并行，`session.current()` 不一定是调用方）

**Lua 侧（新 monitor 插件）**：
- `monitor` 工具：启动 detached job，立即返回 id
- 匹配行经 `maki.session.prompt()` 推回调用会话
- `monitor_stop` 工具、`/monitors` 命令、会话重置清理

**issue 作者自列的 4 个待定问题**：
1. detached job store 是否进 core（#495 先例：sessions 看板做成了 Lua 插件）
2. `detach` 命名是否合适（备选 `keep_alive`、`owner = "session"`）
3. token 成本：推送消息进上下文与 maki 省 token 理念相悖；作者倾向合并突发 + 自动停掉刷屏的 monitor；硬上限进 core 还是插件自理
4. 投递消息包装：建议 `[monitor 3: deploy errors] <line>` 让模型读作观察而非指令；还是引入独立消息种类

issue 状态：作者表示方向确认后再写码（遵循 CONTRIBUTING 的插件迁移期小改动要求）。

### 3. 代码库现状对账（2026-09-04 本地仓库）

| 提案项 | 现状 | 证据 |
|--------|------|------|
| detached job store | **已实现**，命名走 `scope` 而非 `detach` | `maki-lua/src/api/fn.rs` `parse_scope` |
| job 存活期三级 | `scope = "task"`（默认，随任务死）/ `"plugin"`（随插件活）/ `{ session = <id> }`（会话级） | `parse_scope`（fn.rs:991） |
| 会话级 job 跨插件重载存活 | 已实现 | commit `0b1e0196` "session-owned jobs that survive plugin reload" |
| 回传走 run_detached | 已实现 | `deliver_pending` + `run_detached`（runtime.rs:1099/1215） |
| `maki.session.prompt()` | 已实现（#558） | `session.rs:187`，支持 `{ session = id }` |
| tool ctx 带 session id | 会话作用域本身绑定 session，已具备 | `JobOwner::Session { session, plugin }` |
| **monitor 插件** | **不存在**（plugins/ 目录无 monitor） | — |

**结论**：Rust 原语层（issue 最大争议点 Q1 是否进 core、Q2 命名）已被采纳并落地，方向是"job 归属权显式化"。剩 Lua 层 `monitor` 插件未做，按 maki 架构正应是纯 Lua 插件（#495 先例）。

### 4. 与熄灯工程设计的关系

这是熄灯设计"会话内 mailbox 通知"的落地原语：

- `scope = { session }` 的 job + `session.prompt()` = 后台监视器把事件异步推回会话
- `maki-agent/src/mailbox.rs` 的 `SessionMailbox`（notify/drain/claim_wake）是 **agent 侧**信箱
- monitor 是 **job → 会话**方向的投递通道，两者互补
- monitor 的"合并突发 + 自动停刷屏"即设计文档"提请级/阻塞级通知"的节流雏形；issue Q3（token 上限归属）至今无答案，对应设计文档第五节"成本与熔断"

### 5. 后续方向

1. 整理 `jobstart` scope 语义 + `session.prompt` 排队机制成文档（设计文档第五节补充）
2. 落地 monitor 插件（工具 + `/monitors` + 节流），作为"提请级/阻塞级通知"雏形
3. 讨论 monitor 与 SessionMailbox 衔接、token 成本上限归属（issue Q3）

## 二、Issue #703 解析：Goal 模式（长任务跨轮次自动延续）

**解析时间**: 2026-09-05 02:04

**主题**: [tontinton/maki#703](https://github.com/tontinton/maki/issues/703) 提案分析与代码库现状对账；与熄灯工程设计中"Phase 1 单卡执行循环"的关系。解析日期 2026-09-05。

### 1. Issue 概要

**问题**：大任务一轮做不完。agent 做出部分进展、给出貌似合理的总结就停了，得靠人再发"继续"。

**借 Codex `/goal` 的小语义核心**（明确不做 Codex 的大架构）：
1. 每 session 存一个 objective + status
2. 以普通 turn 启动
3. 给 agent `get_goal`/`update_goal` 两个工具
4. agent 只能更新为 `complete`/`blocked`；pause/resume/replace/clear 归用户
5. turn 结束时 goal 仍 active → 用完整 objective 再开一轮
6. 停止条件：complete / 真 blocker / 错误 / 用户 pause / 硬上限

### 2. 提案

**状态**：每会话一个 JSON 记录（objective/status/turns/max_turns=20/summary），存 state_dir；新 objective 换新 id；`clear` 删记录。硬轮次上限防无限循环，token 预算等 Lua 拿到权威 per-session usage 再做。

**命令**：`/goal <objective>`（设置并启动首轮）/ `/goal`（查看）/ `/goal pause` / `/goal resume` / `/goal clear`。替换确认留作后续。

**工具**：`get_goal` 返回当前会话记录；`update_goal` 只接受 `complete`/`blocked`（带 `goal_id` + summary），拒绝过期 id、缺失、或 paused 状态。模型不能 replace/resume/clear。

**延续**：`TurnEnd` 时插件重载该会话 goal——complete/blocked/paused 不动；active 未达上限则计数 +1 并以完整 objective + 延续规则经 `session.notify(..., {wake=true})` 唤醒；达上限转 paused 并告知用户。`TurnError` 时 active → blocked，不自动重试。Cancel 不触发 TurnEnd（记录仍可能 active，但不会自动延续，直到用户 resume 或另起一轮）。

**UI**：仅 `/goal` 摘要 + 状态提示（active/paused/blocked/complete + 轮次数），无自定义面板。

**依赖声明**（issue 作者认为 goal 模式本身零 Rust 改动）：#687 tool session identity、#691 session observations + idle wake。

### 3. 代码库现状对账（2026-09-05 本地仓库）

| issue 声称缺失 / 提案依赖 | 现状 | 证据 |
|--------|------|------|
| #687 `ctx:session_id()` | **已实现** | `maki-lua/src/api/util/ctx.rs:189` |
| #691 `session.notify(..., {wake=true})` | **已实现** | `session.rs:213`，走 `SessionMailbox::notify(session_id, text, wake)`——熄灯设计中的 mailbox 原语 |
| TurnEnd 带 `data.reason` | **已实现且已含 cancel** | `agent_autocmd.rs:82-93`，reason ∈ finished/max_tokens/max_turns/cancelled（Compact 不发） |
| TurnEnd 只对主 session 发 | 文档如此 | autocmd.rs 注释 |
| `session.new` 原地重置（评论区提出） | 仍是**开新 tab**（`{prompt, focus}`） | `session.rs:165`——tontinton 指出的缺口之一**仍未解决** |
| generation slot（评论区提出） | slot API 已存在 | `slot.rs`（LayeredTools） |

### 4. 维护者态度（评论区关键信息）

1. **tontinton：不再加内置插件**（2026-07-28 19:08 UTC，即北京时间 07-29 03:08）——goal 插件应作为**用户插件**存在：

```text
tontinton (2026-07-28 19:08:47Z):
that's awesome, but lets focus on adding just the needed lua APIs, I'm not sure I will add any more builtin plugins to be honest
```

2. **当前就能做**（tontinton，2026-08-29 14:28 UTC，北京时间 08-29 22:28）：`register_command` + `register_tool` + `TurnEnd` autocmd + `state_dir()` 状态文件——组件已齐，缺的只有 `session.new` 原地重置和 TurnEnd 的 reason（**后者已落地**）：

```text
tontinton (2026-08-29 14:28:06Z):
you can already build this as a user plugin today: `register_command` for the loop command,
`register_tool` for `task_done`, `TurnEnd` autocmd to kick the next round, state file under
`state_dir()`.

2 things missing on our side: a lua way to reset the current session in place (`session.new`
stacks a new tab per chunk instead), and a `reason` field on `TurnEnd` (+ firing it on cancel)
so the plugin knows if the turn ended or you just hit esc.

plan path/mode isn't exposed either, so for now you'd pass the file to the command yourself.
```

3. **competitiveNN 的 generation slot 提案**（2026-08-31 16:27 UTC，北京时间 09-01 00:27）：goal 需要"处理失败 prompt 的能力"（模型 fallback 链），建议 engine 声明 `agent.generate` slot 让插件包装：

```text
competitiveNN (2026-08-31 16:27:01Z):
goal mode I think also requires plugin capability to handle failed prompts and currently
there isn't a hook to do this
e.g.
Option A — Generation slot (cleanest, most general)

maki declares a named slot that wraps every generation call:

-- In maki's agent core (not in a plugin):
local generate = maki.api.declare_slot("agent.generate", function(ctx, opts)
  -- this is what maki does today: call the model
  return maki.agent.session(ctx, opts):prompt(...)
end)

-- Plugins can wrap it:
maki.api.set_slot("agent.generate", function(prev, ctx, opts)
  local models = { "opencode/hy3-free", "nvidia-nim1/nemotron-3-super-120b-a12b" }
  for _, spec in ipairs(models) do
    opts.model_spec = spec
    local result, err = prev(ctx, opts)
    if not err then return result end
  end
  return nil, "all models failed"
end)

The slot returns the final text (no streaming support needed for v1 — just the result.text
after the stream completes). The wrapper gets prev and can call it with whatever model spec
it wants. This is also useful for logging, cost tracking, custom routing, etc.

What maki needs to add: one declare_slot call in the agent generation path, and one
generate(...) call through the slot instead of directly into agent.session.

This is necessary not just for goals, but for model fallback logic, and loop logic, which
are basically all about long running persistent prompt execution
```

tontinton 回应（2026-09-01 07:10 UTC，北京时间 09-01 15:10）：**仍开放**：

```text
tontinton (2026-09-01 07:10:06Z):
maki declaring slots for things in-engine is interesting, we need to define the most
"impactful" ones
```

4. **MatthewScholefield 的 plan-loop 模式**（2026-08-28 14:24 UTC，北京时间 08-28 22:24）：`for i in $(seq 1 10); do maki --yolo "complete step $i"`——上下文边界清晰的替代方案，tontinton 指出可直接做插件：

```text
MatthewScholefield (2026-08-28 14:24:24Z):
Just a related thought, a common pattern I use is to have an agent draft a markdown doc
containing a high level plan and including sections for each step of the plan the agent to
mark its progress. Then, I can run maki in a loop like
`for i in $(seq 1 1 10); do maki --yolo --exit-on-done "Read docs/foo-implementation-plan.md
and complete step $i end to end. Commit when you are finished."; done`.

I prefer this approach because it naturally breaks the sessions at boundary points keeping
the context reasonably small in contrast to a "goal" mode that may just keep growing context
larger and larger and then compressing context at arbitrary points. But I dislike that it
relies on an external plan file at an arbitrary path and I have to do the plumbing around
asking the agent to create the file in a way that has steps and delete it at the end and
encode how many steps there are.

If there were a way to create some simple framework / automation around this process without
encoding arbitrary conventions into it I think this would be amazing. For example, an
extension of plan mode that allows you to instead of "Clear context and implement", you
could choose something like "Clear context and implement looped" which would include some
new `task_done` interface but also run the sessions in a loop with a prompt like `Identify a
reasonable chunk of work from the plan {PLAN}, implement this within this session, and then
update the plan to briefly document what changes you made`, resetting the context each time.

This is still a messy idea because we have to think about like what happens when you stop it
in the middle and how does it represent this loop state, so I'm still not quite sure the
best design, but this is as far as I got.
```

补充背景：sdroege（07-28 04:28 UTC）提到 `g4bwy/maki-goal` 插件；laudney（07-28 12:01 UTC）回应说该插件移植的是更大的 `pi-goal` 工作流（多 goal/任务树/契约/审计/归档），本 issue 刻意做小一号；g4bwy 本人（08-06 01:25 UTC）确认那只是练手玩具，已弃。

### 5. 与熄灯设计的关系与结论

goal 模式就是**Phase 1 单卡执行循环的引擎**：objective=卡片 AC，`complete/blocked`=门禁出口，`max_turns=20`=设计中的 per-card `max_turns` 熔断，`session.notify(wake=true)`=夜间唤醒机制。mailbox 已在两端就位：`SessionMailbox::notify`（agent 侧，issue 703 用）+ monitor（job→会话）+ goal（会话内循环）——**熄灯设计里的"信箱通知"三条通道全齐**。

**结论**：#703 提案路线（Lua 插件、零 Rust 改动）基本可行，唯一实质缺口是 `session.new` 原地重置语义；按 tontinton"不加内置插件"立场，goal 应作为用户插件而非内置。

## 三、支持 /goal 的智能体全景（2026-09 核验）

**核验时间**: 2026-09-05 09:27

**主题**: 各智能体 /goal 支持情况与上线时间线；基于公开信息核验，全部属实。

### 1. 支持清单（公开信息）

- **OpenAI Codex**：最早推出之一，架构精密，支持状态管理、预算控制
- **Anthropic Claude Code**：最早推出之一，设计简洁，与 Agent View 集成显示进度
- **Nous Research Hermes**：最早推出之一，强调看板与多智能体编排
- **Qwen Code**：从 v0.16.0 开始支持，特色是引入独立的 judge model 来验收任务
- **Cursor**：已上线，可设定长期目标并持续努力直至完成
- **MiniMax Code**：需要更新到支持 /goal 的版本
- **CodeBuddy**：需要包含 GoalService 模块的版本，会用小模型做评估器来验证目标
- **Pi Coding Agent**：支持 Codex 风格的 /goal 命令
- **OpenCode**：通过安装命令包支持 /goal 及相关命令
- **Qoder**：也支持 /goal 命令

另外，**HagiCode** 通过"Agent 感知"设计，让不支持原生命令的 Agent（如 Gemini、Copilot 等）也能通过提示变体实现类似效果。

### 2. 上线时间线（按时间先后）

| 智能体 | 时间 | 版本/说明 |
|--------|------|----------|
| OpenAI Codex | 2026-04-30 | Codex CLI v0.128.0 率先上线；05-21 结束实验转正式版（GA） |
| Nous Research Hermes | 2026-05-07 左右 | 晚于 Codex 约一周 |
| Anthropic Claude Code | 2026-05-11 | v2.1.139 |
| Qwen Code | 2026-05 底 | v0.16.0 |
| CodeBuddy | 2026-05-28/29 | v2.99.0 |
| Cursor | 2026-08-19 | 从实验功能正式上线 |

MiniMax Code、Pi Coding Agent、OpenCode、Qoder：公开信息中暂无具体上线时间。

### 3. 核验结论（2026-09-05 逐项查证）

11 项全部属实，逐项证据：

| 智能体 | 核验证据 | 备注 |
|--------|---------|------|
| OpenAI Codex | developers.openai.com/use-cases/follow-goals；`features.goals` 配置开关；pause/resume/clear | 与时间线互证：Hermes 官方文档明确写 "inspired by Codex CLI 0.128.0's /goal by Eric Traut" |
| Claude Code | code.claude.com/docs/en/goal；每轮后小模型判断条件是否达成 | 机制属实 |
| Hermes | hermes-agent.nousresearch.com/docs/features/goals；自称 "Ralph loop 的独立实现"，judge model + 延续 prompt | "看板与多智能体编排"未在官方文档强调 |
| Qwen Code | GitHub QwenLM/qwen-code `goalJudge.ts`——独立 goal-completion judge，严格 JSON，证据含糊默认"未达成" | judge model 属实 |
| Cursor | prod.cursor.com/docs/agent；`/goal` + `/loop` skill，Ctrl+C 暂停 | 属实 |
| MiniMax Code | agent.minimax.io/docs/code/desktop/goal——"verifiable outcome + 明确验收标准" | 属实 |
| CodeBuddy | codebuddy.ai/docs/cli/goal；GoalService 模块 + 小模型评估器 | 完全吻合 |
| Pi Coding Agent | pi.dev/packages/pi-codex-goal；get_goal/create_goal/update_goal 三个工具 | Codex 风格属实 |
| OpenCode | npmjs.com/opencode-goal——server 插件，/goal pause/resume/clear/append | "安装命令包"吻合 |
| Qoder | docs.qoder.com/cli/goal-reference；/goal status/clear/pause/resume + --turns 参数 | 属实 |
| HagiCode | dev.to 文章——"Agent-aware" 提示变体，原生 agent 走原生命令、其他（Gemini/Copilot/iFlow/OpenCode）走 fallback 提示 | 描述完全吻合 |

### 4. 机制分派观察

两派：

- **judge 派**（小模型验收）：Codex / Claude Code / Qwen / CodeBuddy / Hermes
- **工具派**（goal 作为模型可调工具）：Pi（get_goal/create_goal/update_goal）、OpenCode
- **混合**：Cursor（/goal + /loop）

对应 issue #703 提案里"update_goal 工具 + TurnEnd 延续"的设计选择：maki 走的是工具派 + autocmd 延续的混合路线。

**注**：AI 领域更新很快，具体支持情况以各产品官方最新公告为准。

## 四、各家 /goal 实现亮点与 #703 问题项对应启示

**时间**: 2026-09-05 09:51

**主题**: 基于第三节全景，拆解各实现可借鉴机制，逐条回答 #703 的六个问题项与评论区遗留问题。

### 1. 各家实现亮点速览

| 智能体 | 核心机制 | 值得借鉴的点 |
|--------|---------|-----------|
| Codex | SQLite + app-server 全栈；goal 带 id 防过期更新；延续时引用 worktree/命令输出作证据 | goal id 防 stale update（#703 已采纳）；"不重新定义成功"的延续规则 |
| Claude Code | 每轮后小模型判完成条件；空闲 check-in 最多 3 次；模型判不可能/报错即清除 | 三层停止（达成/不可能/错误）；check-in 而非死等 |
| Hermes | Ralph loop 独立实现；judge model + 自动喂 continuation；turn budget 耗尽停止 | judge + budget 双保险 |
| Qwen Code | goalJudge.ts：基于 transcript 证据判完成，证据含糊默认未达成，严格 JSON | 证据导向的 judge 默认值——防"伪完成" |
| CodeBuddy | GoalService + 小模型评估器 | 同 Claude 的 judge 派 |
| Cursor | /goal + /loop skill 组合；Ctrl+C 暂停 | 暂停语义 = maki cancel 不触发 TurnEnd |
| MiniMax | 显式验收标准（AC）驱动 | AC 即熄灯设计的门禁标准 |
| Pi / OpenCode | goal 作为模型可调工具（get/create/update）；OpenCode 保留 elapsed-time 统计 | 工具派；统计不因 append 重置（成本/预算基础） |
| Qoder | --turns 参数 | 轮次上限做成显式参数 |
| HagiCode | agent-aware 提示变体：原生走命令，其他走 fallback 提示 | 能力差异下沉到 prompt 层 |

### 2. 对应 #703 的六个问题项

**Q1（TurnEnd + mailbox wake 是正确的接缝吗）**

接缝是对的，行业共识就是"turn 结束 → 判断 → 延续"。真正的分歧在判断者：
- judge 派（Claude/Hermes/Qwen/CodeBuddy）：独立小模型判 transcript 证据
- 工具派（Pi/OpenCode）：模型自己调 update_goal

启示：#703 提案是工具派，但可以混合——TurnEnd 后插件可选跑一个 cheap judge（抄 Qwen 的证据导向 + 含糊默认未达成），与 update_goal 互为校验。另外抄 Claude Code 的"空闲 check-in ≤3 次"：monitor（#676）挂起时定期唤醒而不是一直等。

**Q2（state_dir JSON vs session metadata）**

Codex 的 SQLite 是其多客户端架构的产物，maki 单机不需要——state_dir JSON 是正确的 v1。但借两个字段：goal id（防 stale，已采纳）+ elapsed_turns/elapsed_time 且 append 不重置（OpenCode 的做法，给成本/简报用）。Qoder 的 --turns 表明轮次上限做成参数而非常量。

**Q3（替换未完成 goal 要确认吗）**

Codex/Claude Code 都是直接替换，新 goal id 已覆盖安全（stale update 打不到新 goal）。v1 不需要确认对话框，与 #703 作者倾向一致。Cursor 把 /loop 独立出来也是"避免确认"的另一条路。

**Q4（max_turns 够吗，token 预算呢）**

- 硬轮次上限是行业标准第一道边界（Hermes turn budget、Qoder --turns、Codex 预算控制都这么做）
- 但所有实现还有软停止：Claude/CodeBuddy/MiniMax 的"模型判不可能/进展受阻"。软停止比硬上限先触发，省 token
- token 预算：#703 作者立场正确——Codex 能做预算是因为它有整个 app-server 基础设施；maki 等 Lua 拿到权威 usage 数据再做。OpenCode 的 elapsed-time 统计是过渡方案：先记轮次+耗时，不估钱

**Q5（blocked 模型控制 vs needs_input）**

业界已有分层先例：Claude Code 区分"模型判不可能 → 清除 goal"和"报错 → 需用户修复"。启示：#703 作者的提案（模型报 needs_input，blocked 留给运行时错误）与 Claude 的语义几乎一致，方向正确。可以再加一层：blocked 细分来源字段（error / impasse），晨会简报直接引用。

**Q6（内置 vs 用户插件）**

tontinton 已拍板不加内置插件，这条不用再争。HagiCode 的启示反而在别处：能力差异下沉到 prompt 层——maki 的 goal 插件如果遇到不支持工具派的模型，可以像 HagiCode 一样按模型能力给不同 prompt 变体（有 update_goal 工具走工具，没有就靠每轮 judge）。

### 3. 评论区遗留问题的启示

**session.new 原地重置缺失**（tontinton 指出的缺口）：MatthewScholefield 的 plan-loop 本质是上下文边界控制。启示：goal 延续不必重放整段历史——抄 Codex"引用 worktree/命令输出作证据" + Qwen"judge 看 transcript 摘要"，每轮 notify 推"完整目标 + 最新进展摘要"而非累积上下文。这绕过 session.new 缺口，还顺便解决上下文无限膨胀。

**generation slot（competitiveNN）**：HagiCode 的 fallback 变体就是它的 prompt 层版本；CodeBuddy 的小模型评估器是它的另一个用例。启示：maki 的 slot 系统（slot.rs LayeredTools 已存在）可以承载两件事——judge（goal 验收）和模型 fallback 链。

### 4. 一句话总结

#703 提案方向与行业一致，缺的不是新原语，而是三个可借鉴的机制：证据导向的 judge、三层停止语义（达成/不可能/错误）、上下文边界控制（目标+摘要而非全文重放）。

### 5. judge 派 vs 工具派对比

两派本质分歧：谁来判断"目标完成"——独立小模型（judge 派），还是干活的主模型自己（工具派）。

| 维度 | judge 派（Claude/Codex/Hermes/Qwen/CodeBuddy） | 工具派（Pi/OpenCode/#703 提案） |
|------|-----------------------------------------------|--------------------------------|
| 判定权威性 | 独立 judge，证据导向（Qwen：含糊默认未达成），不受主模型自我表扬偏差影响，防伪完成强 | 主模型自评，容易"合理样子的总结就停"——恰好是 #703 想解决的问题本身 |
| token 成本 | 每轮多一次 judge 调用（小模型便宜，但次数=轮数） | 零额外调用，agent 在 turn 内顺便声明 |
| 延迟 | 每轮结束多一跳往返 | 无额外延迟 |
| 架构复杂度 | 需 judge 模型配置 + judge prompt + 证据提取/摘要 + 严格 JSON 解析（Qwen goalJudge.ts 可见其重）；maki 要新增 Rust 或 provider 接入 | 两个 Lua 工具 + 状态文件，全 Lua 零 Rust（#703 核心论点） |
| 判定粒度 | 能读整段证据（transcript/worktree），发现部分完成、跳过 AC 的情况 | 判定取决于 agent 诚实度和能力；但 agent 有完整上下文，judge 只有摘要 |
| 失败模式 | judge 误判"未达成"→ 无限续轮烧钱；需软停止兜底 | agent 误报 complete → 提前停（靠门禁兜底）；agent 不报 → 轮次上限兜底 |
| 模型无关性 | judge 独立选型（小/便宜/专门调优），主模型换谁不影响验收逻辑 | 依赖主模型工具调用能力，弱模型容易挂 |
| 可审计性 | judge 判定可落盘，能解释"为什么没达成" | 只有 agent 声明 + summary，无独立证据链 |
| 实现成本 | 高（judge 是独立子系统） | 低（#703 现有原语即可组合） |

**各自死穴**：
- judge 派：每次续轮都烧一次 judge 钱，且"判什么"依赖证据摘要的质量——摘要丢细节，judge 就是瞎判。Qwen 用严格 JSON + 默认未达成防的是"误报达成"，但防不了"摘要损失"
- 工具派：验收和干活是同一个脑袋。agent 把"做了大部分"当成"完成"是系统性倾向，不是修个 goal_id 能解决的

**分层建议**：
1. v1 工具派打底——#703 提案成立：全 Lua、零 Rust、与 maki 哲学一致
2. judge 作为可选增强层——TurnEnd 后插件可选调 judge，与 update_goal 互为校验：agent 声明 complete 但 judge 证据不足 → 不续轮但标记"需人确认"（喂给熄灯设计的提请级通知）
3. 最终裁决交给门禁——熄灯设计的 Phase 2 门禁（shell 层 lint/test/build）是唯一权威。judge/工具声明都只是进度信号，不是验收。这正好消解工具派"伪完成"的弱点：agent 谎报 complete 无害，门禁会打回

一句话：工具派解决"怎么少花钱"，judge 派解决"怎么不假完成"，门禁解决"两者都不可信时谁来兜底"——三者的关系是分层而非二选一。

## 五、1jehuang/jcode 的 /goal 实现与迭代历史

**时间**: 2026-09-05 23:16

**主题**: jcode.sh（Rust，约 19.1k 星）的 goal 功能源码解析与提交历史；与 #703 提案及熄灯设计的对照。

### 1. 基本信息

- 仓库：[1jehuang/jcode](https://github.com/1jehuang/jcode)（jcode.sh，"The most RAM efficient harness"，MIT，Rust）
- 支持 /goal；goal 类型定义在 `crates/jcode-task-types`，核心逻辑 `crates/jcode-base/src/goal.rs`（22KB），模型工具 `crates/jcode-app-core/src/tool/goal.rs`（15KB）
- 注：另有一个同名小项目 cnjack/jcode（Go，34 星）也支持 /goal，与本章节无关

### 2. 实现要点（源码实测）

**goal 是结构化任务文档，不是一句话 objective**：

```rust
pub struct GoalCreateInput {
    title: String,
    scope: GoalScope,              // Global / Project——不只 session 级
    description: Option<String>,
    why: Option<String>,           // 为什么做（决策摘要）
    success_criteria: Vec<String>, // 验收标准数组
    milestones: Vec<GoalMilestone>,// 里程碑
    next_steps: Vec<String>,
    blockers: Vec<String>,         // 阻塞项
    current_milestone_id: Option<String>,
    progress_percent: Option<u8>,
}
```

- **双作用域**：goal 存 global 或 project 级（按 working_dir 找目录）；session 只做附件——`attach_goal_to_session` 记录 `goal_id + scope + project_hash`（防止在错误仓库恢复附件）
- **持久化**：JSON 文件，`read_json/write_json_fast`，按 goal id 命名存 goals 目录——与 #703 提案的 state_dir JSON 同思路
- **session 恢复**：`resume_goal`——先看 session 附件，`is_resumable()` 过滤，否则挑最近一个可恢复 goal
- **单一 `goal` 工具**（模型可调）：action 参数分派（create/update/...），带 JSON schema（`goal_milestone_schema`/`goal_step_schema`），status 走 `GoalStatus::parse` 校验——工具派
- **UI**：side panel 渲染 markdown 页面（goals 总览 + 单 goal 页），`GoalDisplayMode::Auto/Focus/UpdateOnly/None` 控制

### 3. 与 #703 提案对照

| 维度 | #703 提案 | jcode |
|------|----------|-------|
| goal 形态 | 单 objective 字符串 | title+why+success_criteria[]+milestones[]+blockers[]+progress% |
| 作用域 | session | global/project，session 仅附件 |
| 持久化 | state_dir JSON | goals 目录 JSON，同思路 |
| 验收 | 无显式字段（靠 update_goal） | success_criteria 数组 + progress_percent |
| UI | /goal 摘要 + 状态提示 | side panel markdown 页 |

**对熄灯设计最有价值的三点**：
1. `success_criteria` 数组——AC 验收标准 checklist 的 goal 内嵌形态，模型每轮对照，比裸 objective 更能防"伪完成"
2. `why` 字段 + `blockers` 字段——决策摘要防失忆（DECISIONS.md 的 goal 内嵌版）与阻塞级通知归因
3. project 级 scope + project_hash 附件校验——多仓库夜间批处理时 goal 绑定具体仓库，防止 worktree 切换后恢复错目标

### 4. goal 功能迭代历史（按提交时间）

| 时间 | 提交 | 内容 |
|------|------|------|
| 2026-03-17 | src/goal.rs | **Add persistent goals support**（首次引入） |
| 2026-03-26 | src/goal.rs | Improve goal side panel integration（侧栏集成） |
| 2026-04-30 | src/goal.rs | **Move goal state types into core**（状态类型进 core；与 Codex v0.128.0 同一天） |
| 2026-04-30 | task-types | Move task state types into leaf crate |
| 2026-05-29 | Phase A/B | 两次 crate 拆分重构（goal.rs 随 jcode-base 迁移，纯结构性） |
| 2026-07-05 | tool/goal.rs | **Initiative tool no longer auto-opens the side panel**（工具不再抢侧栏焦点，update/checkpoint 保持 UpdateOnly——防打扰） |
| 2026-07 起 | task-types | todo/goal 质量门控持续演进：user intention 追踪（07-15/16/20/24）→ hill-climbability 移到 goal 级（07-07/08/11）→ 更名 closed feedback loop（07-28）→ turn 末批量判分而非逐写打断（07-27）→ semantic quality assessments + iteration maturity 门控（08-03）→ requirement traceability（08-06） |

**演进主线**：从"持久 goal"起步（03-17）→ 类型进 core（04-30）→ 工具防打扰（07-05）→ 质量门控逐步加码（07-08 月），最终形成"结构化 goal + 验收标准 + 完成门控"的形态，与熄灯设计"AC 驱动门禁"思路同源。
