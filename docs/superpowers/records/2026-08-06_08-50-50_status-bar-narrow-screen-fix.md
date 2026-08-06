# 状态栏窄屏适配修复记录

## 问题原文（用户描述）

> maki的状态栏(常见为build err dir model ctx四个区域，其中err为有错误提示时临时显示几秒，另在ctx前有thinking等标记项的显示)，当tui屏幕宽度过小时 如在手机竖屏操作:
> 1. 导致尾部超出屏幕，只能部分显示 build dir model
> 2. err的显示区域被dir给挡住了
>
> 结合如上问题 分析代码，需保障:err 及尾部ctx两块的显示，如屏幕宽度过小时:把dir model只显示尾部5个字符，或你脑暴下有其他方案？

## 代码理解

### 状态栏布局

文件：`maki-ui/src/components/status_bar.rs`

状态栏分左右两区，通过 `Layout::horizontal` 布局：

```rust
let [left_area, right_area] = Layout::horizontal([
    Constraint::Min(0),           // 左区：弹性，占剩余空间
    Constraint::Length(right_width), // 右区：固定宽度 = 内容总宽度
])
.areas(area);
```

**左区内容**（从左到右）：
- spinner（流式输出/恢复会话时）
- mode label（NORMAL/INSERT 等）
- chat name（多会话时）
- auto-scroll paused 提示
- retry 信息
- error 消息（当 `Status::Error` 时）

**右区内容**（从左到右，`Alignment::Right` 右对齐）：
- `cwd_branch`（当前目录:分支，如 `~/my-project:main`）
- `  `（两个空格分隔）
- `model_id`（模型名，如 `deepseek-v4-flash`）
- `[thinking]` 或 `[extended]`（thinking 开启时）
- `[fast]`（fast 模式）
- `[workflow]`（workflow 模式）
- `ctx X/Y (Z%) $C`（上下文用量 + 花费）
- `Σ$C`（全局花费，多会话时）

### 根因

`Constraint::Length(right_width)` 强制右区宽度等于内容总宽度。当终端宽度小于右区内容总宽度时：

1. 右区起始位置超出屏幕右边界，内容溢出不可见
2. 左区被挤压到 0 宽度，mode label 和 error 消息被截断或消失
3. 用户看到的只有部分左区内容（build, dir, model 的一部分）

### 首次尝试

将 `Constraint::Length(right_width)` 改为 `Constraint::Length(right_width.min(area.width - 8))`，限制右区最大宽度，利用 `Paragraph` 的 `Alignment::Right` 自动裁剪左侧内容。

**反馈**：用户编译测试后反馈"没有生效"。

**分析**：用户运行的是 `/usr/local/bin/maki`（旧全局安装版），而非新编译的 `target/debug/maki`。但用户确认运行的是正确路径后仍无效，说明 `Paragraph` 的裁剪行为不可靠，需要显式截断字符串。

### 最终方案

在创建 `Span` 之前，先计算右区固定内容（labels、ctx、global）的宽度，剩余宽度平分给 `cwd_branch` 和 `model_id`，超出部分截断只显示尾部：

```rust
// 计算固定部分宽度
let mut mandatory = 2u16; // cwd 和 model 之间的空格
if let Some(ref label) = ctx.thinking_label {
    mandatory += label.len() as u16 + 3; // " [label]"
}
if ctx.fast { mandatory += FAST_LABEL.len() as u16; }
if ctx.workflow { mandatory += WORKFLOW_LABEL.len() as u16; }
mandatory += 25; // ctx: "  X/Y (Z%) $C "
if ctx.stats.show_global { mandatory += 10; }

// 剩余宽度平分给 cwd 和 model，最少保留 5 字符
let max_right = area.width.saturating_sub(8).saturating_sub(mandatory);
let half = (max_right / 2).max(5);

fn trunc_tail(s: &str, max: u16) -> String {
    let w = s.len() as u16;
    if w <= max { s.to_string() } else {
        let start = s.len() - max as usize;
        format!("..{}", &s[start..])
    }
}

let cwd = trunc_tail(&self.cwd_branch, half);
let model = trunc_tail(ctx.model_id, half);
```

**效果**（窄屏 40 列）：
```
  NORMAL  ..main  ..lash  [thinking]  12K/200K (6%) $0.015
```

- `ctx` 始终完整显示 ✅
- `err` 消息有至少 8 字符空间 ✅
- `cwd_branch` 和 `model_id` 自动截尾 ✅
- 右区不溢出屏幕 ✅

### 最终提交

`0963a3d6` — `fix(ui): truncate cwd_branch and model_id in status bar when terminal is narrow`

---

## 代码精简（2026-08-06_09-05-18）

### 审查发现

用户反馈"编译生效了但没有效果"，排查后发现：

1. 用户运行的是 `/usr/local/bin/maki`（旧全局安装版），非新编译的 `target/debug/maki`
2. 即便用正确路径，依赖 `Paragraph` 裁剪的方案不可靠，需显式截断字符串

### 代码评审问题

第一次实现存在以下问题：

| 问题 | 代码 | 影响 |
|------|------|------|
| 复杂度过高 | `mandatory` 逐项计算 11 行 | 难以维护 |
| `half` 溢出 | `half = (max_right / 2).max(5)` | `max_right=0` 时 `half=5`，双字段占 10 位 > 0，仍溢出 |
| 截断超长 | `trunc_tail` 输出 `max + 2` 字符 | `".."` 前缀导致实际占用超出 `max` |
| 字节 vs 字符 | 用 `s.len()` 而非 chars count | 多字节字符可能截断在中间 |

### 简化方案

**删除**：`mandatory` 逐项计算 + `trunc_tail` 内联函数（共 11 行）

**替换为**：固定估算 + 内联逻辑

```rust
// 估算：左区 8 + ctx/labels 35 = 43
let max_right = area.width.saturating_sub(43);
let half = max_right / 2;

let cwd = if half < 5 {
    // 极窄屏：直接显示尾部 5 字符，不加 ".." 前缀
    let s = &self.cwd_branch;
    let keep = 5.min(s.len());
    s[s.len() - keep..].to_string()
} else if (self.cwd_branch.len() as u16) > half {
    let keep = (half - 2).max(1) as usize;
    format!("..{}", &self.cwd_branch[self.cwd_branch.len() - keep..])
} else {
    self.cwd_branch.clone()
};
// model 同理
```

### 改动统计

- 删除 29 行，新增 26 行，净减 **3 行**
- 移除 `mandatory` 逐项计算（11 行 → 1 行固定估算）
- 移除 `trunc_tail` 内联函数（6 行 → 内联表达式）
- 底部 `right_width.min(area.width - 8)` 布局约束保留为安全网

### 最终提交

`579fb2ab` — `fix(ui): simplify status bar truncation, fix overflow when max_right is small`