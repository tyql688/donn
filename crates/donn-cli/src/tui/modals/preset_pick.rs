//! 渠道选择弹窗：可模糊过滤的 preset 列表（Add 表单 Tab 唤起）。
//! 匹配用 nucleo-matcher（helix 同款）：`kcn` 能命中 `kimi-cn`，按得分排序。

use crossterm::event::{KeyCode, KeyEvent};
use donn_core::preset::Preset;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::tui::app::Core;
use crate::tui::components::fuzzy_rank;
use crate::tui::components::modal::{Modal, ModalOutcome, render_box};
use crate::tui::components::text_input::TextInput;
use crate::tui::i18n;

pub struct PresetPick {
    presets: Vec<Preset>,
    filter: TextInput,
    selected: usize,
}

impl PresetPick {
    pub fn new(presets: Vec<Preset>, current_key: &str) -> Self {
        let selected = presets
            .iter()
            .position(|p| p.key == current_key)
            .unwrap_or(0);
        Self {
            presets,
            filter: TextInput::default(),
            selected,
        }
    }

    /// 模糊过滤：对 "key label description" 整体打分，按得分降序。
    /// 空输入 = 全量原序。
    fn matches(&self) -> Vec<&Preset> {
        let needle = self.filter.value().trim();
        if needle.is_empty() {
            return self.presets.iter().collect();
        }
        let needle_lower = needle.to_lowercase();
        let ranked = fuzzy_rank(
            needle,
            self.presets
                .iter()
                .map(|p| format!("{} {} {}", p.key, p.label, p.description)),
        );
        // key 精确/前缀命中排在纯模糊命中前面
        let mut scored: Vec<(u8, u32, usize, &Preset)> = ranked
            .into_iter()
            .map(|(score, index)| {
                let p = &self.presets[index];
                let key_lower = p.key.to_lowercase();
                let priority = if key_lower == needle_lower {
                    0
                } else if key_lower.starts_with(&needle_lower) {
                    1
                } else if p.label.to_lowercase().starts_with(&needle_lower) {
                    2
                } else {
                    3
                };
                (priority, score, index, p)
            })
            .collect();
        scored.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        scored.into_iter().map(|(_, _, _, p)| p).collect()
    }
}

impl Modal for PresetPick {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome {
        let count = self.matches().len();
        match key.code {
            KeyCode::Esc => ModalOutcome::Close,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                ModalOutcome::Keep
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
                ModalOutcome::Keep
            }
            KeyCode::Enter => {
                let picked = self.matches().get(self.selected).map(|p| p.key.clone());
                if let (Some(key), Some(form)) = (picked, core.add.as_mut()) {
                    form.set_provider_key(&key);
                }
                ModalOutcome::Close
            }
            _ => {
                if self.filter.handle_key(&key) {
                    self.selected = 0;
                }
                ModalOutcome::Keep
            }
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        let matches = self.matches();
        let visible = 12usize;
        let width = 62.min(area.width.saturating_sub(4)).max(30).min(area.width);
        let height = (matches.len().min(visible) as u16 + 4).min(area.height);

        let mut lines = vec![
            self.filter
                .render_line(true, width.saturating_sub(4) as usize),
            Line::from(Span::styled(
                i18n::fill(i18n::PICK_PROVIDER_HINT, &[&matches.len().to_string()]),
                theme.dim(),
            )),
        ];
        let offset = self.selected.saturating_sub(visible.saturating_sub(1));
        for (i, preset) in matches.iter().enumerate().skip(offset).take(visible) {
            let is_sel = i == self.selected;
            let marker = if is_sel { "❯ " } else { "  " };
            let key_style = if is_sel {
                theme.selected()
            } else {
                theme.accent()
            };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), theme.accent()),
                Span::styled(format!("{:<12}", preset.key), key_style),
                Span::raw(preset.label.clone()),
                Span::styled(format!("  {}", preset.description), theme.dim()),
            ]));
        }
        render_box(
            f,
            area,
            theme,
            i18n::PICK_PROVIDER_TITLE,
            lines,
            width,
            height,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_key_and_key_prefix_rank_ahead_of_description_matches() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = donn_core::PresetCatalog::load(&donn_core::DonnHome::for_test(dir.path()));
        let mut picker = PresetPick::new(catalog.all().to_vec(), "ant-ling");

        picker.filter.set("official");
        assert_eq!(picker.matches()[0].key, "official");

        picker.filter.set("kimi");
        assert!(picker.matches()[0].key.starts_with("kimi"));
        assert!(picker.matches()[1].key.starts_with("kimi"));
    }
}
