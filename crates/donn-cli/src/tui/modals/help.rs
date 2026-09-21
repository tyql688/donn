//! `?` 帮助速查表：内容与 keymap 同源。

use crate::tui::i18n;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::tui::app::Core;
use crate::tui::components::modal::{Modal, ModalOutcome, render_box};
use crate::tui::keymap;

pub struct HelpModal;

impl Modal for HelpModal {
    fn handle(&mut self, _key: KeyEvent, _core: &mut Core) -> ModalOutcome {
        // 任意键关闭
        ModalOutcome::Close
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        let mut lines: Vec<Line> = Vec::new();
        for (section, entries) in keymap::cheatsheet() {
            lines.push(Line::from(Span::styled(
                section.to_string(),
                theme.accent(),
            )));
            for (key, action) in entries {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {key:<12}"), theme.ok()),
                    Span::raw(action),
                ]));
            }
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(i18n::HELP_CLOSE, theme.dim())));

        let height = (lines.len() as u16 + 2).min(area.height);
        let width = 54.min(area.width.saturating_sub(2));
        render_box(f, area, theme, i18n::HELP_TITLE, lines, width, height);
    }
}
