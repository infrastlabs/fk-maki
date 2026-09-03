# Maki 弹窗宽度与高度逻辑分析

**日期**: 2026-09-03
**主题**: 模型选择器、帮助、/btw、usage、搜索等弹窗的尺寸计算逻辑

## 一、核心 Modal 组件

文件: `maki-ui/src/components/modal.rs`

```rust
pub struct Modal<'a> {
    pub title: &'a str,
    pub width_percent: u16,      // 终端宽度的百分比
    pub max_height_percent: u16, // 终端高度的最大百分比
}

pub const CHROME_LINES: u16 = 2; // 边框 + 标题行数
```

### 1. 尺寸算法 (`Modal::render`)

```rust
let max_h = (area.height as u32 * self.max_height_percent as u32 / 100) as u16;
let total_h = (content_height + CHROME_LINES)
    .min(max_h)
    .max(CHROME_LINES + 1);

// 垂直居中
Layout::vertical([Constraint::Length(total_h)]).flex(Flex::Center)
// 水平居中
Layout::horizontal([Constraint::Percentage(self.width_percent)]).flex(Flex::Center)
```

返回 `(popup, inner)`,其中 `inner = block.inner(popup)` 是去掉边框后的内容区。

## 二、各弹窗尺寸一览

| 弹窗 | width_percent | max_height_percent | 文件 |
|------|--------------|-------------------|------|
| **Model Picker** (Ctrl+M) | 65 | 80 | `list_picker.rs` |
| **Help** (Ctrl+H) | 50 | 80 | `help_modal.rs` |
| **/btw** | 65 | 80 | `btw_modal.rs` |
| **Usage** (/usage) | 60 | 70 | `usage_modal.rs` |
| **Search** (转录搜索) | 50 | 60 | `search_modal.rs` |
| **Command Palette** (`:`) | 内容自适应 | 内容自适应(锚定输入框) | `command.rs` |
| **Login Picker** | 50~65 | 30~40(按步骤) | `login_picker.rs` |

## 三、两种尺寸策略

### 1. 内容驱动 + Modal 包裹 (Model Picker, Help, /btw, Usage, Search)

先计算内容高度,再交给 Modal 渲染并钳制最大高度:

```rust
// list_picker.rs:render_ready()
let content_rows = visual_rows_in_range(&s.filtered, &s.items, 0, s.filtered.len()) as u16;
let modal = Modal {
    title,
    width_percent: MIN_WIDTH_PERCENT,     // 65
    max_height_percent: MAX_HEIGHT_PERCENT, // 80
};
let (popup, inner) = modal.render(
    frame,
    area,
    content_rows + SEARCH_ROW + footer_rows + error_rows,
);
let viewport_h = inner.height.saturating_sub(error_rows + SEARCH_ROW + footer_rows);
s.viewport_height = viewport_h as usize;
```

### 2. 锚定输入框 (Command Palette)

不居中,直接贴在输入框上方,宽度由内容决定:

```rust
// command.rs:view()
let popup_height = (filtered.len() as u16).min(input_area.y);
let popup_width = (PAD + max_name + GAP + max_desc + PAD) as u16;

let popup = Rect {
    x: input_area.x,
    y: input_area.y.saturating_sub(popup_height),
    width: popup_width.min(input_area.width),
    height: popup_height,
};
```

## 四、关键常量

```rust
// list_picker.rs
const MIN_WIDTH_PERCENT: u16 = 65;
const MAX_HEIGHT_PERCENT: u16 = 80;
const SEARCH_ROW: u16 = 1;

// search_modal.rs
const MODAL_WIDTH_PERCENT: u16 = 50;
const MODAL_MAX_HEIGHT_PERCENT: u16 = 60;

// modal.rs
pub const CHROME_LINES: u16 = 2;
```

## 五、高度计算规律

1. **最小高度**: `content_height + CHROME_LINES` 下限为 `CHROME_LINES + 1`(至少 3 行)
2. **最大高度**: 终端高度的 `max_height_percent`%(60%~80%)
3. **实际高度**: `min(内容高度, 最大高度)`,超出部分由内部滚动条处理
4. **宽度**: 固定百分比(50%~65%),不受内容影响,水平居中

## 六、滚动处理

- 所有可滚动弹窗用 `ModalScroll` 管理 offset
- `scroll.update_dimensions(total, viewport_h)` 在渲染时同步
- 内容超出时渲染垂直滚动条(`render_vertical_scrollbar`)
- 键位: PgUp/PgDn、Ctrl+U/Ctrl+D、鼠标滚轮

## 七、特例

- **/btw**: 内容宽度按 `area.width * 65% - 2*border - 2*H_PAD` 先计算换行后的行数,再用 `Paragraph::wrap` 渲染,高度是换行后动态的
- **Usage**: 底部有 "Ctrl+R reload" 提示,渲染在 popup 右下角(`popup.x + popup.width - hint_w - 1`)
- **Command Palette**: 唯一不居中、锚定输入框的弹窗,宽度内容自适应