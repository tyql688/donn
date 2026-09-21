//! 模型选择：搜 / 选 / 手填（全 provider 同一套）。
//!
//! UI 只改模型槽。上下文套餐由 `render()` 按 sonnet 生效 id 注入；用户在 env 行
//! 自己写的 `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 等键属于 profile 意图，优先于套餐，换模型不动它。

use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent};
use donn_core::SpecChange;
use donn_core::keys::ModelSlot;
use donn_core::preset::{ModelChoice, Preset};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::tui::app::Core;
use crate::tui::components::modal::{Modal, ModalOutcome, render_box};
use crate::tui::components::text_input::TextInput;
use crate::tui::components::{fit_right, fuzzy_rank, pad};
use crate::tui::i18n;

pub enum Pick {
    Follow,
    /// 空串等同 Follow。
    Id(String),
}

/// 候选 = model_choices ∪ 槽位默认 id。
pub fn collect_choices(preset: &Preset) -> Vec<ModelChoice> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for choice in &preset.model_choices {
        if !choice.id.is_empty() && seen.insert(choice.id.clone()) {
            out.push(choice.clone());
        }
    }
    for slot in ModelSlot::ALL {
        if let Some(id) = preset.models.get(slot)
            && !id.is_empty()
            && seen.insert(id.to_string())
        {
            out.push(ModelChoice {
                id: id.to_string(),
                ..Default::default()
            });
        }
    }
    out
}

/// preset 叠上用户自己定义过窗口的模型。选择器候选、「要不要问窗口」都看合并后的这份：
/// 定义过的自定义模型和 preset 候选一样带着窗口出现，同 id 的条目压过 preset 的数字。
pub fn with_custom_models(
    preset: &Preset,
    windows: &std::collections::BTreeMap<String, u64>,
) -> Preset {
    let mut merged = preset.clone();
    for (id, tokens) in windows {
        match merged.model_choices.iter_mut().find(|c| &c.id == id) {
            Some(choice) => choice.max_context = Some(*tokens),
            None => merged.model_choices.push(ModelChoice {
                id: id.clone(),
                label: Some(format!("custom · {tokens} tokens")),
                max_context: Some(*tokens),
                ..Default::default()
            }),
        }
    }
    merged
}

/// 这个 id 的上下文窗口没人知道：preset 候选里没有它的窗口，Claude Code 也不认识
/// （带 `claude-` 的它认识）。这种 sonnet 需要用户自己给窗口。
pub fn needs_window(preset: &Preset, id: &str) -> bool {
    !id.is_empty()
        && !donn_core::preset::is_claude_model(id)
        && preset
            .choice_for_model(id)
            .and_then(|choice| choice.max_context)
            .is_none()
}

fn model_change(preset: &Preset, slot: ModelSlot, id: &str) -> SpecChange {
    if preset.models.get(slot) == Some(id) {
        SpecChange::Model(slot, None)
    } else {
        SpecChange::Model(slot, Some(id.to_string()))
    }
}

/// 匹配 catalog 则用官方 id 大小写；空 → Follow。
pub fn resolve_id(preset: &Preset, raw: &str) -> Pick {
    let v = raw.trim();
    if v.is_empty() {
        return Pick::Follow;
    }
    if let Some(c) = collect_choices(preset)
        .into_iter()
        .find(|c| c.id == v || c.id.eq_ignore_ascii_case(v))
    {
        Pick::Id(c.id)
    } else {
        Pick::Id(v.to_string())
    }
}

/// Pick → SpecChange（详情写盘）。只写目标槽位，不碰 env。
pub fn to_change(preset: &Preset, slot: ModelSlot, pick: Pick) -> SpecChange {
    match pick {
        Pick::Follow => SpecChange::Model(slot, None),
        Pick::Id(id) => match resolve_id(preset, &id) {
            Pick::Follow => SpecChange::Model(slot, None),
            Pick::Id(id) => model_change(preset, slot, &id),
        },
    }
}

// ── 弹窗 ──────────────────────────────────────────────────────────

#[derive(Clone)]
enum Row {
    Follow,
    Choice(usize),
    UseInput,
}

/// 返回 `Some` = 还有后续要问，用它替换选择器。
type OnDone = Box<dyn FnOnce(&mut Core, Pick) -> Option<Box<dyn Modal>>>;

fn compact_name(choice: &ModelChoice) -> &str {
    choice
        .display()
        .split_once(" · ")
        .map_or(choice.display(), |(name, _)| name)
}

fn detail(choice: &ModelChoice) -> Option<String> {
    choice
        .display()
        .split_once(" · ")
        .map(|(_, detail)| detail.to_string())
        .or_else(|| choice.max_context.map(compact_context))
}

fn compact_context(value: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if value.is_multiple_of(MIB) {
        format!("{}M context", value / MIB)
    } else if value.is_multiple_of(KIB) {
        format!("{}K context", value / KIB)
    } else {
        format!("{value} context")
    }
}

fn name_column_width(
    choices: &[ModelChoice],
    follow_label: &str,
    follow_id: &str,
    row_width: usize,
) -> usize {
    let widest_name = choices
        .iter()
        .filter_map(|choice| {
            let name = compact_name(choice);
            (name != choice.id.as_str()).then_some(name.width())
        })
        .chain(std::iter::once(follow_label.width()))
        .max()
        .unwrap_or_default();
    let widest_id = choices
        .iter()
        .map(|choice| choice.id.width())
        .chain(std::iter::once(follow_id.width()))
        .max()
        .unwrap_or_default();
    let minimum_name = follow_label.width().min(row_width);
    let reserved_id = widest_id.min(row_width.saturating_sub(minimum_name + 2));
    widest_name.min(row_width.saturating_sub(reserved_id + 2))
}

fn aligned_row(name: &str, id: &str, name_width: usize, row_width: usize) -> String {
    let name = fit_right(name, name_width);
    fit_right(&format!("{}  {id}", pad(&name, name_width)), row_width)
}

fn choice_row(choice: &ModelChoice, name_width: usize, row_width: usize) -> String {
    let name = compact_name(choice);
    if name == choice.id {
        return aligned_row("", &choice.id, name_width, row_width);
    }
    aligned_row(name, &choice.id, name_width, row_width)
}

struct Picker {
    title: String,
    default_label: String,
    choices: Vec<ModelChoice>,
    input: TextInput,
    selected: usize,
    on_done: Option<OnDone>,
}

impl Picker {
    fn rows_for(needle: &str, choices: &[ModelChoice], default_label: &str) -> Vec<Row> {
        let needle = needle.trim();
        let mut rows = Vec::new();
        let lower = needle.to_lowercase();
        if needle.is_empty() || lower == "follow" || default_label.to_lowercase().contains(&lower) {
            rows.push(Row::Follow);
        }
        if needle.is_empty() {
            rows.extend((0..choices.len()).map(Row::Choice));
        } else {
            let scored = fuzzy_rank(
                needle,
                choices.iter().map(|c| format!("{} {}", c.id, c.display())),
            );
            rows.extend(scored.into_iter().map(|(_, i)| Row::Choice(i)));
            if !choices
                .iter()
                .any(|c| c.id == needle || c.id.eq_ignore_ascii_case(needle))
            {
                rows.push(Row::UseInput);
            }
        }
        if rows.is_empty() {
            rows.push(Row::UseInput);
        }
        rows
    }

    fn rows(&self) -> Vec<Row> {
        Self::rows_for(self.input.value(), &self.choices, &self.default_label)
    }

    fn commit(&mut self, core: &mut Core) -> ModalOutcome {
        let rows = self.rows();
        let i = self.selected.min(rows.len().saturating_sub(1));
        let pick = match rows.get(i) {
            Some(Row::Follow) | None => Pick::Follow,
            Some(Row::Choice(ci)) => Pick::Id(self.choices[*ci].id.clone()),
            Some(Row::UseInput) => Pick::Id(self.input.value().trim().to_string()),
        };
        match self.on_done.take().and_then(|done| done(core, pick)) {
            Some(next) => ModalOutcome::Replace(next),
            None => ModalOutcome::Close,
        }
    }
}

impl Modal for Picker {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome {
        let n = self.rows().len();
        match key.code {
            KeyCode::Esc => ModalOutcome::Close,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                ModalOutcome::Keep
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(n.saturating_sub(1));
                ModalOutcome::Keep
            }
            KeyCode::Enter => self.commit(core),
            _ => {
                if self.input.handle_key(&key) {
                    self.selected = 0;
                }
                ModalOutcome::Keep
            }
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        let rows = self.rows();
        let vis = 12usize;
        let width = 56.min(area.width.saturating_sub(4)).max(36).min(area.width);
        let height = (rows.len().min(vis) as u16 + 5).min(area.height);
        let content_width = width.saturating_sub(4) as usize;
        let row_width = content_width.saturating_sub(2);
        let name_width = name_column_width(
            &self.choices,
            i18n::MODEL_FOLLOW_PRESET,
            &self.default_label,
            row_width,
        );
        let mut lines = vec![
            self.input.render_line(true, content_width),
            Line::from(Span::styled(i18n::MODEL_PICK_HINT, theme.dim())),
        ];
        let off = self.selected.saturating_sub(vis.saturating_sub(1));
        for (i, row) in rows.iter().enumerate().skip(off).take(vis) {
            let sel = i == self.selected;
            let style = if sel {
                theme.selected()
            } else {
                ratatui::style::Style::default()
            };
            let label = match row {
                Row::Follow => aligned_row(
                    i18n::MODEL_FOLLOW_PRESET,
                    &self.default_label,
                    name_width,
                    row_width,
                ),
                Row::Choice(ci) => choice_row(&self.choices[*ci], name_width, row_width),
                Row::UseInput => {
                    let q = self.input.value().trim();
                    if q.is_empty() {
                        i18n::MODEL_CUSTOM.to_string()
                    } else {
                        i18n::fill(i18n::MODEL_USE_INPUT, &[q])
                    }
                }
            };
            lines.push(Line::from(vec![
                Span::styled(if sel { "❯ " } else { "  " }, theme.accent()),
                Span::styled(label, style),
            ]));
        }
        let selected = rows.get(self.selected.min(rows.len().saturating_sub(1)));
        let preview = match selected {
            Some(Row::Choice(i)) => detail(&self.choices[*i]),
            _ => None,
        }
        .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            fit_right(&preview, content_width),
            theme.dim(),
        )));
        render_box(
            f,
            area,
            theme,
            format!(" {} ", self.title),
            lines,
            width,
            height,
        );
    }
}

/// 打开选择器。输入框空（便于搜）；`current_override` 只用于高亮当前项。
pub fn open(
    title: String,
    preset: &Preset,
    slot: ModelSlot,
    current_override: &str,
    on_done: impl FnOnce(&mut Core, Pick) -> Option<Box<dyn Modal>> + 'static,
) -> Box<dyn Modal> {
    let default_label = preset
        .models
        .get(slot)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| i18n::MODEL_NONE.to_string());
    let choices = collect_choices(preset);
    // 空 needle = 完整列表（含 Follow），不把当前 id 预填进过滤框
    let rows = Picker::rows_for("", &choices, &default_label);
    let selected = if current_override.is_empty() {
        0
    } else {
        rows.iter()
            .position(|r| match r {
                Row::Choice(i) => choices.get(*i).is_some_and(|c| c.id == current_override),
                _ => false,
            })
            .unwrap_or(0)
    };
    Box::new(Picker {
        title,
        default_label,
        choices,
        input: TextInput::new(""),
        selected,
        on_done: Some(Box::new(on_done)),
    })
}

/// 表单侧只写当前槽；每槽独立编辑，即使 preset 各槽默认相同也不联动。
pub fn apply_to_slots(
    preset: &Preset,
    slots_ui: &mut [TextInput; ModelSlot::ALL.len()],
    slot: ModelSlot,
    pick: Pick,
) {
    let pick = match pick {
        Pick::Id(id) => resolve_id(preset, &id),
        other => other,
    };
    match pick {
        Pick::Follow => {
            slots_ui[slot.index()].set(String::new());
        }
        Pick::Id(id) => {
            slots_ui[slot.index()].set(if preset.models.get(slot) == Some(id.as_str()) {
                String::new()
            } else {
                id
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use donn_core::keys::SlotMap;

    fn kimi() -> Preset {
        let mut models = SlotMap::default();
        for s in ModelSlot::ALL {
            models.set(s, Some("k3[1m]".into()));
        }
        Preset {
            models,
            model_choices: vec![
                ModelChoice {
                    id: "k3[1m]".into(),
                    max_context: Some(1_048_576),
                    ..Default::default()
                },
                ModelChoice {
                    id: "k3".into(),
                    max_context: Some(262_144),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn minimax_like() -> Preset {
        let mut models = SlotMap::default();
        models.set(ModelSlot::Sonnet, Some("M3".into()));
        models.set(ModelSlot::Haiku, Some("M2".into()));
        Preset {
            models,
            model_choices: vec![
                ModelChoice {
                    id: "M3".into(),
                    max_context: Some(1_000_000),
                    ..Default::default()
                },
                ModelChoice {
                    id: "M2".into(),
                    max_context: Some(204_800),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn only_ids_nobody_knows_the_window_of_need_one() {
        let p = kimi();
        assert!(!needs_window(&p, "k3[1m]"), "preset 候选自带窗口");
        assert!(!needs_window(&p, "claude-sonnet-5"), "Claude Code 认识");
        assert!(!needs_window(&p, "anthropic/claude-opus-5"));
        assert!(!needs_window(&p, ""));
        assert!(needs_window(&p, "my-gateway/model-x"));
    }

    #[test]
    fn picking_a_model_touches_only_that_slot() {
        let p = kimi();
        assert!(matches!(
            to_change(&p, ModelSlot::Sonnet, Pick::Id("k3".into())),
            SpecChange::Model(ModelSlot::Sonnet, Some(id)) if id == "k3"
        ));
        // 选中 preset 默认值 = 清除覆盖，跟随 preset
        assert!(matches!(
            to_change(&p, ModelSlot::Sonnet, Pick::Id("k3[1m]".into())),
            SpecChange::Model(ModelSlot::Sonnet, None)
        ));
        assert!(matches!(
            to_change(&p, ModelSlot::Haiku, Pick::Follow),
            SpecChange::Model(ModelSlot::Haiku, None)
        ));
        let mm = minimax_like();
        assert!(matches!(
            to_change(&mm, ModelSlot::Haiku, Pick::Id("M2".into())),
            SpecChange::Model(ModelSlot::Haiku, None)
        ));
    }

    #[test]
    fn freeform_case_and_custom() {
        let p = kimi();
        assert!(matches!(resolve_id(&p, "K3"), Pick::Id(id) if id == "k3"));
        assert!(matches!(
            to_change(&p, ModelSlot::Sonnet, Pick::Id("custom".into())),
            SpecChange::Model(ModelSlot::Sonnet, Some(id)) if id == "custom"
        ));
    }

    #[test]
    fn choice_collection_deduplicates_ids() {
        let mut p = kimi();
        p.model_choices.push(p.model_choices[0].clone());
        assert_eq!(
            collect_choices(&p)
                .iter()
                .filter(|choice| choice.id == "k3[1m]")
                .count(),
            1
        );
    }

    #[test]
    fn picker_row_keeps_identity_and_moves_metadata_to_preview() {
        let choice = ModelChoice {
            id: "k3[1m]".into(),
            label: Some("K3 · 1M (Allegretto+; Claude Code form)".into()),
            max_context: Some(1_048_576),
            ..Default::default()
        };

        assert_eq!(choice_row(&choice, 2, 50), "K3  k3[1m]");
        assert_eq!(
            detail(&choice).as_deref(),
            Some("1M (Allegretto+; Claude Code form)")
        );
    }

    #[test]
    fn picker_rows_and_preview_do_not_exceed_their_width() {
        let choice = ModelChoice {
            id: "kimi-for-coding-highspeed".into(),
            label: Some("K2.7 Code HighSpeed · 256K (Allegretto+)".into()),
            ..Default::default()
        };

        assert!(choice_row(&choice, 7, 24).width() <= 24);
        assert!(fit_right(detail(&choice).as_deref().unwrap_or_default(), 12).width() <= 12);
    }

    #[test]
    fn picker_model_ids_share_one_column() {
        let choices = vec![
            ModelChoice {
                id: "MiniMax-M3[1m]".into(),
                label: Some("M3 · 1M".into()),
                ..Default::default()
            },
            ModelChoice {
                id: "MiniMax-M2.7-highspeed".into(),
                label: Some("M2.7 HighSpeed · 200K".into()),
                ..Default::default()
            },
        ];
        let name_width = name_column_width(&choices, "default", "MiniMax-M3[1m]", 50);
        let default = aligned_row("default", "MiniMax-M3[1m]", name_width, 50);
        let m3 = choice_row(&choices[0], name_width, 50);
        let fast = choice_row(&choices[1], name_width, 50);

        let column = default.find("MiniMax").unwrap();
        assert_eq!(m3.find("MiniMax"), Some(column));
        assert_eq!(fast.find("MiniMax"), Some(column));
    }
}
