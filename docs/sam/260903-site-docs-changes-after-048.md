# site 目录文档 v0.4.8 之后变更检查

**日期**: 2026-09-03
**主题**: 检查 `site/docs/content/` 自 v0.4.8 标签以来的文档变更情况

## 规模

- 76 个提交涉及 `site/`(v0.4.8..HEAD 全部 173 个提交,site 约占 44%)
- 文档内容:`19 个文件,+2254 / -93 行`
- 无删除页面

## 一、新增文档页面(5 个,v0.4.8 后新增功能)

| 页面 | 内容 |
|------|------|
| hooks/ | 工具槽位(tool slots)钩子系统 |
| notifications/ | 终端注意力通知(attention notifications) |
| packages/ | Lua 包管理(install/update/delete、plugin.toml、权限批准) |
| plugins/ | 插件编写指南(代码位置、权限、调试) |
| telemetry/ | OpenTelemetry 导出(metrics + events) |

## 二、主要变更(非新增页面)

| 页面 | 变更量 | 对应功能 |
|------|--------|---------|
| lua-api/ | +1082 | 最大变更:jobs API(jobstart/jobinfo/joblist/jobfind/jobattach/jobstop/jobwait/jobforget)、maki.model、maki.toast、defer_fn、notify、ModelChanged/SessionEnd/SessionStatusChanged 事件、maki.version 版本检查、managed package state |
| providers/ | +112 | Regolo、Aperture、DeepSeek 价格/高峰计费、llama-cpp thinking、Copilot keyring |
| configuration/ | +73 | `--no-rtk` → `agent.rtk`、telemetry env vars、SSRF 私有主机白名单、env 展开的 MCP 头 |
| keybindings/ | +14 | Ctrl+P 会话选择器、Ctrl+M 模型选择器、插件拥有的 bindings |
| commands/ | +8 | /packupdate、/packdel 新命令 |
| context/ | +15 | 子代理工具过滤器相关 |
| mcp/、tools/、permissions/、cli/、acp/、token-economy/、_index | 小 | plan mode MCP 说明、edit_lines 默认开、insert_lines 修复、elicitation 等 |

## 三、与 260902-v048-changes.md 的一致性

- 文档反映的变更与先前调查完全吻合:jobs API、事件、包管理(maki-pack)、遥测(maki-otel)、新提供商、UI 选择器键位、edit/ACP 修复
- 唯一 260902 提到但 site 未单独成页的大项:**UI 渲染性能(segment 化滚动)** —— 属内部实现,无对应新增文档页(仅 lua-api/plugins 里间接提到)

## 四、检查命令速查

```bash
git log --oneline v0.4.8..HEAD -- site/ | wc -l        # 76
git diff --stat v0.4.8..HEAD -- site/docs/content/      # +2254/-93
git diff --diff-filter=A --name-only v0.4.8..HEAD -- site/docs/content/  # 新页面 5 个
```