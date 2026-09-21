//! 状态栏：类型化 severity 的消息 + 上下文键提示。

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warn,
    Error,
}

/// 当前状态消息。默认为空（只显示键提示）。
#[derive(Debug, Clone, Default)]
pub struct Status {
    message: Option<(String, Severity)>,
}

impl Status {
    fn set(&mut self, message: impl Into<String>, severity: Severity) {
        self.message = Some((message.into(), severity));
    }

    pub fn ok(&mut self, message: impl Into<String>) {
        self.set(message, Severity::Ok);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.set(message, Severity::Warn);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.set(message, Severity::Error);
    }

    pub fn clear(&mut self) {
        self.message = None;
    }

    /// 渲染：有消息显示消息，否则显示键提示（`键 动作` 对）。
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme, hints: &[(&str, &str)]) {
        let line = match &self.message {
            Some((text, severity)) => {
                let mark = match severity {
                    Severity::Ok => "✓",
                    Severity::Warn => "⚠",
                    Severity::Error => "✗",
                };
                Line::from(Span::styled(
                    format!(" {mark} {text}"),
                    theme.severity(*severity),
                ))
            }
            None => {
                let mut spans = vec![Span::raw(" ")];
                for (i, (key, action)) in hints.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::styled("  ·  ", theme.dim()));
                    }
                    spans.push(Span::styled(*key, theme.accent()));
                    spans.push(Span::styled(format!(" {action}"), theme.dim()));
                }
                Line::from(spans)
            }
        };
        f.render_widget(Paragraph::new(line), area);
    }
}
