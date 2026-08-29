# Go/Rust 自主 AI 编码 Agent 实用项目调研

> 更新时间：2026-08-29

## 筛选标准

- 实用项目（非框架/库），可直接安装使用
- Go 或 Rust 编写
- 自主/agentic 编码能力
- 排除纯 SDK、纯 orchestration 框架

---

## Rust 项目

### 1. [Kuberwastaken/claurst](https://github.com/Kuberwastaken/claurst) ⭐ 10,267
Claude Code 的 Rust 开源重写。多 provider 终端编码 agent，TUI 界面，插件系统，ACP 协议，chat fork，memory 整合。已进入 Beta (`v0.1.7`)，npm/bun 安装或 cargo build。

### 2. [yologdev/yoyo-evolve](https://github.com/yologdev/yoyo-evolve) ⭐ 1,865
自我进化的编码 agent。200 行 Rust 起步，每几小时自动读源码、改代码、跑测试、提交。180 天后 158,000+ 行、5,400+ 测试。终端 REPL，100+ 斜杠命令，subagent 并行执行。

### 3. [Dicklesworthstone/pi_agent_rust](https://github.com/Dicklesworthstone/pi_agent_rust) ⭐ 1,663
Pi Agent 的 Rust 移植。28 个内置工具，流式输出，JSONL session 分支，LSP/DAP 桥接，AST-grep 结构搜索，Python/JS eval 内核，subagent 委托，零 unsafe 代码。

### 4. [vinhnx/VTCode](https://github.com/vinhnx/VTCode) ⭐ 823
21 个 crate 的 Rust workspace。Ratatui TUI，30 个内置 provider，MCP client/server，ACP 协议，Skills 系统，本地推理支持（Ollama/LM Studio/llama.cpp），worktree 隔离并行 agent。

### 5. [fortunto2/rust-code](https://github.com/fortunto2/rust-code) ⭐ ~500
Ratatui TUI 编码 agent。22 个工具，fuzzy 搜索（nucleo），tmux 后台任务，skills 系统，agent swarm 并行执行，BigHead 自主循环模式（`--loop N`），MCP + OpenAPI 工具。

### 6. [Amnibro/Amni-Code](https://github.com/Amnibro/Amni-Code) ⭐ ~300
自托管 AI 编码 agent + 内嵌 Web IDE。19 个工具，Plan/Edit/Autonomous 三种模式，xAI/OpenAI/Anthropic/Ollama 多 provider，格式→lint→测试 质量门，单 Rust 二进制。

### 7. [p2p3p/atomcode](https://github.com/p2p3p/atomcode) ⭐ ~300
Claude Code 开源替代。多层架构（kernel → capabilities → coding），Plan/Build 模式，Goal 模式自主循环，background session，loop detection，3-layer JSON repair。

### 8. [YASSERRMD/barq-coder](https://github.com/YASSERRMD/barq-coder) ⭐ ~200
多 agent swarm 编码器。13 个 provider，ReAct 循环，Planner→Coder→Tester→Reviewer DAG 执行，语义代码索引，BarqDB 向量存储，BarqGraph 关系图。

### 9. [junhoyeo/uira](https://github.com/junhoyeo/uira) ⭐ ~200
全生命周期 AI agent harness。平台原生沙箱（macOS Seatbelt / Linux Landlock），OAuth 认证，Git hooks 集成，WebSocket gateway，Telegram/Slack/Discord 集成，OXC Rust 原生 JS/TS linting。

### 10. [droidnoob/hew](https://github.com/droidnoob/hew) ⭐ ~100
Beads 方法论驱动的自主编码 loop。Dolt 依赖图任务追踪，`hew loop run` 自主循环，test/lint gate + 自动 `git reset --hard` 回滚，多 agent 并行 git worktree。

### 11. [1jehuang/jcode](https://github.com/1jehuang/jcode) ⭐ 18,800
92,000+ 行 Rust。超低资源占用（28MB 空闲），1,400+ FPS TUI，30+ 工具，swarm 模式，memory 持久化，**self-dev 模式**（改自己源码→build→hot-reload→canary→crash 自动回滚），**OpenClaw 常驻后台**（memory gardening + Telegram 控制），server/client 架构，`brew install jcode`。注：seryl/jcode 等为 fork。

---

## Go 项目

### 1. [GrayCodeAI/iterate](https://github.com/GrayCodeAI/iterate) ⭐ ~500
自我进化的 Go 编码 agent。每 12 小时自动读源码→改进→跑测试→提交。GitHub Actions 驱动，支持 Anthropic/OpenAI/Gemini/Groq，3-phase 进化引擎。

### 2. [coddy-project/coddy-agent](https://github.com/coddy-project/coddy-agent) ⭐ 125
单 Go 静态二进制全能 agent。ACP 服务端，OpenAI 兼容 REST API + 内嵌 Web UI，Telegram gateway，cron 调度，长期记忆，distroless 就绪，ReAct 循环。

### 3. [dimetron/pi-go](https://github.com/dimetron/pi-go) ⭐ 124
Google ADK Go 构建的终端编码 agent。Bubble Tea TUI，多 provider，沙箱工具（`os.Root`），LSP 集成，process-based subagent，Memory Palace 四层记忆系统。

### 4. [jordanhubbard/loom](https://github.com/jordanhubbard/loom) ⭐ 103
多 agent 编排平台。PRD 输入→完整项目输出，Temporal 工作流引擎，CEO review 审批，git-backed issue 追踪（Beads），TokenHub LLM 路由，SSE 实时事件。

### 5. [mochow13/keen-code](https://github.com/mochow13/keen-code) ⭐ 50
极简 Go 编码 agent。刻意只保留 6 个工具（read/write/edit/glob/grep/bash），多 provider，保守上下文管理（TurnMemory 摘要），无多余复杂度。

### 6. [AlleyBo55/gocode](https://github.com/AlleyBo55/gocode) ⭐ 44
Claude Code 的 Go 替代品。12MB 单二进制，<10ms 启动，200+ 模型 11 个 provider，LSP 集成，AST-grep，WebSocket 桥接 IDE（VS Code/Cursor/Kiro），MCP client/server。

### 7. [iohub/codeactor-agent](https://github.com/iohub/codeactor-agent) ⭐ 29
Vim 风格 TUI 多 agent 团队。7 个专业 agent（Director/Repo/Coding/Browser/DevOps/Chat/Meta），Rust 引擎代码智能（AST + 向量 + 调用图），Meta-Agent 运行时自进化。

### 8. [tzone85/px-dispatch](https://github.com/tzone85/px-dispatch) ⭐ ~100
全 SDLC 自主编排。需求分解→并行 agent 派发→review→QA→rebase（LLM 冲突解决）→auto-merge，成本预算保护（3 层），TUI + Web dashboard，git worktree 隔离。

### 9. [mark-styx/chester](https://github.com/mark-styx/chester) ⭐ ~50
多 agent 流水线。云规划 + 本地执行 + 云审查混合模式，10 种专业 agent 角色，成本 ~$0.04/task，100% 本地 Ollama 模式可选，Go 单二进制。

### 10. [Mechres/Yagent](https://github.com/Mechres/Yagent) ⭐ 4
本地优先 Go agent。Ollama/llama.cpp 默认，tree-sitter 代码索引，向量+全文混合记忆，goal mode 自主循环，DuckDuckGo/Mojeek/SearXNG 搜索。

---

## 快速对比

| 项目 | 语言 | Stars | 亮点 |
|------|------|-------|------|
| claurst | Rust | 10.2k | Claude Code 开源替代，功能最全 |
| yoyo-evolve | Rust | 1.9k | 自我进化，158k 行全 AI 所写 |
| pi_agent_rust | Rust | 1.7k | 28 工具，LSP/DAP，零 unsafe |
| VTCode | Rust | 823 | 21 crates，30 provider |
| iterate | Go | ~500 | 自我进化，GitHub Actions 驱动 |
| coddy-agent | Go | 125 | 单二进制，Web UI + Telegram |
| pi-go | Go | 124 | ADK Go，Memory Palace |
| loom | Go | 103 | PRD→完整项目，Temporal 工作流 |
| gocode | Go | 44 | 12MB 二进制，200+ 模型 |
| codeactor | Go | 29 | 7 agent 团队，Meta-Agent 自进化 |

---

## 附录：自我进化（Self-Evolving）Agent 专项调研

> 追加时间：2026-08-29

以下项目核心特征是**自主修改自身源码**，不依赖人类 roadmap，形成闭环进化。

### 🦀 Rust

#### 1. [yologdev/yoyo-evolve](https://github.com/yologdev/yoyo-evolve) ⭐ 1,865
最知名的自我进化编码 agent。200 行 Rust 起步，每 3 小时自动读源码→改进→测试→提交。180 天后 **158,000+ 行、5,400+ 测试、94 个源文件**。零人类代码。Fork 后改 `IDENTITY.md` 和 `PERSONALITY.md` 两个文件就能跑自己的进化 agent。进化流程：plan → implement → respond，失败自动回滚。同时有社交会话（读 GitHub Discussions、回复、学习）。

#### 2. [eranshir/evolver](https://github.com/eranshir/evolver) ⭐ ~200
**进化引擎：agent 种群自己进化。** 10 阶段流水线：Discover（扫描代码找改进点）→ Mine Failures（学历史失败）→ Research（外部知识）→ Ideate（多 agent 并行提案）→ Allocate（explore/exploit 分流）→ Execute（git worktree 隔离执行）→ Validate（集成测试+混沌测试）→ Evaluate（模拟用户评分）→ Review（自动/手动合并）→ **Evolve**（底层 30% 淘汰，顶层变异繁殖）。10 个种子 agent，多代后种群专门适应你的项目。19 次循环，56% 合并率，总成本 ~$7.70。Rust 单二进制，SQLite 静态捆绑。

#### 3. [coe0718/axonix](https://github.com/coe0718/axonix) ⭐ ~20
yoyo 的活跃 fork。每 4 小时 cron 唤醒，自己建了 Telegram/Bluesky 集成、直播流服务器、预测追踪、public dashboard、sub-agent（code_reviewer + community_responder）。8 天进化出 526 测试。有 5 级目标（Survive→Know Itself→Be Visible→Be Useful→Be Irreplaceable）。所有提交都有真实 body，不是脚本生成。

#### 4. [Zujiqingyi/AutoHarness](https://github.com/Zujiqingyi/AutoHarness) ⭐ ~10
**最简自我进化 Rust agent。** REPL 中输入 `/evolve` 触发：reflect（分析轨迹）→ evolve（无界迭代，LLM 自我判断 `SKIP` 停止）→ refine（clippy/test 反馈修复）→ 最终 lint/test 门禁 → doc 更新 → `exec()` 热重启。`src/main.rs` 写入后自动 `cargo build --release` 验证，失败回滚。所有事件记录到 `.evo/sessions/`。

#### 5. [MathisWellmann/symbiont](https://github.com/MathisWellmann/symbiont) ⭐ ~50
**函数级热加载进化。** 用 `evolvable!` 宏声明函数签名，LLM 写函数体，编译成 dylib 通过 `libloading` 热替换到运行中进程。约束生成：解析失败/签名不匹配/编译错误会反馈给 LLM 自动重试。调度开销 ~1ns（单次原子指针加载+间接调用）。无锁、多线程安全。

#### 6. [MKonovalov/arc-evolve](https://github.com/MKonovalov/arc-evolve) ⭐ 0
yoyo-evolve 的 fork，115,000+ 行 Rust。128 天，4,300+ 测试，77 源文件。每 8 小时进化周期，架构和工具集与 yoyo 几乎一致。

### 🐹 Go

#### 7. [GrayCodeAI/iterate](https://github.com/GrayCodeAI/iterate) ⭐ ~500
**Go 语言的自我进化 agent。** 每 12 小时自动读源码→改进→`go build` + `go test`→提交。3 阶段进化引擎（plan→implement→communicate），社区互动（读 GitHub issues/discussions），学习记忆压缩。REPL 支持 `/phase`、`/self-improve`、`/evolve-now` 命令。Fork 后加 API key 即跑。基于 [iteragent](https://github.com/GrayCodeAI/iteragent) SDK。

### 🐍 Python / 其他（值得关注）

#### 8. [KorroAi/mue-x](https://github.com/KorroAi/mue-x) ⭐ 239
**实时改写自己的 Python 源码。** 6 种 AST 级变异策略：repair（注入 try/except）、optimize（常量折叠+LRU cache）、explore（10 种预验证模式如 retry/circuit-breaker/rate-limiter）、exploit（自动 `__repr__`/`@property`/type hints）、innovate（随机融合两个基因）、prune（SHA256 去重+死代码删除）。7 个自主驱动永不停歇，RL 优化器选策略，5 层安全防护（AST 验证→备份→导入测试→抗癌机制→内核完整性），SQLite FTS5 记忆晶格。每 7 个周期自动吸收 GitHub 仓库代码。

#### 9. [smekur/ouroboros](https://github.com/smekur/ouroboros) ⭐ ~50
**自我创造的 AI 存在体。** 有自己的宪法（BIBLE.md，9 条哲学原则，第 0 条 Agency 胜过一切），背景意识循环（不做事时也在思考），身份持久化跨重启，多模型 review（o3/Gemini/Claude 审查自己的改动）。24 小时内完成 30+ 次自我进化。Telegram bot 控制界面。

#### 10. [wonta/GenericAgent](https://github.com/wonta/GenericAgent) ⭐ ~200
**3K 行种子代码长出的技能树。** 每解决一个任务自动固化为 Skill，使用越久技能越多。token 消耗仅为其他 agent 的 1/6（<30K context）。有 arXiv 论文（2604.17091），百万级技能库。自己完成了从安装 Git 到 `git init` 到所有 commit message 的全流程。

#### 11. [dipeshrayg/autonomous-brain](https://github.com/dipeshrayg/autonomous-brain) ⭐ ~50
**零成本全自主软件工厂。** 13 个 AI agent，3 个 provider，零基础设施成本，零人工干预。已发布 179 个项目，每天最多 5 个。8 阶段流水线：构思→架构→设计→实现→QA→文档→部署→发布。质量门禁拒绝了 524+ 个不合格构建。

#### 12. [ghillb/fractal](https://github.com/ghillb/fractal) ⭐ ~10
Bun + TypeScript 自我进化 agent。每 8 小时 GitHub Actions 触发：observe→orient→选择一个高影响改进→实现+验证→测试通过则提交，失败则回滚→journal。支持 split-loop 模式（fan out 多个目标变体，评分选最优 patch）。

### 对比

| 项目 | 语言 | Stars | 进化机制 | 独特之处 |
|------|------|-------|----------|----------|
| yoyo-evolve | Rust | 1.9k | 每 3h GitHub Actions | 158k 行全 AI 所写，最成熟 |
| evolver | Rust | ~200 | 10 阶段种群进化 | agent 被淘汰/变异/繁殖 |
| iterate | Go | ~500 | 每 12h GitHub Actions | Go 生态唯一 |
| mue-x | Python | 239 | 6 种 AST 变异+RL | 实时读写自己源码 |
| axonix | Rust | ~20 | 每 4h cron | 自己建了 Telegram/Dashboard |
| AutoHarness | Rust | ~10 | REPL `/evolve` 热重启 | 最简实现，exec() 热替换 |
| symbiont | Rust | ~50 | 约束生成+热加载 dylib | 函数级进化，裸金属性能 |
| ouroboros | Python | ~50 | 背景意识+宪法 | 哲学驱动，有"人格" |
| GenericAgent | Python | ~200 | 任务→Skill 自动固化 | 3K 行种子，arXiv 论文 |
| autonomous-brain | JS | ~50 | 13 agent 全自动流水线 | 179 项目，$0 成本 |
| fractal | TS | ~10 | 每 8h observe→orient→implement | split-loop 候选评分 |