//! 左面板（Profiles 模式）：profile 列表（ratatui List，滚动/高亮原生处理）。

use donn_core::KeyState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph};

use crate::tui::app::{Core, PaneId};
use crate::tui::components::{draw_list, list_hit_area, pad};
use crate::tui::i18n;

pub fn render(core: &mut Core, f: &mut Frame, area: Rect) {
    let focused = core.focus == PaneId::Left;
    let theme = core.theme;
    let title = i18n::fill(i18n::PROFILES_TITLE, &[&core.profiles.len().to_string()]);
    let block = theme.pane_block(title, focused);

    if core.profiles.is_empty() {
        core.hit.left_list = Rect::default();
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(vec![
                Line::default(),
                Line::from(Span::styled(i18n::NO_PROFILES, theme.dim())),
                Line::from(Span::styled(i18n::PRESS_A_TO_CREATE, theme.dim())),
            ]),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = core
        .profiles
        .items
        .iter()
        .map(|card| {
            let key_badge = if card.broken.is_some() {
                // 坏 profile 可见可诊断：选中后详情面板展示完整错误
                Span::styled(i18n::BADGE_BROKEN, theme.err())
            } else {
                match &card.key {
                    KeyState::Present { tail4 } => Span::styled(format!("···{tail4}"), theme.ok()),
                    KeyState::Absent => Span::styled(i18n::BADGE_NO_KEY, theme.err()),
                    KeyState::NotNeeded => Span::styled(i18n::BADGE_OAUTH, theme.dim()),
                }
            };
            ListItem::from(Line::from(vec![
                Span::raw(pad(&card.name, 14)),
                Span::styled(pad(&card.preset, 11), theme.dim()),
                key_badge,
            ]))
        })
        .collect();
    core.hit.left_list = list_hit_area(area);
    draw_list(
        f,
        area,
        items,
        theme.highlight(focused),
        &mut core.profiles.state,
        Some(block),
    );
}
