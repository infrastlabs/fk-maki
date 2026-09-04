# 熄灯工程：夜间批处理流水线设计

**日期**: 2026-09-04
**主题**: 多 Issue 封装任务 → 批处理异步队列 → 夜间执行 → 晨会简报；信箱通知、多阶段看板、门控规则与验收标准

## 一、目标与背景

把实时交互转换为批处理：白天人收集/确认 Issue 入队，夜间无人值守执行，早上人只处理汇总结果。目标是把人从"实时调度员"退居"邮件收件人"，处理模式从同步阻塞转为异步非阻塞。

核心收益（修正后的三条，不以成本论）：
1. 批量化摊薄单次运行固定开销（session 冷启动、prompt 前缀缓存）
2. 人退出交互回路后，吞吐不再被人卡住
3. 队列深时一次性调度多任务拉满 API 配额

## 二、Maki 现有能力对照

| 已有积木 | 位置 | 用途 |
|---------|------|------|
| `--print` 无头模式 | `maki-agent/src/headless.rs` | 单 issue 一趟 headless 运行，JSON 输出含 `session_id`/`cost`；SDK 流式协议支持 `max-turns`/`--fork-session` |
| `SessionMailbox` | `maki-agent/src/mailbox.rs` | 会话内通知原语（notify/drain/wake） |
| 会话持久化 | `maki-storage` | 失败回放、简报数据底座 |
| 取消原语 | `maki-agent/src/cancel.rs` | 超限强制终止 |
| 权限配置 | `maki-agent/src/permissions.rs` | 路径作用域规则、插件白名单 |
| OS 通知 | OSC9/bell | 仅限交互场景 |

缺口（本设计覆盖范围）：任务队列、多阶段看板状态机、门控规则、串行合并窗口、通知触达、预算熔断。

## 三、竞品现状调研（2026-09 盘点）

赛道已跑出成品，按可开箱即用程度分三档（对照本设计维度：队列/夜间调度/门禁/Worktree 隔离/通知看板）：

### A 档：商业成品（与整体愿景重合度最高）

| 产品 | 匹配点 | 与设计差距 |
|------|--------|-----------|
| Devin（Cognition） | Scheduled Sessions/Automations（cron 夜间执行）、Managed Devins（拆任务并行，独立 VM）、跑完推送 Slack；"Schedule Devins"可直接组合定时+并行 | 队列/看板隐式（会话列表），无自定义门禁，靠人审 PR；ACU 计量贵 |
| Factory Droids | 官方用法即 cron 夜间跑依赖升级、早上 PR 已开好；分级自主（medium 档暂停危险命令）；CLI/SDK 可被 GitHub Action 调 | 无内置队列排期；多 droid 协同仍在路线图（独立作用域是其架构前置优势） |
| Copilot coding agent | issue 指派给 bot 自开 PR；REST/GraphQL 可编程，配 Actions cron 即可做夜间批量；license 内含 | 无队列/看板/门禁，编排层须自写 |
| Kiro / Kiro Crew（AWS） | kiro-action：打标签即开工、cron 定时、steering file 约束行为；Crew 有工单队列 triage、morning digest、跨 session 恢复、ACP 可观测 | 依赖顺序靠"错开 start date"；绑 AWS agentic 生态 |
| Conductor（background agents） | worktree 隔离、每任务独立分支/终端/diff 审阅流，并行 Codex/Claude sessions | 本地开发流工具，非队列产品，无夜间调度 |

### B 档：GitHub 开源、可当产品用

| 项目 | 说明 | 诚实评估 |
|------|------|---------|
| [Deputies（deputies.dev）](https://deputies.dev) | 后台 coding agent 控制平面：沙箱、持久化工作队列、Postgres+React 前端、Slack/GitHub/webhook/定时触发、子任务续派、PR/artifact、跨 session 历史 | 与本设计架构几乎同图；建在 Pi 上，团队向 |
| [OpenHands](https://github.com/All-Hands-AI/OpenHands) | 成熟开源 agent，`--headless` 跑批处理/CI，`--resume`/`--json` 支持 | 引擎级，无队列无看板，须 cron 自编；headless 强制 always-approve（踩中本设计权限坑） |
| [autoresearch](https://github.com/karpathy/autoresearch)（karpathy 及各 fork） | issue 驱动：首 agent 实现 → 多 agent 轮转审核 → 评分门禁 ≥85 → 自动 PR/合并/关 issue，支持断点续跑 | 能跑通闭环，但定位评估型工具（results.tsv 实验气息）；个人仓库级可用，团队慎用 |
| [code-conductor](https://github.com/ryanmac/code-conductor)（ryanmac） | GitHub Actions 编排并行 Claude Code，worktree 隔离，issue 自动认领→实现→PR→自动合并 | 完成度不错、零冲突设计，但深度依赖 Claude Code 生态，门禁即仓库 CI |

### C 档：明确排除（半成品/框架）

- [multi-agent-software-team](https://github.com/maquekenzhegua/multi-agent-software-team)：任务板+worktree+预算控制，与设计高度相似，但带 stub 模式、单作者实验室气质
- [microsoft/conductor](https://github.com/microsoft/conductor)：workflow 引擎（YAML 定义 DAG），非产品
- `gh-issues` skill：六阶段流水线齐全，但依附宿主 agent，非独立产品

### 调研结论对本设计的影响

1. 零基建当天跑通：Copilot coding agent + Actions cron 即最小闭环；要控制平面（队列/看板/触发）可用 Deputies
2. **门禁自定义 + 串行合并窗口是所有成品都不做的部分**——Devin/Factory/Copilot 均依赖仓库 CI + 人审 PR。这是本设计的差异化空位，也是自建（或做成 Maki 模块）成立的理由
3. 印证"先外部脚本闭环、再内置"路线：B 档项目证明外部编排 maki CLI 的技术路径已被验证

### 4. 第三方推荐清单核验（2026-09）

外部渠道给出 17 项"理念相似"开源项目清单，2026-09-04 逐项对 GitHub API 验真：**全部真实存在**（初轮搜索因星数过低漏检，已按清单地址逐一确认）。星数与清单提供值有出入，以本次 API 查询为准。

| 清单项目 | 仓库 | 星数 | 语言 | 备注 |
|---------|------|------|------|------|
| ai-night-shift | [JudyiLab/ai-night-shift](https://github.com/JudyaiLab/ai-night-shift) | 224 | Shell | 与清单的 219 星略有出入 |
| openclaw-night-shift | [asistent-alex/openclaw-night-shift](https://github.com/asistent-alex/openclaw-night-shift) | 0 | Shell | 0 星，新建项目 |
| cavil-loop | [GigleAI/cavil-loop](https://github.com/GigleAI/cavil-loop) | 10 | Shell | 理念最贴近，demo 级 |
| agtx-sweep（属 agtx） | [fynnfluegge/agtx](https://github.com/fynnfluegge/agtx) | 1472 | Rust | 与清单的 907 星出入大（或为历史快照） |
| beutl-loop（属 beutl） | [b-editor/beutl](https://github.com/b-editor/beutl) | 1225 | C# | 实为视频编辑器，非编码闭环；"beutl-loop"子项目未证实 |
| Kanine | [scottkeckwarren/kanine](https://github.com/scottkeckwarren/kanine) | 0 | PHP | TUI Kanban + GitHub Issue，0 星新建 |
| Ralph | [frankbria/ralph-claude-code](https://github.com/frankbria/ralph-claude-code) | 9619 | Shell | 另有 [subsy/ralph-tui](https://github.com/subsy/ralph-tui) 2432 星 |
| Agent Orchestrator | [bpinhosilva/agent-orchestrator](https://github.com/bpinhosilva/agent-orchestrator) | 5 | TypeScript | 与描述"计划→审批→实施"基本吻合 |
| Emdash | [generalaction/emdash](https://github.com/generalaction/emdash)（另有 [emdash-ai/emdash](https://github.com/emdash-ai/emdash) 0 星镜像） | 5593 | TypeScript | 取星数高的主仓库 |
| overnight-compute | [Infatoshi/overnight-compute](https://github.com/Infatoshi/overnight-compute) | 10 | Python | SQLite 计算租约，吻合 |
| agent-loop | [tonoid/agent-loop](https://github.com/tonoid/agent-loop) | 0 | TypeScript | 多账号调度，与描述吻合 |
| Vigilante | [aliengiraffe/vigilante](https://github.com/aliengiraffe/vigilante) | 39 | Go | sandbox-first 编排层 |
| orchestrator | [kbarendrecht/orchestrator](https://github.com/kbarendrecht/orchestrator) | 1 | Rust | 编排多会话 + PR 管理 |
| gustdeck-cli | npm 包 [gustdeck-cli](https://www.npmjs.com/package/gustdeck-cli) | - | - | 非 GitHub 仓库，npm 包 |
| hive-cli | [ultraviolettes/hive-cli-agent-for-laravel](https://github.com/ultraviolettes/hive-cli-agent-for-laravel) | 1 | PHP | demo 级 |
| ghpm | [jackchuka/ghpm](https://github.com/jackchuka/ghpm) | 20 | 未标注 | GitHub Projects v2 agent skills |
| OrcAI.Tool | [dburriss/orcai](https://github.com/dburriss/orcai) | 1 | F# | 与清单的 C# 不符，实际 F# |

结论：清单 17 项全部属实（1 项为 npm 包）；星数普遍极低（0-20 星为主），能称成品的只有 Ralph（9619 星）与 Emdash（5593 星）；cavil-loop 的"最贴近建议"需打折扣（demo 级）。beutl 为视频编辑器张冠李戴；gustdeck 不在 GitHub。第三节 1-3 条设计结论不受影响。

## 四、两个前提修正（历史论断纠偏）

1. **"夜间省钱 30%-50%、无速率限制"不成立**。LLM 按 token 计费与本地时间无关；限流按 API key + 时间窗口算，昼夜一个配额。个别提供方有 off-peak 价，不能当设计前提。
2. **"确定性回放"不成立**。session 落盘让失败任务能 `--fork-session` 续跑，省上下文重建成本；但 LLM 随机、多数 API 无 seed，回放不等于复现正确结果。门禁必须兜底。

## 五、四相流水线

| 阶段 | 机制 | 成本 |
|------|------|------|
| Phase 0 队列预检 | 入队即时执行：AC 段存在性、依赖完整性、`cargo check` 主干、git 干净；失败即拒收并邮件说明 | 零 token |
| Phase 1 并行编码农场 | 每卡一个 git worktree + 一次独立 headless session + 独立权限策略 + 预算上限；无依赖卡并发 | 唯一大头 |
| Phase 2 门控质量闸门 | shell 层重跑 lint/test/build（零 token），失败自动打回 + 1 次修复，再挂顺延明晚 | 零 token |
| Phase 3 串行合并与回归 | 依赖拓扑排序逐个合入主干，每合一个增量集成测，末尾一次全量回归 | 增量部分少量 |
| 晨会简报 | 一次便宜的 headless run 读看板 + session 尾 + DECISIONS.md 生成摘要 | 极低 |

两条总原则：**门禁跑命令，agent 只负责修复**（门禁可信度来自执行者而非声称者）；**不绿不入队**（主干是红时不启动队列，避免 agent 与预存失败搏斗）。

## 六、深水区设计

### 1. 门控规则

- AC 入队强制声明：markdown checklist，每项为可执行命令或可断言行为
- 门禁分级（`gate.toml` per-repo 默认 + 卡片覆盖）：
  - 阻塞性：`clippy -D warnings`、`cargo check`、受影响 crate 测试。挂 → 打回 + 1 次自动修复，再挂 → 顺延
  - 提醒性：格式、覆盖率、大 diff 警告。不阻塞，仅进简报
- 门禁在**干净 worktree** 上跑，不在 agent 脏目录上跑（防假绿）
- 伪通过防线：冒烟测试门槛 = 合并后主干在全新 checkout 上 build + 受影响 crate 测试过（抓数据库变更类集成问题）

### 2. 队列与调度

- 持久化沿用 maki-storage；卡片字段：id、issue 引用、worktree 路径、deps（显式卡片 id）、area 标签、状态、重试次数、session_id、预算、AC、决策摘要、时间戳
- 依赖 = 显式 deps + area 标签推断（同 area 合并阶段强制串行；编码阶段可并行）
- 并发 N 由 API RPM 配额反推；worktree 即沙箱
- 失败卡 `--fork-session` 续跑；重试上限 1 次（成本护栏）
- 优先级 P0/P1/P2；预算见底时低优先级卡自然留置明晚

### 3. 无人值守权限（最深的水）

- `--yolo`（全放行）与"询问"（无人答）都不可用；需要第三种：**作用域内放行，作用域外升级**
- 放行：worktree 内读写、只读全局、测试/构建命令；拒绝：worktree 外写入、密钥文件（`.env`、私钥默认拒）
- 越界动作 → 不询问，卡片冻结为 `blocked`，worktree + session 原地保留，进明早报告"需决策"（冻结优于拒绝：现场保留，人看一眼即可续）
- Maki 已有 `PermissionsConfig`（路径作用域）+ headless 本就不能提示；缺"越界→冻结"语义，需新模块补
- 插件白名单（`PluginRuleStore`）；MCP 夜间默认关闭

### 4. 通知与看板

- **v1 不设实时寻呼**：凌晨的邮件没人看；收敛为清晨一封摘要邮件 + 仓库内 `kanban.md`
- 摘要三段式（分级内容化，而非独立通知）：
  - 信息级：合并成功的卡
  - 提请级：自动修复过、提醒性门禁未过（要人确认）
  - 阻塞级：权限冻结、冲突无法自动解、预算顺延（明确动作项）
- 看板状态机：`queued → precheck → coding → gated → merging → merged / blocked / failed`，转移条件 = 门控规则；渲染为自动维护的 `kanban.md` 提交进仓库（零 UI 工作量；TUI 看板页是后续增强）
- 决策摘要防失忆：离开 `gated` 前必须存在非空 `DECISIONS.md`（做了什么/为什么/否掉什么）；存在性检查阻塞、内容质量提醒

### 5. 成本与熔断

- 两层预算：每卡 `max_turns`（headless 原生支持）+ 夜间总钱包（共享计数）；每卡硬预算按 AC 行数估算 + 兜底上限
- 超限用 `CancelToken` 强制终止（已存在）
- 429 → 指数退避，退避是等不是重试
- 回归成本：全量一晚一次（Phase 3 末尾），每合一个跑增量；测试分快慢两层，慢（集成）只在合并阶段碰
- 坏的黎明比坏的卡贵：门禁挂了默认顺延而非无限重试

## 七、待拍板决策（未定）

1. **送达策略**：v1 清晨一封摘要（默认）；若存在"上线阻断 fix"热路径，加"仅阻塞级实时 OS 通知"开关
2. **实现形态**：外部脚本编排 maki CLI（最快 Demo，验证闭环）/ 内置 Rust 模块（复用 spawn + storage，最终形态）/ 纯 Lua 插件。推荐先外部闭环、再内置

## 八、后续步骤

1. 定稿本文档
2. 写实现计划（Demo 闭环：入队 → 编码 → 门控 → 摘要）
3. 验证后再决定内置设计