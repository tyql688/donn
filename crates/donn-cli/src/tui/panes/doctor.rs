//! 底部抽屉：doctor 检查结果。

use donn_core::doctor::{self, Check};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

use crate::tui::app::{Core, PaneId};
use crate::tui::components::list::SelectList;
use crate::tui::components::{draw_list, list_hit_area};
use crate::tui::i18n;

pub struct DoctorPane {
    pub checks: SelectList<Check>,
}

impl DoctorPane {
    pub fn run(donn: &donn_core::Donn) -> Self {
        Self {
            checks: SelectList::new(doctor::run(donn)),
        }
    }
}

pub fn render(core: &mut Core, f: &mut Frame, area: Rect) {
    let focused = core.focus == PaneId::Doctor;
    let theme = core.theme;
    let Some(doctor_pane) = &mut core.doctor else {
        return;
    };
    let failing = doctor_pane.checks.items.iter().filter(|c| !c.ok).count();
    let total = doctor_pane.checks.len();
    let title = if failing == 0 {
        i18n::fill(i18n::DOCTOR_ALL_PASS, &[&total.to_string()])
    } else {
        i18n::fill(
            i18n::DOCTOR_FAILING,
            &[&failing.to_string(), &total.to_string()],
        )
    };
    let block = theme.pane_block(title, focused);

    let items: Vec<ListItem> = doctor_pane
        .checks
        .items
        .iter()
        .map(|check| {
            let (mark, mark_style) = if check.ok {
                ("✓", theme.ok())
            } else {
                ("✗", theme.err())
            };
            let mut spans = vec![
                Span::styled(format!("{mark} "), mark_style),
                Span::raw(format!("{}: ", check.title)),
                Span::styled(check.detail.clone(), theme.dim()),
            ];
            if let Some(fix) = &check.fix {
                spans.push(Span::styled(format!("  fix: {fix}"), theme.warn()));
            }
            ListItem::from(Line::from(spans))
        })
        .collect();
    core.hit.doctor_list = list_hit_area(area);
    let checks = &mut doctor_pane.checks.state;
    draw_list(
        f,
        area,
        items,
        theme.highlight(focused),
        checks,
        Some(block),
    );
}
