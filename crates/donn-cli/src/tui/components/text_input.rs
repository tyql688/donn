//! 单行文本输入：编辑逻辑委托 `tui-input`，这里只做掩码显示与光标渲染。

use crossterm::event::{Event, KeyEvent};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

/// 单行文本输入，带光标；`mask` 时渲染为 `•`。
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    inner: Input,
    pub mask: bool,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            inner: Input::new(value.into()),
            mask: false,
        }
    }

    pub fn masked(value: impl Into<String>) -> Self {
        Self {
            mask: true,
            ..Self::new(value)
        }
    }

    pub fn value(&self) -> &str {
        self.inner.value()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.value().is_empty()
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.inner = Input::new(value.into());
    }

    /// 掩码开关（小眼睛：^t 明文/掩码切换）。
    pub fn toggle_mask(&mut self) {
        self.mask = !self.mask;
    }

    /// 处理按键；返回是否消费（编辑键位表由 tui-input 提供：含 Ctrl-U/W、Home/End 等）。
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        self.inner
            .handle_event(&Event::Key(*key))
            .is_some_and(|state| state.value)
            || matches!(
                key.code,
                crossterm::event::KeyCode::Left
                    | crossterm::event::KeyCode::Right
                    | crossterm::event::KeyCode::Home
                    | crossterm::event::KeyCode::End
            )
    }

    /// 渲染为带光标高亮的 Line；超宽时横向滚动保证光标可见。
    /// `width` = 可用显示宽度（0 视为不限制）。窗口按**显示宽度**计量（CJK 占 2 列），
    /// 光标字符恒可见；行宽不超过 width（滚动提示 `…` 占用的 1 列在预算内）。
    pub fn render_line(&self, focused: bool, width: usize) -> Line<'static> {
        use unicode_width::UnicodeWidthChar;
        let char_w = |c: char| c.width().unwrap_or(0);
        let chars: Vec<char> = if self.mask {
            self.inner.value().chars().map(|_| '•').collect()
        } else {
            self.inner.value().chars().collect()
        };
        // 光标要占一列
        let window = if width == 0 {
            usize::MAX
        } else {
            width.saturating_sub(1).max(1)
        };
        let total_width: usize = chars.iter().map(|c| char_w(*c)).sum();

        if !focused {
            let mut shown = String::new();
            let mut used = 0usize;
            for c in &chars {
                let w = char_w(*c);
                if used + w > window {
                    break;
                }
                shown.push(*c);
                used += w;
            }
            if used < total_width {
                shown.push('…');
            }
            return Line::from(shown);
        }

        let cursor = self.inner.cursor().min(chars.len());
        // 从光标向左回退，直到光标字符（恒可见）连同左侧内容恰好放进 window 列
        let mut scroll = cursor;
        let mut used = chars[scroll..cursor]
            .iter()
            .map(|c| char_w(*c))
            .sum::<usize>()
            + chars.get(cursor).map(|c| char_w(*c)).unwrap_or(0);
        while scroll > 0 && used + char_w(chars[scroll - 1]) <= window {
            used += char_w(chars[scroll - 1]);
            scroll -= 1;
        }
        // 向右吃掉剩余预算
        let mut end = cursor;
        let mut budget = window.saturating_sub(used);
        while end < chars.len() && budget >= char_w(chars[end]) {
            budget -= char_w(chars[end]);
            end += 1;
        }

        let mut spans = Vec::with_capacity(4);
        if scroll > 0 {
            spans.push(Span::styled("…".to_string(), Style::default()));
        }
        let before: String = chars[scroll..cursor].iter().collect();
        let at: String = chars
            .get(cursor)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".into());
        let after: String = if cursor < chars.len() {
            chars[cursor + 1..end].iter().collect()
        } else {
            String::new()
        };
        spans.push(Span::raw(before));
        spans.push(Span::styled(at, cursor_style()));
        spans.push(Span::raw(after));
        Line::from(spans)
    }
}

/// 光标反色样式。
fn cursor_style() -> ratatui::style::Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn editing_via_tui_input() {
        let mut input = TextInput::new("ab");
        input.handle_key(&key(KeyCode::Char('c')));
        assert_eq!(input.value(), "abc");
        input.handle_key(&key(KeyCode::Left));
        input.handle_key(&key(KeyCode::Left));
        input.handle_key(&key(KeyCode::Char('x')));
        assert_eq!(input.value(), "axbc");
        input.handle_key(&key(KeyCode::Backspace));
        assert_eq!(input.value(), "abc");
    }

    #[test]
    fn unicode_safe() {
        let mut input = TextInput::new("中文");
        input.handle_key(&key(KeyCode::Char('a')));
        assert_eq!(input.value(), "中文a");
        input.handle_key(&key(KeyCode::Backspace));
        input.handle_key(&key(KeyCode::Backspace));
        assert_eq!(input.value(), "中");
    }

    #[test]
    fn masked_render_hides_value() {
        let input = TextInput::masked("secret");
        let line = input.render_line(false, 0);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert_eq!(text, "••••••");
    }

    #[test]
    fn cjk_input_windows_by_display_width() {
        use unicode_width::UnicodeWidthStr;
        let mut input = TextInput::new("");
        for c in "中文输入abc".chars() {
            input.handle_key(&key(KeyCode::Char(c)));
        }
        // 光标在末尾：可见部分 ≤ 12 列且尾部可见
        let line = input.render_line(true, 12);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(UnicodeWidthStr::width(text.as_str()) <= 12, "{text}");
        assert!(text.contains('c'), "尾部可见: {text}");
        // Home：头部可见
        input.handle_key(&key(KeyCode::Home));
        let line = input.render_line(true, 12);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(UnicodeWidthStr::width(text.as_str()) <= 12, "{text}");
        assert!(text.starts_with('中'), "头部可见: {text}");
        // 非焦点截断也不超宽
        let line = input.render_line(false, 5);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(UnicodeWidthStr::width(text.as_str()) <= 5, "{text}");
        assert!(text.ends_with('…'), "{text}");
    }

    #[test]
    fn long_value_scrolls_to_keep_cursor_visible() {
        let mut input = TextInput::new("");
        for c in "sk-0123456789abcdefghijklmnopqrstuvwxyz".chars() {
            input.handle_key(&key(KeyCode::Char(c)));
        }
        // 光标在末尾：窗口内必须能看到尾部字符
        let line = input.render_line(true, 12);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.contains("wxyz"), "尾部可见: {text}");
        assert!(text.starts_with('…'), "左侧有滚动提示: {text}");
        // 光标回到开头：窗口回到头部
        input.handle_key(&key(KeyCode::Home));
        let line = input.render_line(true, 12);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.starts_with("sk-"), "头部可见: {text}");
        // 非焦点：截断加省略号
        let line = input.render_line(false, 12);
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.ends_with('…') && text.len() <= 14, "{text}");
    }
}
