//! 可复用的 UI 组件：不含业务逻辑，只有输入、状态和渲染。

pub mod list;
pub mod modal;
pub mod status_bar;
pub mod text_input;

use donn_core::Effort;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, List, ListItem, ListState};
use unicode_truncate::{Alignment, UnicodeTruncateStr};
use unicode_width::UnicodeWidthStr;

/// 统一列表渲染骨架：List + `❯ ` 高亮符 + 原生滚动状态。
/// profiles/presets/doctor 传 `Some(block)` 由 List 画整框；
/// detail/settings 已单独画框并切好子区，传 `None`。命中区仍由各调用方按语义记录。
pub fn draw_list<'a>(
    f: &mut Frame,
    area: Rect,
    items: Vec<ListItem<'a>>,
    highlight: Style,
    state: &mut ListState,
    block: Option<Block<'a>>,
) {
    let mut list = List::new(items)
        .highlight_style(highlight)
        .highlight_symbol("❯ ");
    if let Some(block) = block {
        list = list.block(block);
    }
    f.render_stateful_widget(list, area, state);
}

/// 按终端显示宽度补齐到 `width` 列（`format!("{s:<w}")` 按字符数补齐，CJK 会错位）。超宽不截断。
pub fn pad(s: &str, width: usize) -> String {
    s.unicode_pad(width, Alignment::Left, false).into_owned()
}

/// Effort 选择弹窗的选项表与当前档下标（detail 行与 add 表单共用）。
pub fn effort_options(effort_auto: &str, current: Effort) -> (Vec<String>, usize) {
    let options = Effort::ALL
        .iter()
        .map(|e| e.env_value().unwrap_or(effort_auto).to_string())
        .collect();
    (options, current.index())
}

/// nucleo 模糊打分：返回命中项 `(score, index)`，按分数降序。空 needle 由调用方处理。
pub fn fuzzy_rank(needle: &str, haystacks: impl Iterator<Item = String>) -> Vec<(u32, usize)> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(needle, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = haystacks
        .enumerate()
        .filter_map(|(i, hay)| {
            pattern
                .score(Utf32Str::new(&hay, &mut buf), &mut matcher)
                .map(|s| (s, i))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored
}

/// 带边框面板内列表的鼠标命中区：去掉上下边框各一行。
/// 各面板渲染时记录进 `HitAreas`，鼠标点击/滚轮据此换算条目下标。
pub fn list_hit_area(area: Rect) -> Rect {
    let mut inner = area;
    inner.y += 1;
    inner.height = inner.height.saturating_sub(2);
    inner
}

/// 从右侧省略到给定显示宽度（保留开头）。
pub fn fit_right(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_string();
    }
    match width {
        0 => String::new(),
        1 => "…".to_string(),
        _ => format!("{}…", value.unicode_truncate(width - 1).0),
    }
}

/// 把「值 + 后缀」这组 span 收进给定宽度：省略第一个 span（值），后缀（如 `(default)`）保持完整；
/// 窄到后缀要占一半以上时丢掉后缀。
pub fn fit_spans(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| s.content.width()).sum();
    if total <= width {
        return spans;
    }
    let rest: usize = spans.iter().skip(1).map(|s| s.content.width()).sum();
    let mut room = width.saturating_sub(rest);
    // 值比后缀标记重要：后缀要占掉一半以上宽度时丢掉它，整行让给值
    if room * 2 < width {
        spans.truncate(1);
        room = width;
    }
    if let Some(first) = spans.first_mut() {
        first.content = fit_right(&first.content, room).into();
    }
    spans
}

/// 从左侧省略到给定显示宽度（路径尾部比头部更有信息量）。
pub fn fit_left(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    match width {
        0 => String::new(),
        1 => "…".to_string(),
        _ => format!("…{}", s.unicode_truncate_start(width - 1).0),
    }
}
