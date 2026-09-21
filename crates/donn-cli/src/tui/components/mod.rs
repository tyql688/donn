//! 可复用 UI 组件：无业务逻辑，纯输入/状态/渲染。

pub mod list;
pub mod modal;
pub mod status_bar;
pub mod text_input;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
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
pub fn effort_options(effort_auto: &str, current: donn_core::Effort) -> (Vec<String>, usize) {
    use donn_core::Effort;
    let options = Effort::ALL
        .iter()
        .map(|e| e.env_value().unwrap_or(effort_auto).to_string())
        .collect();
    (options, current.index())
}

/// nucleo 模糊打分：返回命中项 `(score, index)`，按分数降序。空 needle 由调用方处理。
pub fn fuzzy_rank(needle: &str, haystacks: impl Iterator<Item = String>) -> Vec<(u32, usize)> {
    use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher, Utf32Str};
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
pub fn list_hit_area(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
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
pub fn fit_spans(
    mut spans: Vec<ratatui::text::Span<'static>>,
    width: usize,
) -> Vec<ratatui::text::Span<'static>> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn fit_left_and_right_ellipsize_by_display_width() {
        assert_eq!(super::fit_left("abcdef", 10), "abcdef");
        assert_eq!(super::fit_left("abcdef", 4), "…def");
        assert_eq!(super::fit_left("路径中文", 5), "…中文");
        assert_eq!(super::fit_right("abcdef", 4), "abc…");
        assert_eq!(super::fit_right("中文路径", 5), "中文…");
        assert_eq!(super::fit_right("abc", 1), "…");
        assert_eq!(super::fit_right("abc", 0), "");
    }

    #[test]
    fn fit_spans_trims_only_the_value_and_keeps_the_suffix() {
        use ratatui::text::Span;
        let spans = super::fit_spans(
            vec![
                Span::raw("https://api.z.ai/api/anthropic"),
                Span::raw("  (default)"),
            ],
            24,
        );
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "https://api.…  (default)");
        assert_eq!(unicode_width::UnicodeWidthStr::width(text.as_str()), 24);

        // 窄到后缀要占一半以上：丢后缀保值
        let spans = super::fit_spans(
            vec![
                Span::raw("https://api.z.ai/api/anthropic"),
                Span::raw("  (default)"),
            ],
            12,
        );
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "https://api…");
    }

    #[test]
    fn pad_accounts_for_cjk_width() {
        assert_eq!(super::pad("abc", 5), "abc  ");
        assert_eq!(super::pad("模型", 6), "模型  "); // 2 个宽字符 = 4 列
        assert_eq!(super::pad("too-long", 4), "too-long");
    }
}
