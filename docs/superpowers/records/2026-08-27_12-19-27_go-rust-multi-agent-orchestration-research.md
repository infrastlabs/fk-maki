# Go/Rust 多智能体编排（智能体控制智能体）项目调研

> 更新时间：2026-08-27_12-19-27

## 一、问题原文（用户描述）

> 基于"用智能体控制智能体"的技术逻辑（任务分解、任务分配、执行监控、动态仲裁四大机制），挖掘 GitHub 看有哪些 Go/Rust 的项目实现。各给 5 个项目，获取 star/fork/commits/committers/issue-PR 数据信息，分析技术特色做对比。

## 二、四大机制定义（对照组）

"用智能体控制智能体"的本质，是把"全能神"的问题拆解成"管理层 + 执行层"的组织协作问题。技术层通过以下 4 个核心机制实现：

**机制1：任务分解（将目标变为计划）**
控制层 Agent 收到模糊指令（如"做个电商网站"），通过规划链（Plan-and-Solve）或思维树（Tree-of-Thoughts）拆解成可执行子任务（需求分析、前端、后端等）。高级方案（如 Open Multi-Agent）动态生成 DAG（有向无环图），明确任务间依赖与并行关系。

**机制2：任务分配（匹配最合适的"手"）**
拆解后，控制层依据注册表中各执行 Agent 的能力、负载和成本进行路由分发。为增加灵活性，支持动态孵化——当现有 Agent 无法胜任时，控制层通过代码实时生成新的微 Agent（如 spawn_tracked_worker 工具）。

**机制3：执行监控与状态同步（确保闭环）**
控制层通过共享内存或消息总线接收执行层的进度回传。针对长时任务，用检查点（Checkpoint）记录中间状态，支持断点续跑；同时监控 Token 消耗和轮次，防止死循环。

**机制4：动态仲裁与冲突解决（管理的价值）**
当执行 Agent 给出矛盾结论（如一个说用 React、一个说用 Vue）或子任务相互依赖时，控制层充当仲裁者，综合评估后修改 DAG 或调整执行策略，让整体行动收敛到最优解。

补充：**动态协商（Negotiation）**——控制层只定目标，执行层实时商量，灵活但结果难预测（如 OpenRath 的会话接力）。

## 三、背景事实（2026-08 时间线）

- Python 的 CrewAI/LangGraph 生态仍占主导。
- **Rust 侧**：主流是"编码智能体 + 控制面 + 通用框架"三块，专门的编排框架（swarms-rs、cloudllm）普遍处于早期（几十~两百 star）。
- **Go 侧**：更成熟，出现大型"编码智能体编排器"（star 上万）和企业级编排框架（trpc、MAF）。注：**MAF = Microsoft Agent Framework（微软智能体框架）**，即 `microsoft/agent-framework-go`，开源的跨语言（.NET/Python/Go）生产级多智能体工作流编排框架，支持图工作流、检查点续跑、可观测性、人工介入，集成 MCP/A2A/AG-UI 等协议。

## 四、数据总表（GitHub API 实时抓取）

### 1. Rust（5 个）

| 项目 | Stars | Forks | Committers | Commits | Open Issues | 定位 |
|---|---|---|---|---|---|---|
| Hmbown/CodeWhale | 40,869 | 3,538 | 214 | 8,239 | 120 | 终端编码智能体（Rust） |
| 0xPlaygrounds/rig | 8,416 | 938 | 230 | 1,400 | 113 | LLM 应用框架 |
| Nasiko-Labs/nasiko | 5,338 | 1,228 | 39 | 795 | 17 | A2A 智能体控制面 |
| The-Swarm-Corporation/swarms-rs | 177 | 54 | 11 | 早期 | 0 | 企业级 swarm 编排 |
| CloudLLM-ai/cloudllm | 32 | 3 | 1 | 早期 | 0 | 多智能体编排工具包 |

### 2. Go（5 个）

| 项目 | Stars | Forks | Committers | Commits | Open Issues | 定位 |
|---|---|---|---|---|---|---|
| Untrivial-ai/agent-orchestrator | 9,969 | 1,420 | 92 | 2,435 | 834 | 编码智能体编排器（Kanban 监督） |
| nextlevelbuilder/goclaw | 3,563 | 1,039 | 104 | 2,044 | 278 | OpenClaw 的 Go 重建（多租户） |
| trpc-group/trpc-agent-go | 1,735 | 298 | 57 | 1,970 | 108 | 图工作流框架（LangGraph 等价物） |
| go-kratos/blades | 810 | 102 | 17 | 167 | 28 | 多模态 Agent 框架 |
| microsoft/agent-framework-go | 541 | 48 | 多 | 706 | 76 | 微软 Agent Framework Go 实现 |

## 五、四大机制对照分析

### 1. 机制1：任务分解（目标→计划）

- **trpc-agent-go(go)**：`Planner` 独立组件，Agent→Runner→Planner→Tools 分层；`GraphAgent` 支持条件路由、子图。
- **CloudLLM(rust)**：Ralph 模式用 PRD 任务清单做迭代式分解，带 `[TASK_COMPLETE]` 标记回传。
- **agent-orchestrator(go)**：把大目标拆给"每个任务一个 agent + 独立 workspace"，项目级 orchestrator 规划。
- **goclaw(go) / swarms-rs(rust) / rig(rust)**：通过 ReAct/计划循环做即时分解，rig 提供 `multi-agent` orchestrator 模式。
- **nasiko(rust)**：不做分解本身，而是把已拆好的 A2A 智能体做路由与控制面。

### 2. 机制2：任务分配（路由与动态孵化）

- **nasiko(rust)**：核心就是 deploy/route/secure/observe A2A 智能体，按能力路由。
- **CodeWhale(rust)**：一个 leader agent 协调多个子 agent（coordinate agents），可动态 spawn。
- **goclaw(go)**：多租户下按任务隔离分配 agent；多 provider 路由。
- **agent-orchestrator(go)**：为每个 worker 分配独立 workspace，防分支冲突。
- **trpc-agent-go(go)**：Chain / Parallel / Cycle 三种组合式分配，可嵌套（Lego 式）。
- **CloudLLM(rust)**：AnthropicAgentTeams 采用去中心化认领（agent 自己从任务池 claim），而非集中分配。

### 3. 机制3：执行监控与状态同步（闭环）

- **agent-orchestrator(go)**：最突出。实时 Kanban 跟踪每个 worker、PR、CI、review 状态。
- **MAF(go) / trpc-agent-go(go)**：checkpointing + restartability + OpenTelemetry 观测；trpc 有 session/artifact 版本管理。
- **nasiko(rust)**：A2A 智能体的观测与安全（observe/secure）内建。
- **goclaw(go)**：会话持久化、模型熔断（circuit breaker）+ 限流。
- **CodeWhale(rust)**：durable `/goal`、session 保存、`/undo`、快照恢复。

### 4. 机制4：动态仲裁与冲突解决

- **CloudLLM(rust)**：7 种协作模式——Moderated（主持人综合）、Debate（争论至收敛）、Hierarchical（leader 协调）、Parallel 聚合——最完整覆盖"仲裁"语义。
- **swarms-rs(rust)**：面向企业 swarm 的收敛/协作原语。
- **trpc-agent-go(go)**：图条件路由 + 状态 reducer 做分支决策。
- **agent-orchestrator(go)**：review + approve/reject 的人工仲裁闭环。
- **goclaw(go)**：leader-follower / consensus 协作模式。

## 六、关键结论与对比

**1. 控制模式的两种路线**
- **集中式仲裁**（Go 侧主导）：agent-orchestrator 的 Kanban 监督、trpc/MAF 的图引擎，控制层掌握全局 DAG，闭环可预测。
- **去中心化协商**（Rust 侧偏多）：CloudLLM 的 AgentTeams 认领制、swarms-rs 的 swarm 自组织，灵活但结果难预测。

**2. Go vs Rust 定位差异**
- **Go**：编排"更产品化、更重管理"。大型项目聚焦"管一群编码 agent"（多租户、Kanban、CI/PR 闭环），企业框架（trpc/MAF）主打图工作流 + 检查点 + 可观测，工程完备度高。
- **Rust**：编排"更底层、更嵌入式"。要么是高性能编码 agent（CodeWhale）或控制面（nasiko），要么是早期的原生编排库（swarms-rs、cloudllm）。性能收益（零 GIL、并发）明显，但专门编排框架生态尚未成熟（多停留在 0~200 star）。

**3. 选型建议**
- 需要生产级、可回滚、可观测的图编排 → trpc-agent-go / MAF。
- 需要管理一群编码 agent 的 IDE/调度层 → agent-orchestrator（Go）/ CodeWhale（Rust）。
- 想研究仲裁/协商机制本身 → CloudLLM（Rust，7 种模式最全）、swarms-rs。
- 想要跨智能体协议（A2A/MCP）路由控制面 → nasiko。

## 七、附录：仓库地址与数据汇总

### 1. Rust

- https://github.com/Hmbown/CodeWhale · star 40869 / fork 3538 / issue 120 / commits 8239 / committers 214
- https://github.com/0xPlaygrounds/rig · star 8416 / fork 938 / issue 113 / commits 1400 / committers 230
- https://github.com/Nasiko-Labs/nasiko · star 5338 / fork 1228 / issue 17 / commits 795 / committers 39
- https://github.com/The-Swarm-Corporation/swarms-rs · star 177 / fork 54 / issue 0 / commits 早期 / committers 11
- https://github.com/CloudLLM-ai/cloudllm · star 32 / fork 3 / issue 0 / commits 早期 / committers 1

### 2. Go

- https://github.com/Untrivial-ai/agent-orchestrator · star 9969 / fork 1420 / issue 834 / commits 2435 / committers 92
- https://github.com/nextlevelbuilder/goclaw · star 3563 / fork 1039 / issue 278 / commits 2044 / committers 104
- https://github.com/trpc-group/trpc-agent-go · star 1735 / fork 298 / issue 108 / commits 1970 / committers 57
- https://github.com/go-kratos/blades · star 810 / fork 102 / issue 28 / commits 167 / committers 17
- https://github.com/microsoft/agent-framework-go · star 541 / fork 48 / issue 76 / commits 706 / committers 多

## 八、框架生态：rig / trpc-agent-go 之上的项目

### 1. 基于 rig（Rust）的成熟开源项目

rig 生态明显更成熟，官方 README 有 "Who is using Rig" 专列。开源且较成熟的有：

| 项目 | Stars | Forks | 定位 | 用 rig 做什么 |
|---|---|---|---|---|
| nearai/ironclaw | 12,607 | 1,488 | Agent OS（类 OpenClaw 的 Rust 重写） | 重构后改用 rig adapter 做 LLM 路由/RetryProvider 组合 |
| sopaco/deepwiki-rs | 1,704 | 191 | 代码→技术文档生成 | 用 rig 做 LLM 调用与结构化输出 |
| piotrostr/listen | 1,085 | 164 | AI 投资组合管理智能体框架 | 用 rig 构建 agentic 层 |
| vinhnx/VTCode | 824 | 80 | 终端编码智能体 | 用 rig 简化 LLM 调用 + 模型选择器 |
| M4n5ter/rigs | 17 | 4 | 基于 rig 的编排框架 | 早期，直接 build on rig-core |

> 注：Swiftide 早期用过 rig，现已迁到 async-openai，不算。其余多为商业闭源（St Jude、Dria、Neon、Nethermind、Coral 等）。

### 2. 基于 trpc-agent-go（Go）的项目

成熟的开源项目几乎没有，其主要生产用户是腾讯内部闭源业务：腾讯元宝（Yuanbao）、腾讯视频、腾讯新闻、IMA、QQ 音乐——这是官方 README 明确致谢的"生产环境验证方"。开源侧能找到的都很早期：

| 项目 | Stars | 定位 |
|---|---|---|
| liuzengh/trpc-agent-service | 8 | 多租户节点化 Agent 部署平台 |
| hurricane1988/kube-agents | 4 | AI Kubernetes 运维助手 |
| Skylm808/CR-trpc-agent-go | 1 | 自动代码评审 Agent |
| XnLemon/trpc-agent-service | 2 | 多租户 Agent 平台（教程衍生） |

另有配套的兄弟框架 trpc-a2a-go、trpc-mcp-go（不算"基于"它，是并列的生态）。

### 3. 生态对比结论

- **rig：开源生态已经长出来**，有 IronClaw(12.6k)、deepwiki-rs(1.7k)、Listen(1k)、VTCode(0.8k) 这些有真实用户量的项目，属于"框架 + 生态"双成熟。
- **trpc-agent-go：框架成熟但生态闭源**。价值验证在腾讯内部（元宝等 5+ 业务），开源侧依赖者多为 demo 和小项目（star <10），尚未形成第三方开源生态。选型信号：自搭开源方案可借力 rig 生态；看重生产背书则 trpc 有腾讯内部大规模验证。

## 九、VTCode 四大机制覆盖情况

基于 VTCode 官方文档（loop-engineering、planning-workflow、full-automation、safety 等）逐条对照：

| 机制 | 覆盖度 | 具体实现 |
|---|---|---|
| 1. 任务分解 | 中 | 有 `/plan` + `plan` 主 agent 迭代生成构建计划，经 review gate 交给 `build`/`auto`；`--full-auto` 内置 plan-build-evaluate 流程。但属线性计划迭代，无动态 DAG（不生成依赖/并行图） |
| 2. 任务分配 | 中 | 有 delegated subagents（propose/verify 分离）+ worktree 隔离实现多 agent 并行；但无"能力注册表 + 负载/成本"路由，分配靠固定角色交接而非动态匹配 |
| 3. 执行监控 | 强 | durable loop state（持久化循环状态，类 checkpoint）+ session resume（断点续跑）+ cost guardrails（成本/Token 护栏）+ lifecycle hooks + audit logging，最贴合"闭环"要求 |
| 4. 动态仲裁 | 弱-中 | 有 structured review gate 和 propose/verify 机制（提议 vs 验证分离）做质量仲裁，`vtcode review` 评审未提交改动；但属静态流程化仲裁，不是"综合矛盾结论后动态修改计划/策略"的仲裁者 |
| 动态协商 | 弱 | 支持 A2A / Open Responses / ATIF 协议，具备跨 agent 通信基础；但本身不以会话接力式协商为核心 |

**结论**：VTCode 覆盖最扎实的是机制3（执行监控与状态同步）——durable loop state + session resume + cost guardrails 三件套正好对应"检查点、断点续跑、Token 监控"；机制1/2 有雏形（plan agent、subagent、worktree 并行）；机制4 动态仲裁是明显短板，用"review gate 人工/静态把关"代替了"控制层动态裁决"，更接近"流程管控"而非"智能仲裁"。VTCode 属于单 agent 深度自动化里的控制闭环型（机制3 强），而非"管理层+执行层"的多智能体协作型。

## 十、成熟产品/平台/智能体工具调研（不限语言，排除框架）

围绕四大机制，重新搜索 GitHub（不限实现语言），选取**成熟可用的产品/平台/智能体工具**（排除框架/SDK/库）。

### 1. 入选清单

| 项目 | 语言 | ★ / fork / issue | commits / committers | 类型 |
|---|---|---|---|---|
| Hmbown/CodeWhale | Rust | 40,869 / 3,538 / 120 | 8,239 / 214 | 终端编码智能体 |
| zeroclaw-labs/zeroclaw | Rust | 32,661 / 4,915 / 811 | 4,971 / 423 | 个人 AI 助理基础设施 |
| herdrdev/herdr | Rust | 32,720 / 2,365 / 237 | 1,472 / 79 | 编码智能体运行环境 |
| superset-sh/superset | TypeScript | 13,404 / 1,222 / 568 | 3,886 / 103 | Agentic IDE（100+ 并行编码智能体） |
| iflytek/astron-agent | 多语言 | 8,911 / 863 / 41 | 3,214 / 84 | 企业级 Agentic 工作流平台 |
| YaoApp/yao | Go/TS | 7,814 / 696 / 5 | 4,230 / 12 | 智能体+工作区调度台 |
| builderz-labs/mission-control | TypeScript | 6,113 / 61 / 18 | 536 / 53 | 智能体控制平面（纯管理层产品） |
| AgentsMesh/AgentsMesh | Rust | 2,329 / 241 / 20 | 1,106 / 14 | AI Agent 用工平台（控制面/数据面分离） |

仓库地址与数据汇总（与第七节附录同格式）：

- https://github.com/Hmbown/CodeWhale · star 40869 / fork 3538 / issue 120 / commits 8239 / committers 214 · 终端编码智能体（Rust）
- https://github.com/zeroclaw-labs/zeroclaw · star 32661 / fork 4915 / issue 811 / commits 4971 / committers 423 · 个人 AI 助理基础设施（Rust）
- https://github.com/herdrdev/herdr · star 32720 / fork 2365 / issue 237 / commits 1472 / committers 79 · 编码智能体运行环境（Rust）
- https://github.com/superset-sh/superset · star 13404 / fork 1222 / issue 568 / commits 3886 / committers 103 · Agentic IDE（100+ 并行编码智能体）（TypeScript）
- https://github.com/iflytek/astron-agent · star 8911 / fork 863 / issue 41 / commits 3214 / committers 84 · 企业级 Agentic 工作流平台（多语言）
- https://github.com/YaoApp/yao · star 7814 / fork 696 / issue 5 / commits 4230 / committers 12 · 智能体+工作区调度台（Go/TS）
- https://github.com/builderz-labs/mission-control · star 6113 / fork 61 / issue 18 / commits 536 / committers 53 · 智能体控制平面（TypeScript）
- https://github.com/AgentsMesh/AgentsMesh · star 2329 / fork 241 / issue 20 / commits 1106 / committers 14 · AI Agent 用工平台（Rust）

### 2. 四大机制匹配矩阵

| 项目 | 1 任务分解 | 2 任务分配 | 3 执行监控 | 4 动态仲裁 | 综合 |
|---|---|---|---|---|---|
| mission-control | 弱（明确不替代 agent 的规划，只管理任务流转） | 强：任务收件箱+指派+执行队列，agent 认领（/api/tasks/queue） | 强：实时活动流、日志、token/成本视图、会话/在场状态 | 强：Aegis 质量门、评审、审批、审计、无头收据 | 管理闭环最完整 |
| AgentsMesh | 中：Autopilot 控制 agent 看护并下发下一步指令（迭代上限） | 强：Runner 集群调度（max_concurrent_pods）、动态 spawn pod、gRPC+mTLS | 强：统一控制台多屏实时终端流、自愈防卡死 | 中-强：人工接管/交还、决策历史、Mesh 协作拓扑 | 控制面/数据面架构最规范 |
| herdr | 弱：规划在 agent 内部 | 强：agent 可动态 spawn pane、互相 prompt | 强：会话跨重启恢复、持久运行 | 中：agent 间互相等待/协作 | 运行环境类标杆 |
| superset | 中：plan review 工作流 | 强：并行分发到隔离 worktree | 强：所有 agent 一个面板监控+完成提醒 | 中-强：plan 评审+工具审批 | 编码智能体 IDE 标杆 |
| astron-agent | 强：企业级工作流编排（图+DAG+模型/工具编排） | 强：模型管理+AI/MCP 工具调度+RPA 执行 | 强：HA 部署+CNCF 可观测性 | 中：审批/协作（企业级治理） | 企业平台类最全面 |
| zeroclaw | 中：自主任务规划 | 中：多 agent 编排 | 中-强：持久状态/记忆 | 中：权限审批 | 个人助理类 |
| CodeWhale | 中：/goal 目标规划 | 中：leader 协调子 agent | 强：durable goal、session、undo/快照 | 中：review 门禁 | 单产品深度强 |
| yao | 弱-中 | 中：任务看板分配 | 中：看板跟踪 | 弱-中 | 轻量调度台 |

### 3. 关键结论

**1. 真正"管理层+执行层"结构化的产品有 3 个**：
- **mission-control**（控制平面产品）：唯一把"管理层"本身做成产品卖点——只管分配/监控/评审/审计/花费，明确不管规划；
- **AgentsMesh**（Agent 用工平台）：控制面/数据面分离，Runner 集群做执行层，控制台做管理层，Autopilot 做自主看护；
- **astron-agent**（企业平台）：把分解（工作流编排）、分配（模型/工具/RPA 调度）、监控（HA+可观测）全做进企业平台。

**2. 动态仲裁基本都是"人工/审批门禁"而非"LLM 仲裁者"**：mission-control 的 Aegis 质量门、superset 的 plan review+工具审批、AgentsMesh 的人工接管。目前没有产品把"矛盾结论→LLM 综合裁决→改 DAG"做成产品化能力——这正是可切入的差异化点。

**3. 成熟度排序**（按社区+代码活跃）：CodeWhale/zeroclaw/herdr/superset 属高成熟；astron-agent/yao/AgentsMesh/mission-control 次之但架构针对性强。

## 十一、8 类综合类别的 Top5（各补 4 款）

按第十节矩阵中各产品的"综合"类别，对 GitHub 重新搜索（不限语言、排除框架），每类取匹配度前 5（第 1 名为原矩阵产品，另补 4 款）。

### 1. 控制平面（管理闭环）｜第1名 mission-control

| 排名 | 项目 | 语言 | ★ / fork / issue | 四大机制匹配要点 |
|---|---|---|---|---|
| 1 | [builderz-labs/mission-control](https://github.com/builderz-labs/mission-control) | TS | 6,113 / 61 / 18 | 任务收件箱/指派/队列、Aegis 质量门、审批/审计、token/成本视图 |
| 2 | [musistudio/claude-code-router](https://github.com/musistudio/claude-code-router) | TS | 36,903 / 3,096 / 1,098 | 本地控制面，跨 provider 路由分发（机制2 极强） |
| 3 | [superplanehq/superplane](https://github.com/superplanehq/superplane) | — | 5,551 / 608 / 510 | agentic engineering 控制面，任务编排+观测 |
| 4 | [huangruiteng/loopx](https://github.com/huangruiteng/loopx) | — | 5,213 / 465 / 53 | 长时程控制面，持久检查点/断点续跑/治理（即文中 LoopX） |
| 5 | [Nasiko-Labs/nasiko](https://github.com/Nasiko-Labs/nasiko) | Rust | 5,338 / 1,228 / 17 | 开发者控制面，A2A 路由/观测/安全（与八节重复） |

### 2. Agent 用工平台（控制面/数据面）｜第1名 AgentsMesh

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [AgentsMesh/AgentsMesh](https://github.com/AgentsMesh/AgentsMesh) | Rust | 2,329 / 241 / 20 | Runner 集群调度、spawn pod、控制台、Autopilot 看护 |
| 2 | [MervinPraison/PraisonAI](https://github.com/MervinPraison/PraisonAI) | Python | 8,967 / 1,425 / 59 | "24/7 AI Workforce"，no-code+agent 编排 |
| 3 | [a5c-ai/babysitter](https://github.com/a5c-ai/babysitter) | — | 1,738 / 102 / 302 | 对 agentic workforce 的监督与纪律执行（仲裁向） |
| 4 | [Anas-Khan93/ai-agency-agents](https://github.com/Anas-Khan93/ai-agency-agents) | — | 511 / 482 / 0 | 开源自营 AI workforce（专家 agent） |
| 5 | [saltbo/agent-kanban](https://github.com/saltbo/agent-kanban) | — | 455 / 39 / 22 | agent-first 任务看板（轻量 mission control） |

### 3. 运行环境（agent runtime）｜第1名 herdr

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [herdrdev/herdr](https://github.com/herdrdev/herdr) | Rust | 32,720 / 2,365 / 237 | spawn pane、会话跨重启恢复、持久运行 |
| 2 | [KunAgent/Kun](https://github.com/KunAgent/Kun) | — | 6,258 / 593 / 10 | local-first agent 工作区（编码/写作/设计） |
| 3 | [Q00/ouroboros](https://github.com/Q00/ouroboros) | — | 5,707 / 570 / 60 | Agent OS，自我进化 |
| 4 | [GCWing/BitFun](https://github.com/GCWing/BitFun) | — | 1,820 / 202 / 123 | 高性能 agent runtime |
| 5 | [poco-ai/poco-claw](https://github.com/poco-ai/poco-claw) | — | 1,351 / 127 / 1 | OpenClaw 美化替代（运行环境向） |

> 注：jayminwest/overstory（多 agent 编排 runtime）已归档，排除。

### 4. Agentic IDE（编码智能体 IDE）｜第1名 superset

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [superset-sh/superset](https://github.com/superset-sh/superset) | TS | 13,404 / 1,222 / 568 | 100+ 并行编码 agent、worktree、监控、plan review |
| 2 | [cline/cline](https://github.com/cline/cline) | TS | 66,952 / 7,233 / 1,118 | 自主编码 agent（SDK/IDE 扩展/CLI） |
| 3 | [can1357/oh-my-pi](https://github.com/can1357/oh-my-pi) | — | 27,762 / 2,750 / 1,810 | 编码 agent + IDE 接线 |
| 4 | [Untrivial-ai/agent-orchestrator](https://github.com/Untrivial-ai/agent-orchestrator) | Go | 9,969 / 1,420 / 834 | Agent IDE 管理 agent 舰队（与四节重复） |
| 5 | [stagewise-io/stagewise](https://github.com/stagewise-io/stagewise) | TS | 6,789 / 508 / 17 | 开源模型 agentic IDE |

### 5. 企业平台｜第1名 astron-agent

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [iflytek/astron-agent](https://github.com/iflytek/astron-agent) | 多语言 | 8,911 / 863 / 41 | 工作流编排+模型/工具/RPA 调度+HA+可观测 |
| 2 | [ToolJet/ToolJet](https://github.com/ToolJet/ToolJet) | JS/TS | 40,777 / 5,418 / 1,168 | 开源 low-code 基础（ToolJet AI） |
| 3 | [1Panel-dev/MaxKB](https://github.com/1Panel-dev/MaxKB) | Python | 22,625 / 3,121 / 29 | 企业知识库 agent 平台 |
| 4 | [arc53/DocsGPT](https://github.com/arc53/DocsGPT) | Python | 18,230 / 2,135 / 105 | 私有 AI 平台（agent/assistant/enterprise） |
| 5 | [dataelement/bisheng](https://github.com/dataelement/bisheng) | Python | 11,913 / 1,953 / 129 | 开放 LLM devops 平台 |

### 6. 个人 AI 助理｜第1名 zeroclaw

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [zeroclaw-labs/zeroclaw](https://github.com/zeroclaw-labs/zeroclaw) | Rust | 32,661 / 4,915 / 811 | 自持 AI 助理基础设施（自主规划/记忆） |
| 2 | [openclaw/openclaw](https://github.com/openclaw/openclaw) | TS | 387,755 / 81,420 / 5,666 | 个人 AI 助理巨无霸（Any OS） |
| 3 | [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) | — | 237,050 / 47,957 / 36,349 | "agent that grows with you" |
| 4 | [nearai/ironclaw](https://github.com/nearai/ironclaw) | Rust | 12,607 / 1,488 / — | Agent OS（与八节重复） |
| 5 | [poco-ai/poco-claw](https://github.com/poco-ai/poco-claw) | — | 1,351 / 127 / 1 | OpenClaw 替代 |

### 7. 编码智能体（产品）｜第1名 CodeWhale

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [Hmbown/CodeWhale](https://github.com/Hmbown/CodeWhale) | Rust | 40,869 / 3,538 / 120 | durable goal、session、undo/快照、leader 协调 |
| 2 | [anomalyco/opencode](https://github.com/anomalyco/opencode) | Rust | 201,817 / 26,188 / 5,539 | 开源编码智能体 |
| 3 | [github/copilot-cli](https://github.com/github/copilot-cli) | Go | 11,123 / 1,911 / 2,223 | GitHub Copilot CLI |
| 4 | [aannoo/hcom](https://github.com/aannoo/hcom) | — | 463 / 66 / 32 | agents message/watch/spawn each other |
| 5 | [dcouple/Pane](https://github.com/dcouple/Pane) | — | 415 / 200 / 74 | terminal-first agent manager |

### 8. 轻量调度台/工作区看板｜第1名 yao

| 排名 | 项目 | 语言 | ★ / fork / issue | 匹配要点 |
|---|---|---|---|---|
| 1 | [YaoApp/yao](https://github.com/YaoApp/yao) | Go/TS | 7,814 / 696 / 5 | 全设备 agent 工作区+任务看板 |
| 2 | [daggerhashimoto/openclaw-nerve](https://github.com/daggerhashimoto/openclaw-nerve) | — | 867 / 145 / 40 | OpenClaw 实时 web cockpit |
| 3 | [saltbo/agent-kanban](https://github.com/saltbo/agent-kanban) | — | 455 / 39 / 22 | agent-first 看板（与类别2重复） |
| 4 | [BarraDev/slashit](https://github.com/BarraDev/slashit) | — | 2 / 1 / — | AI coding agents mission control（Kanban/pay） |
| 5 | [anotherplnt/agent-ledger](https://github.com/anotherplnt/agent-ledger) | — | 2 / 0 / — | SQLite 认领板（多 agent 认领任务） |

> 注：类别8 除 yao 外整体规模很小/早期，成熟产品稀少。

### 9. 跨类别观察

- 真正同时命中机制 2/3/4 的成熟产品集中在**类别 1（控制平面）和类别 2（用工平台）**；
- **个人助理类（类别 6）体量最大但四大机制匹配一般**（openclaw/hermes 都是"单助理"而非"管理层+执行层"）；
- 动态仲裁依旧是全行业空白。

## 十二、Yao 同类项目推荐

基于 Yao 的特征（多智能体工作区 + 任务看板 + 自托管 + 跨设备管理"一堆 agent"），核验过的同类项目，按相似度分三档。

### 1. 一档：最贴近 yao（看板式多 agent 调度工作区）

| 项目 | 语言 | ★ / fork / issue | 说明 |
|---|---|---|---|
| [777genius/agent-teams-ai](https://github.com/777genius/agent-teams-ai) | TypeScript | 1,989 / 336 / 26 | "你是老板，agents 是你的团队"，任务看板分配——最贴合"管理层+执行层+看板"，是 yao 最直接的同类 |
| [Paca-AI/paca](https://github.com/Paca-AI/paca) | Go | 1,759 / 143 / 10 | AI 原生的 Jira/Trello/ClickUp 替代，把 agent 任务当工单管（卡片/看板/流程） |
| [saltbo/agent-kanban](https://github.com/saltbo/agent-kanban) | TypeScript | 455 / 39 / 22 | agent-first 任务看板，自称"AI workforce 的 mission control"，轻量 |
| [Mng-dev-ai/agentrove](https://github.com/Mng-dev-ai/agentrove) | TypeScript | 316 / 61 / 0 | 自托管 AI 编码工作区，可运行并编排 Claude Code 等 agent |
| [Peiiii/nextclaw](https://github.com/Peiiii/nextclaw) | TypeScript | 253 / 43 / 6 | 自托管、可扩展的 agent 工作区（OpenClaw 类替代），跨设备 |

### 2. 二档：偏"一屏监控"（yao 的监控面，但非全任务编排）

| 项目 | 语言 | ★ / fork / issue | 说明 |
|---|---|---|---|
| [hoangsonww/Claude-Code-Agent-Monitor](https://github.com/hoangsonww/Claude-Code-Agent-Monitor) | TypeScript | 942 / 219 / 43 | Claude Code & Codex 实时监控仪表盘（看运行态，不做任务分配） |
| [daggerhashimoto/openclaw-nerve](https://github.com/daggerhashimoto/openclaw-nerve) | TypeScript | 867 / 145 / 40 | OpenClaw 实时 web cockpit（语音对话 + agent 状态） |
| [hallucinogen/agent-viewer](https://github.com/hallucinogen/agent-viewer) | HTML | 394 / 52 / 1 | tmux 中管理 Claude Code agents 的看板（终端场景） |

### 3. 三档：团队/上下文协作向

| 项目 | 语言 | ★ / fork / issue | 说明 |
|---|---|---|---|
| [kanwas-ai/kanwas](https://github.com/kanwas-ai/kanwas) | TypeScript | 744 / 101 / 7 | 团队 + agent 的共享上下文看板（偏协作，非任务调度） |
| [Agenta-AI/agenta](https://github.com/Agenta-AI/agenta) | TypeScript | 4,556 / 643 / 299 | 团队构建 agent 的工作区（更偏开发/评测平台，非运行时调度） |

### 4. 选型提示

- 要最像 yao 的"看板式多 agent 调度"：agent-teams-ai、paca、agent-kanban。
- 要自托管工作区 + 编排 Claude Code/Codex：agentrove、nextclaw。
- 要监控仪表盘（yao 弱项，可互补）：Claude-Code-Agent-Monitor、openclaw-nerve。

## 十三、Maki 集成能力分析：哪些平台可接入 Maki

基于 Maki 项目源码（`maki-acp/src/lib.rs`、`maki-agent/src/headless.rs`、`src/cli.rs`），分析其对外暴露的集成接口，以及前述调研平台中哪些可以接入 Maki 作为执行层智能体。

### 1. Maki 的集成接口

| 接口 | 命令 | 源码位置 | 说明 |
|---|---|---|---|
| Headless（CLI） | `maki "prompt" --print` | `maki-agent/src/headless.rs` | stdin/stdout，支持 text/json/stream-json 输出 |
| Claude Code 兼容 | `maki "prompt" --print` | `site/docs/content/headless/_index.md` | 官方文档明确：`--print` 是 Claude Code 的 drop-in 替换 |
| ACP（Agent Client Protocol） | `maki acp` | `maki-acp/src/lib.rs` `maki-acp/src/server.rs` | ndjson stdio 服务器，Zed 等 ACP 编辑器可直接驱动 |
| MCP（客户端） | 配置集成 | `maki-agent/src/mcp/mod.rs` | 作为 MCP 客户端连接外部工具服务器（stdio/http/oauth），但不暴露为 MCP server |

### 2. 可接入 Maki 的平台

| 平台 | 集成方式 | 难度 | 说明 |
|---|---|---|---|
| HexSleeves/waggle | Exec 适配器 | 最低 | 直接用 `bash -c "maki 'task' --print"`，已有 Exec adapter |
| superset-sh/superset | CLI agent | 低 | "any CLI agent" 可接入，每条指令 `maki --print` |
| builderz-labs/mission-control | Claude Code 适配器 | 低 | 已有 Claude Code adapter，Maki 的 `--print` 是 drop-in 替换 |
| AgentsMesh/AgentsMesh | Pod CLI agent | 低 | Runner 集群 spawn pod，pod 内跑 `maki --print` |
| herdrdev/herdr | CLI / socket | 低 | agent 通过 CLI socket 驱动，Maki 可注册为子 agent |
| Untrivial-ai/agent-orchestrator | CLI agent | 低 | 每个 worker 一个 Maki 进程 |
| nextlevelbuilder/goclaw | CLI agent | 低 | 多租户分配 Maki 实例 |
| stagewise-io/stagewise | CLI agent | 低 | 开源模型 agentic IDE，CLI 接入 |
| YaoApp/yao | 自定义注册 | 中 | 需要写 agent 注册适配器 |
| Nasiko-Labs/nasiko | A2A | 需开发 | Maki 无 A2A 协议，需新增 A2A 支持 |

### 3. 对智能体（Maki）的要求

| 要求 | Maki 满足？ | 说明 |
|---|---|---|
| CLI 可执行 | ✅ | `maki "prompt" --print` |
| stdin/stdout 通信 | ✅ | `maki -p` 支持管道输入 |
| 结构化输出 | ✅ | JSON / stream-json 输出 |
| Claude Code 兼容 | ✅ | 官方文档确认 drop-in 替换 |
| ACP 协议 | ✅ | `maki acp`，ndjson stdio |
| MCP 工具端 | ✅（客户端） | 可消费 MCP 工具，但不暴露为 MCP server |
| A2A 协议 | ❌ | 不支持，需开发 |
| 会话持久化 | ✅ | session 支持 resume |
| 无头运行 | ✅ | `--print` / `-p` 完全无交互 |
| 权限控制 | ✅ | `--yolo` / `PermissionsConfig` |
| 多模型切换 | ✅ | `--model provider/model-id` |

### 4. 结论

Maki 通过 `--print`（Claude Code drop-in）可被 waggle、superset、mission-control、AgentsMesh、herdr、agent-orchestrator、goclaw、stagewise 共 8 个平台直接集成，无需额外开发。A2A 协议是缺失项，限制了与 nasiko 等 A2A 控制面的集成。

## 十四、A2A 协议分析

### 1. 协议简介

**A2A（Agent2Agent Protocol）** 是 Google 于 2025 年 4 月发起、后捐赠给 Linux Foundation 的开放标准，当前版本 v1.0.0。定位是"智能体之间的 HTTP 协议"——让不同框架、不同厂商、不同组织的智能体能互相发现、委派任务、共享结果。

| 维度 | 说明 |
|---|---|
| 协议层 | JSON-RPC 2.0 / gRPC / HTTP+JSON over HTTP |
| 核心原语 | Agent Card（发现）、Task（任务生命周期）、Message（多轮对话）、Artifact（产物） |
| 发现机制 | 静态 Agent Card 在 `/.well-known/agent-card.json`，无需预先连接即可发现 |
| 任务模型 | 有状态：created → working → input-required → completed/failed/cancelled |
| 通信模式 | 异步长时任务 + SSE 流式 + Push Notification 回调 |
| 认证 | OAuth 2.0 / Bearer Token，v1.0 加入签名 Agent Card（防伪造） |
| 治理 | Linux Foundation Agentic AI Foundation（AAIF），与 MCP 同属一个基金会 |

与 MCP 的关系：不是竞争，是互补。MCP 是"智能体→工具"（垂直），A2A 是"智能体→智能体"（水平）。一句话：MCP 给智能体装手，A2A 给智能体配同事。

### 2. 支持 A2A 的智能体/平台

从本次调研覆盖的项目中，明确支持 A2A 的有：

| 项目 | 语言 | ★ | A2A 支持方式 |
|---|---|---|---|
| [trpc-group/trpc-agent-go](https://github.com/trpc-group/trpc-agent-go) | Go | 1,735 | 内置 A2A 协议集成 |
| [microsoft/agent-framework-go](https://github.com/microsoft/agent-framework-go) | Go | 541 | 内置 A2A 支持 |
| [Nasiko-Labs/nasiko](https://github.com/Nasiko-Labs/nasiko) | Rust | 5,338 | 核心就是 A2A 控制面（deploy/route/secure/observe A2A agent） |
| [go-gocode/gocel](https://github.com/go-gocode/gocel) | Go | — | 支持 Google A2A v1.0.0，HTTP/Stdio 通信 |
| [vinhnx/VTCode](https://github.com/vinhnx/VTCode) | Rust | 824 | 协议列表含 A2A |
| [gianlucamazza/orka](https://github.com/gianlucamazza/orka) | Rust | 6 | 内置 A2A 协议（Agent Card + JSON-RPC） |
| [langoai/lango](https://github.com/langoai/lango) | Go | — | 能力层含 A2A |

此外，Google ADK、Agent Engine、AgentSpace、以及 Salesforce/Accenture/MongoDB 等 50+ 企业合作伙伴也原生支持 A2A。

### 3. 优势

| 优势 | 说明 |
|---|---|
| 跨厂商互操作 | Google agent ↔ Anthropic agent ↔ OpenAI agent，同一协议通信 |
| 静态发现 | Agent Card 在公开 URL，其他智能体无需预配置即可发现能力 |
| 有状态长时任务 | 任务有完整生命周期（创建→执行→需输入→完成/失败），支持分钟到小时级异步执行 |
| 跨组织信任 | OAuth 2.0 + v1.0 签名 Agent Card，适合跨公司/跨团队委托 |
| 企业级 | 50+ 启动合作伙伴，Linux Foundation 治理，v1.0 已稳定 |
| 多模态 | 原生支持文本/音频/视频/表单/iframe 等交互 |
| 与 MCP 互补 | 一个智能体可以同时用 MCP 访问工具、用 A2A 与其他智能体协作 |

### 4. 缺点

| 缺点 | 说明 |
|---|---|
| 实现复杂度高 | 协调原语 10 个（MCP 仅 4 个），每次交互 6 个协调阶段（MCP 仅 2 个）——学术论文实测 |
| 过度设计风险 | 如果只有一个智能体、或所有智能体在同一信任域内，A2A 是额外负担（MCP 足够） |
| 生态不如 MCP 成熟 | MCP 有 1000+ 现成 server、97M 月下载量；A2A 工具链仍较薄 |
| 调试困难 | 多智能体系统的调用链追踪、端到端可观测性尚未标准化 |
| 身份伪造风险 | v1.0 之前 Agent Card 可被伪造（v1.0 签名卡已解决，但需实际部署） |
| 需要常驻服务 | Agent Card 需要 HTTP 端点常驻（不像 MCP 可以 stdio 本地进程） |
| 成本高 | 多智能体系统 token 消耗更大、响应更慢，需让系统"挣回"这个复杂度 |

### 5. 一句话选型

- 只有一个智能体 + 一堆工具 → MCP 足够，不需要 A2A
- 两个以上智能体，且跨团队/跨组织/跨框架 → A2A
