//! 全局设置面板：`S` 打开，右栏按分组编辑 donn 全局旋钮（类型化开关/选项/数值）
//! 与自由 env/settings 补充。每次改动 = 落盘 config.toml + 自动同步全部 profile。

use donn_core::keys::PERMISSION_MODES;
use donn_core::knobs::{BOOL_KNOBS, KnobGroup, VALUE_KNOBS, ValueKind};
use donn_core::{ConfigChange, Knobs};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

use crate::tui::app::Core;
use crate::tui::components::list::{Nav, SelectList};
use crate::tui::components::modal::{Modal, Prompt, Select};
use crate::tui::components::{draw_list, fit_spans, pad};
use crate::tui::i18n::{self, knob_label};

/// 标签列宽（`❯ ` 标记之外）。
const LABEL_WIDTH: usize = 30;

/// 可交互的设置行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 分组标题：不可选中。
    Section(&'static str),
    /// 开关旋钮：Enter 直接翻转。
    AgentTeams,
    ToolSearch,
    HideAttribution,
    Nonessential,
    /// 选项旋钮：Enter 弹选择框。
    PermissionMode,
    /// 数值旋钮：Enter 弹输入框。
    Timeout,
    /// 简单布尔旋钮（[`BOOL_KNOBS`] 下标）：Enter 翻转。
    Flag(usize),
    /// 简单标量旋钮（[`VALUE_KNOBS`] 下标）：枚举弹选择框，其余弹输入框。
    Value(usize),
    /// 自由补充。
    Env(String),
    AddEnv,
    Setting(String),
    AddSetting,
}

impl Row {
    fn is_section(&self) -> bool {
        matches!(self, Row::Section(_))
    }
}

/// 类型化旋钮所属分组（表驱动旋钮的分组写在表里）。
fn typed_rows(group: KnobGroup) -> &'static [Row] {
    match group {
        KnobGroup::Session => &[Row::PermissionMode, Row::AgentTeams, Row::ToolSearch],
        KnobGroup::Endpoint => &[Row::Timeout, Row::Nonessential],
        KnobGroup::Privacy => &[Row::HideAttribution],
    }
}

fn section_title(group: KnobGroup) -> &'static str {
    match group {
        KnobGroup::Session => i18n::CFG_SEC_SESSION,
        KnobGroup::Endpoint => i18n::CFG_SEC_ENDPOINT,
        KnobGroup::Privacy => i18n::CFG_SEC_PRIVACY,
    }
}

#[derive(Default)]
pub struct SettingsPane {
    pub rows: SelectList<Row>,
}

impl SettingsPane {
    pub fn new(donn: &donn_core::Donn) -> Result<Self, String> {
        let cfg = donn.config().map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        for group in KnobGroup::ALL {
            rows.push(Row::Section(section_title(group)));
            rows.extend(typed_rows(group).iter().cloned());
            rows.extend(
                VALUE_KNOBS
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| k.group == group)
                    .map(|(i, _)| Row::Value(i)),
            );
            rows.extend(
                BOOL_KNOBS
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| k.group == group)
                    .map(|(i, _)| Row::Flag(i)),
            );
        }
        rows.push(Row::Section(i18n::CFG_SEC_CUSTOM));
        rows.extend(cfg.defaults.env.keys().cloned().map(Row::Env));
        rows.push(Row::AddEnv);
        rows.extend(cfg.defaults.settings.keys().cloned().map(Row::Setting));
        rows.push(Row::AddSetting);
        let mut pane = Self {
            rows: SelectList::new(rows),
        };
        pane.settle_from(0, 1);
        Ok(pane)
    }

    /// 列表导航；落在分组标题上就继续往同方向走，到头则退回原位。
    pub fn nav(&mut self, nav: Nav) {
        let before = self.rows.selected();
        self.rows.nav(nav);
        let dir = match nav {
            Nav::By(d) if d < 0 => -1,
            _ => 1,
        };
        self.settle_from(before, dir);
    }

    /// 鼠标点击：点到分组标题时选中其下第一行。
    pub fn click(&mut self, rect: Rect, pos: Position) {
        let before = self.rows.selected();
        self.rows.click(rect, pos);
        self.settle_from(before, 1);
    }

    fn settle_from(&mut self, before: usize, dir: i32) {
        while self.rows.current().is_some_and(Row::is_section) {
            let i = self.rows.selected();
            let next = if dir > 0 {
                Some(i + 1).filter(|n| *n < self.rows.len())
            } else {
                i.checked_sub(1)
            };
            match next {
                Some(n) => self.rows.select(n),
                None => {
                    self.rows.select(before);
                    return;
                }
            }
        }
    }
}

/// 应用变更并汇报。
fn apply(core: &mut Core, change: ConfigChange) -> Result<(), String> {
    let report = core.donn.edit_config(change).map_err(|e| e.to_string())?;
    if let Some(pane) = core.settings.take() {
        match SettingsPane::new(&core.donn) {
            Ok(mut fresh) => {
                fresh.rows.select(pane.rows.selected());
                core.settings = Some(fresh);
            }
            Err(e) => core.status.error(e),
        }
    }
    if !report.errors.is_empty() {
        core.status.error(report.errors.join("; "));
    } else if !report.overwritten.is_empty() {
        let msg = i18n::fill(i18n::ST_OVERWROTE, &[&report.overwritten.join(", ")]);
        core.status.warn(msg);
    } else {
        let msg = i18n::fill(i18n::ST_CONFIG_SYNCED, &[&report.synced.to_string()]);
        core.status.ok(msg);
    }
    Ok(())
}

/// 开关翻转：新生效值若与默认一致则回到 None（跟随默认）。
fn toggle(field: &mut Option<bool>, default_on: bool) {
    let effective = field.unwrap_or(default_on);
    let next = !effective;
    *field = if next == default_on { None } else { Some(next) };
}

fn label(field: &str) -> String {
    knob_label(field).unwrap_or(field).to_string()
}

/// Enter：按行语义翻转/弹窗。
pub fn activate_row(core: &mut Core) -> Option<Box<dyn Modal>> {
    let pane = core.settings.as_ref()?;
    let row = pane.rows.current()?.clone();
    let cfg = match core.donn.config() {
        Ok(cfg) => cfg,
        Err(e) => {
            core.status.error(e.to_string());
            return None;
        }
    };
    let knobs = cfg.defaults.knobs.clone();
    match row {
        Row::Section(_) => None,
        // 开关旋钮：Enter 直接翻转，无需弹窗。
        // 默认值取自 Knobs::default() 的取值方法——唯一定义处在 core，不在这里重复
        Row::AgentTeams | Row::HideAttribution | Row::Nonessential => {
            let defaults = Knobs::default();
            let base = knobs;
            let mut value = base.clone();
            match row {
                Row::AgentTeams => toggle(&mut value.agent_teams, defaults.agent_teams_on()),
                Row::HideAttribution => {
                    toggle(&mut value.hide_attribution, defaults.hide_attribution_on());
                }
                Row::Nonessential => {
                    toggle(
                        &mut value.disable_nonessential_traffic,
                        defaults.disable_nonessential_traffic_on(),
                    );
                }
                _ => {}
            }
            if let Err(e) = apply(core, ConfigChange::Knobs { base, value }) {
                core.status.error(e);
            }
            None
        }
        // 三态：不写键（上游按端点主机判定）/ 强制开 / 关
        Row::ToolSearch => {
            let states = [None, Some(true), Some(false)];
            let options = vec![
                i18n::EFFORT_FOLLOW.to_string(),
                i18n::V_ON.to_string(),
                i18n::V_OFF.to_string(),
            ];
            let current = states
                .iter()
                .position(|s| *s == knobs.tool_search)
                .unwrap_or(0);
            Some(Box::new(Select::new(
                label("tool_search"),
                options,
                current,
                move |core, picked| {
                    let base = knobs;
                    let mut value = base.clone();
                    value.tool_search = states[picked];
                    if let Err(e) = apply(core, ConfigChange::Knobs { base, value }) {
                        core.status.error(e);
                    }
                },
            )))
        }
        Row::PermissionMode => {
            let effective = knobs.permission_mode().to_string();
            let mut options: Vec<String> =
                PERMISSION_MODES.iter().map(|m| (*m).to_string()).collect();
            if !PERMISSION_MODES.contains(&effective.as_str()) {
                options.insert(0, effective.clone());
            }
            let current = options.iter().position(|m| m == &effective).unwrap_or(0);
            Some(Box::new(Select::new(
                label("permission_mode"),
                options,
                current,
                move |core, picked| {
                    let base = knobs;
                    let mut value = base.clone();
                    let picked = if !PERMISSION_MODES.contains(&effective.as_str()) && picked == 0 {
                        effective
                    } else {
                        let offset = usize::from(!PERMISSION_MODES.contains(&effective.as_str()));
                        PERMISSION_MODES[picked - offset].to_string()
                    };
                    value.permission_mode =
                        (picked != Knobs::default().permission_mode()).then_some(picked);
                    if let Err(e) = apply(core, ConfigChange::Knobs { base, value }) {
                        core.status.error(e);
                    }
                },
            )))
        }
        Row::Flag(i) => {
            let knob = &BOOL_KNOBS[i];
            let base = knobs;
            let mut value = base.clone();
            let next = !base.flag(knob);
            value.set_extra(
                knob.field,
                (next != knob.claude_default).then(|| serde_json::json!(next)),
            );
            if let Err(e) = apply(core, ConfigChange::Knobs { base, value }) {
                core.status.error(e);
            }
            None
        }
        Row::Value(i) => {
            let knob = &VALUE_KNOBS[i];
            if let ValueKind::Enum(levels) = knob.kind {
                // 有限取值走选择框；第 0 项 = 跟随 Claude Code（删键）
                let mut options = vec![i18n::EFFORT_FOLLOW.to_string()];
                options.extend(levels.iter().map(|l| (*l).to_string()));
                let current = knobs
                    .value(knob)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|cur| levels.iter().position(|l| *l == cur))
                    .map_or(0, |i| i + 1);
                return Some(Box::new(Select::new(
                    label(knob.field),
                    options,
                    current,
                    move |core, picked| {
                        let base = knobs;
                        let mut value = base.clone();
                        value.set_extra(
                            knob.field,
                            picked
                                .checked_sub(1)
                                .and_then(|i| levels.get(i))
                                .map(|l| serde_json::json!(l)),
                        );
                        if let Err(e) = apply(core, ConfigChange::Knobs { base, value }) {
                            core.status.error(e);
                        }
                    },
                )));
            }
            let current = knobs
                .value(knob)
                .map(Knobs::scalar_text)
                .unwrap_or_default();
            Some(Box::new(
                Prompt::new(label(knob.field), current, false, move |core, raw| {
                    let raw = raw.trim();
                    let base = knobs.clone();
                    let mut value = base.clone();
                    let parsed = if raw.is_empty() {
                        None
                    } else if knob.kind == ValueKind::Number {
                        let n: u64 = raw.parse().map_err(|_| i18n::V_NUMBER_ERR.to_string())?;
                        Some(serde_json::json!(n))
                    } else {
                        Some(serde_json::json!(raw))
                    };
                    value.set_extra(knob.field, parsed);
                    apply(core, ConfigChange::Knobs { base, value })
                })
                .with_hint(i18n::CFG_VALUE_HINT),
            ))
        }
        Row::Timeout => Some(Box::new(
            Prompt::new(
                label("api_timeout_ms"),
                knobs.api_timeout().to_string(),
                false,
                move |core, value| {
                    let base = knobs.clone();
                    let mut next = base.clone();
                    let value = value.trim();
                    next.api_timeout_ms = if value.is_empty() {
                        None
                    } else {
                        let ms: u64 = value.parse().map_err(|_| i18n::V_NUMBER_ERR.to_string())?;
                        Some(ms).filter(|ms| *ms != Knobs::default().api_timeout())
                    };
                    apply(core, ConfigChange::Knobs { base, value: next })
                },
            )
            .with_hint(i18n::fill(
                i18n::CFG_BUILTIN_HINT,
                &[&Knobs::default().api_timeout().to_string()],
            )),
        )),
        Row::Env(key) => {
            let current = cfg.defaults.env.get(&key).cloned().unwrap_or_default();
            let title = i18n::fill(i18n::CFG_ROW_ENV, &[&key]);
            Some(Box::new(
                Prompt::new(title, current, false, move |core, value| {
                    let value = value.trim();
                    let change = if value.is_empty() {
                        ConfigChange::RemoveDefaultEnv(key.clone())
                    } else {
                        ConfigChange::SetDefaultEnv(key.clone(), value.to_string())
                    };
                    apply(core, change)
                })
                .with_hint(i18n::PROMPT_ENV_HINT),
            ))
        }
        Row::AddEnv => Some(Box::new(
            Prompt::new(i18n::CFG_ENV_ADD_TITLE, "", false, move |core, value| {
                let (key, val) = value
                    .split_once('=')
                    .ok_or_else(|| i18n::ENV_FORMAT_ERR.to_string())?;
                apply(
                    core,
                    ConfigChange::SetDefaultEnv(key.trim().into(), val.trim().into()),
                )
            })
            .with_hint(i18n::ENV_FORMAT_ERR),
        )),
        Row::Setting(key) => {
            let current = cfg
                .defaults
                .settings
                .get(&key)
                .map(|v| v.to_string())
                .unwrap_or_default();
            let title = i18n::fill(i18n::CFG_ROW_SETTING, &[&key]);
            Some(Box::new(
                Prompt::new(title, current, false, move |core, value| {
                    let value = value.trim();
                    let change = if value.is_empty() {
                        ConfigChange::RemoveDefaultSetting(key.clone())
                    } else {
                        ConfigChange::SetDefaultSetting(key.clone(), parse_value(value))
                    };
                    apply(core, change)
                })
                .with_hint(i18n::CFG_SETTING_HINT),
            ))
        }
        Row::AddSetting => Some(Box::new(
            Prompt::new(
                i18n::CFG_SETTING_ADD_TITLE,
                "",
                false,
                move |core, value| {
                    let (key, val) = value
                        .split_once('=')
                        .ok_or_else(|| i18n::CFG_SETTING_FORMAT_ERR.to_string())?;
                    let val = val.trim();
                    if val.is_empty() {
                        return Err(i18n::CFG_SETTING_FORMAT_ERR.to_string());
                    }
                    apply(
                        core,
                        ConfigChange::SetDefaultSetting(key.trim().into(), parse_value(val)),
                    )
                },
            )
            .with_hint(i18n::CFG_SETTING_HINT),
        )),
    }
}

/// 值解析：合法 JSON 按 JSON（false / 3 / {"a":1}），否则按字符串。
fn parse_value(raw: &str) -> serde_json::Value {
    let raw = raw.trim();
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_string()))
}

pub fn render(core: &mut Core, f: &mut Frame, area: Rect) {
    let theme = core.theme;
    let block = theme.pane_block(i18n::CFG_TITLE, true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(pane) = &mut core.settings else {
        return;
    };
    let cfg = match core.donn.config() {
        Ok(cfg) => cfg,
        Err(e) => {
            f.render_widget(Paragraph::new(e.to_string()), inner);
            return;
        }
    };
    let knobs = &cfg.defaults.knobs;

    // 说明行按面板宽度换行，窄终端不截断
    let header = Paragraph::new(Line::from(Span::styled(
        i18n::CFG_INTRO.to_string(),
        theme.dim(),
    )))
    .wrap(Wrap { trim: false });
    let header_height = header.line_count(inner.width) as u16 + 1;
    let [header_area, list_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(header_height),
        ratatui::layout::Constraint::Min(1),
    ])
    .areas(inner);
    f.render_widget(header, header_area);

    // 开关值渲染：开/关
    let on_off = |value: bool, inherited: bool| -> Vec<Span<'static>> {
        let mut spans = vec![if value {
            Span::styled(i18n::V_ON.to_string(), theme.ok())
        } else {
            Span::styled(i18n::V_OFF.to_string(), theme.dim())
        }];
        if inherited {
            spans.push(Span::styled(
                format!("  {}", i18n::FROM_DEFAULT),
                theme.dim(),
            ));
        }
        spans
    };
    let default_tag = || Span::styled(format!("  {}", i18n::FROM_DEFAULT), theme.dim());
    let value_width = (list_area.width as usize).saturating_sub(2 + LABEL_WIDTH);
    let section_style = theme.accent().add_modifier(Modifier::BOLD);

    let items: Vec<ListItem> = pane
        .rows
        .items
        .iter()
        .map(|row| {
            let (label_text, value): (String, Vec<Span>) = match row {
                Row::Section(title) => {
                    return ListItem::from(Line::from(Span::styled(
                        (*title).to_string(),
                        section_style,
                    )));
                }
                Row::AgentTeams => (
                    label("agent_teams"),
                    on_off(knobs.agent_teams_on(), knobs.agent_teams.is_none()),
                ),
                Row::ToolSearch => (
                    label("tool_search"),
                    match knobs.tool_search {
                        Some(value) => on_off(value, false),
                        None => vec![Span::styled(i18n::EFFORT_FOLLOW.to_string(), theme.dim())],
                    },
                ),
                Row::HideAttribution => (
                    label("hide_attribution"),
                    on_off(
                        knobs.hide_attribution_on(),
                        knobs.hide_attribution.is_none(),
                    ),
                ),
                Row::Nonessential => (
                    label("disable_nonessential_traffic"),
                    on_off(
                        knobs.disable_nonessential_traffic_on(),
                        knobs.disable_nonessential_traffic.is_none(),
                    ),
                ),
                Row::PermissionMode => {
                    let mut spans = vec![Span::styled(
                        knobs.permission_mode().to_string(),
                        theme.accent(),
                    )];
                    if knobs.permission_mode.is_none() {
                        spans.push(default_tag());
                    }
                    (label("permission_mode"), spans)
                }
                Row::Timeout => {
                    let mut spans = vec![Span::raw(knobs.api_timeout().to_string())];
                    if knobs.api_timeout_ms.is_none() {
                        spans.push(default_tag());
                    }
                    (label("api_timeout_ms"), spans)
                }
                Row::Flag(i) => {
                    let knob = &BOOL_KNOBS[*i];
                    (
                        label(knob.field),
                        on_off(knobs.flag(knob), !knobs.is_explicit(knob.field)),
                    )
                }
                Row::Value(i) => {
                    let knob = &VALUE_KNOBS[*i];
                    let value = match knobs.value(knob) {
                        Some(v) if matches!(knob.kind, ValueKind::Enum(_)) => {
                            Span::styled(Knobs::scalar_text(v), theme.accent())
                        }
                        Some(v) => Span::raw(Knobs::scalar_text(v)),
                        None => Span::styled(i18n::FROM_DEFAULT.to_string(), theme.dim()),
                    };
                    (label(knob.field), vec![value])
                }
                Row::Env(key) => {
                    let value = cfg.defaults.env.get(key).cloned().unwrap_or_default();
                    (
                        i18n::fill(i18n::CFG_ROW_ENV, &[key]),
                        vec![Span::raw(value)],
                    )
                }
                Row::AddEnv => (
                    i18n::CFG_ENV_ADD.into(),
                    vec![Span::styled(i18n::ADD_ELLIPSIS.to_string(), theme.dim())],
                ),
                Row::Setting(key) => {
                    let value = cfg
                        .defaults
                        .settings
                        .get(key)
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    (
                        i18n::fill(i18n::CFG_ROW_SETTING, &[key]),
                        vec![Span::raw(value)],
                    )
                }
                Row::AddSetting => (
                    i18n::CFG_SETTING_ADD.into(),
                    vec![Span::styled(i18n::ADD_ELLIPSIS.to_string(), theme.dim())],
                ),
            };
            let mut spans = vec![Span::styled(pad(&label_text, LABEL_WIDTH), theme.dim())];
            spans.extend(fit_spans(value, value_width));
            ListItem::from(Line::from(spans))
        })
        .collect();
    core.hit.detail_list = list_area;
    draw_list(
        f,
        list_area,
        items,
        theme.selected(),
        &mut pane.rows.state,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use donn_core::{Donn, DonnHome};

    fn pane() -> SettingsPane {
        let dir = tempfile::tempdir().unwrap();
        let donn = Donn::with_home(DonnHome::for_test(dir.path())).unwrap();
        SettingsPane::new(&donn).unwrap()
    }

    #[test]
    fn navigation_never_lands_on_a_section_title() {
        let mut pane = pane();
        assert!(!pane.rows.current().unwrap().is_section(), "initial row");
        let n = pane.rows.len();
        for _ in 0..n + 2 {
            pane.nav(Nav::By(1));
            assert!(!pane.rows.current().unwrap().is_section());
        }
        for _ in 0..n + 2 {
            pane.nav(Nav::By(-1));
            assert!(!pane.rows.current().unwrap().is_section());
        }
        assert_eq!(
            pane.rows.selected(),
            1,
            "moving up past the first section stays put"
        );
        pane.nav(Nav::Top);
        assert_eq!(pane.rows.selected(), 1);
        pane.nav(Nav::Bottom);
        assert_eq!(pane.rows.current(), Some(&Row::AddSetting));
    }

    #[test]
    fn every_knob_is_listed_exactly_once_under_a_section() {
        let pane = pane();
        let flags = pane
            .rows
            .items
            .iter()
            .filter(|r| matches!(r, Row::Flag(_)))
            .count();
        let values = pane
            .rows
            .items
            .iter()
            .filter(|r| matches!(r, Row::Value(_)))
            .count();
        assert_eq!(flags, BOOL_KNOBS.len());
        assert_eq!(values, VALUE_KNOBS.len());
        assert!(matches!(pane.rows.items[0], Row::Section(_)));
        let sections = pane.rows.items.iter().filter(|r| r.is_section()).count();
        assert_eq!(sections, KnobGroup::ALL.len() + 1);
    }
}
