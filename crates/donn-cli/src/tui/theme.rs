//! Theme：全部样式的唯一出口。渲染代码不得内联颜色/边框——改观感只改这里。

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Padding};

use crate::tui::components::status_bar::Severity;

/// Claude 品牌橙（#D97757）。
const CLAUDE_ORANGE: Color = Color::Rgb(217, 119, 87);

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// 主强调色：焦点边框、选中项、徽标。
    pub accent: Color,
    /// 次级文字（提示、标签、非焦点内容）。
    pub dim: Color,
    /// 非焦点面板边框。
    pub border: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    /// 选中行背景。
    pub sel_bg: Color,
    /// 选中行前景。
    pub sel_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: CLAUDE_ORANGE,
            dim: Color::DarkGray,
            border: Color::DarkGray,
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            sel_bg: CLAUDE_ORANGE,
            sel_fg: Color::Black,
        }
    }
}

impl Theme {
    /// 面板外框：焦点面板亮边框 + 加粗标题，非焦点淡化。
    pub fn pane_block<'a>(&self, title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
        let (border_style, title_style) = if focused {
            (
                Style::default().fg(self.accent),
                Style::default()
                    .fg(self.accent)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                Style::default().fg(self.border),
                Style::default().fg(self.dim),
            )
        };
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
            .title(title.into().style(title_style))
            .padding(Padding::horizontal(1))
    }

    /// 弹窗外框。
    pub fn modal_block<'a>(&self, title: impl Into<Line<'a>>) -> Block<'a> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.accent))
            .title(
                title.into().style(
                    Style::default()
                        .fg(self.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .padding(Padding::horizontal(1))
    }

    pub fn selected(&self) -> Style {
        Style::default()
            .fg(self.sel_fg)
            .bg(self.sel_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// 非焦点面板中的选中行：不抢焦点面板的视觉。
    pub fn selected_unfocused(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    /// 列表高亮样式：按面板焦点状态选取。
    pub fn highlight(&self, focused: bool) -> Style {
        if focused {
            self.selected()
        } else {
            self.selected_unfocused()
        }
    }

    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn ok(&self) -> Style {
        Style::default().fg(self.ok)
    }

    pub fn warn(&self) -> Style {
        Style::default().fg(self.warn)
    }

    pub fn err(&self) -> Style {
        Style::default().fg(self.err).add_modifier(Modifier::BOLD)
    }

    pub fn severity(&self, severity: Severity) -> Style {
        match severity {
            Severity::Ok => self.ok(),
            Severity::Warn => self.warn(),
            Severity::Error => self.err(),
        }
    }
}
