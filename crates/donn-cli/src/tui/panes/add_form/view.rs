//! Add 表单渲染：选渠道列表/详情 + 填表预览。状态与按键逻辑在父模块。

use donn_core::Isolation;
use donn_core::keys::ModelSlot;
use donn_core::preset::AuthMode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};

use super::{AddForm, Field};
use crate::tui::app::{Core, PaneId};
use crate::tui::components::{draw_list, fit_left, fit_right, list_hit_area, pad};
use crate::tui::i18n;

/// Add 时左栏：渠道列表。
pub fn render_providers(form: &mut AddForm, core: &mut Core, f: &mut Frame, area: Rect) {
    let focused = core.focus == PaneId::Left;
    let theme = core.theme;
    let n = form.providers().items.len();
    let block = theme.pane_block(
        i18n::fill(i18n::PROVIDERS_TITLE, &[&n.to_string()]),
        focused,
    );
    let items: Vec<ListItem> = form
        .providers()
        .items
        .iter()
        .map(|p| {
            ListItem::from(Line::from(vec![
                Span::raw(pad(&p.key, 14)),
                Span::styled(format!("[{}]", p.source.label()), theme.dim()),
            ]))
        })
        .collect();
    core.hit.left_list = list_hit_area(area);
    draw_list(
        f,
        area,
        items,
        theme.highlight(focused),
        &mut form.providers_mut().state,
        Some(block),
    );
}

/// Add 选渠道时右栏：当前渠道只读摘要。
pub fn render_provider_detail(form: &AddForm, core: &Core, f: &mut Frame, area: Rect) {
    let theme = &core.theme;
    let block = theme.pane_block(i18n::PROVIDER_PANE_TITLE, false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(p) = form.providers().current() else {
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(p.label.clone(), theme.accent()),
            Span::raw("  "),
            Span::styled(p.description.clone(), theme.dim()),
        ]),
        Line::default(),
        field(theme, "base_url", p.base_url.as_deref().unwrap_or("-")),
        field(theme, "auth", p.auth_mode.label()),
    ];
    if let Some(url) = &p.key_url {
        lines.push(field(theme, i18n::PROVIDER_GET_KEY, url));
    }
    if !p.models.is_empty() {
        lines.push(Line::default());
        for slot in ModelSlot::ALL {
            if let Some(model) = p.models.get(slot) {
                lines.push(field(theme, slot.label(), model));
            }
        }
    }
    let choices = crate::tui::modals::model_pick::collect_choices(p);
    if !choices.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("{} ({})", i18n::PV_MODELS, choices.len()),
            theme.dim(),
        )));
        lines.extend(
            choices
                .into_iter()
                .map(|choice| Line::from(vec![Span::raw("  "), Span::raw(choice.id)])),
        );
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn field<'a>(theme: &crate::tui::theme::Theme, label: &str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{} ", pad(label, 12)), theme.dim()),
        Span::raw(value.to_string()),
    ])
}

/// 右栏渲染：表单区 + 底部生效预览。`form` 从 Core 里取出传入（借用分离）。
pub fn render(form: &mut AddForm, core: &Core, f: &mut Frame, area: Rect) {
    let theme = core.theme;
    let block = theme.pane_block(i18n::ADD_TITLE, true);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let fields = form.fields();
    let current = form.current_field();
    // 输入区可用宽度：面板内宽 − 标记(2) − 标签(18) − 余量
    let input_width = (inner.width as usize).saturating_sub(24);
    let mut lines: Vec<Line> = Vec::new();

    // 头部：已选渠道（只读；esc 返回重选）
    let provider = form.provider().clone();
    lines.push(Line::from(vec![
        Span::styled(pad(i18n::F_PROVIDER, 20), theme.dim()),
        Span::styled(provider.key.clone(), theme.accent()),
        Span::styled(format!("  {}", provider.label), theme.dim()),
    ]));
    lines.push(Line::default());

    for field in &fields {
        let is_sel = *field == current;
        let marker = if is_sel { "❯ " } else { "  " };
        let label_style = if is_sel {
            theme.selected()
        } else if *field == Field::Create {
            theme.accent()
        } else {
            theme.dim()
        };
        let label = match field {
            Field::Name => i18n::F_NAME.to_string(),
            Field::Key => i18n::F_API_KEY.into(),
            Field::BaseUrl => i18n::F_BASE_URL.into(),
            Field::Model(slot) => i18n::fill(i18n::ROW_MODEL, &[slot.label()]),
            Field::MaxContext => format!("  {}", i18n::ROW_MAX_CONTEXT),
            Field::Effort => i18n::F_EFFORT.into(),
            Field::Isolation => i18n::F_ISOLATION.into(),
            Field::Aliases => i18n::F_ALIASES.into(),
            Field::Create => i18n::F_CREATE.into(),
        };
        let label = if *field == Field::Create {
            label
        } else {
            pad(&label, 18)
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), theme.accent()),
            Span::styled(label, label_style),
        ];
        match field {
            Field::Effort => {
                let text = match form.effort.env_value() {
                    Some(level) => level.to_string(),
                    None => match form.provider().env.get(donn_core::keys::EFFORT) {
                        // Auto = 无 intent 覆盖；展示渠道默认，避免误读成 “follow global”
                        Some(level) => format!("{level} {}", i18n::FROM_DEFAULT),
                        None => i18n::EFFORT_AUTO.to_string(),
                    },
                };
                spans.push(Span::styled(text, theme.accent()));
            }
            Field::Isolation => {
                let text = match form.isolation {
                    Isolation::Full => i18n::ISO_FULL,
                    Isolation::Shared => i18n::ISO_SHARED,
                };
                spans.push(Span::styled(text.to_string(), theme.accent()));
                if form.isolation == Isolation::Shared {
                    spans.push(Span::styled(
                        format!("  {}", i18n::ISO_SHARED_NOTE),
                        theme.warn(),
                    ));
                }
            }
            Field::Create => {}
            Field::Model(slot) => {
                let input = &form.slots[slot.index()];
                if input.is_empty() {
                    if let Some(default_model) = form.provider().models.get(*slot) {
                        spans.push(Span::styled(
                            format!("{default_model} {}", i18n::FROM_DEFAULT),
                            theme.dim(),
                        ));
                    }
                    if is_sel {
                        spans.extend(input.render_line(true, input_width).spans);
                    }
                } else {
                    spans.extend(input.render_line(is_sel, input_width).spans);
                }
            }
            Field::BaseUrl => {
                let input = &form.base_url;
                if input.is_empty() {
                    if is_sel {
                        spans.extend(input.render_line(true, input_width).spans);
                    }
                    if let Some(default_url) = &form.provider().base_url {
                        spans.push(Span::styled(
                            fit_right(
                                &format!("{} {default_url}", i18n::FROM_DEFAULT),
                                input_width,
                            ),
                            theme.dim(),
                        ));
                    }
                } else {
                    spans.extend(input.render_line(is_sel, input_width).spans);
                }
            }
            Field::Name | Field::Key | Field::Aliases | Field::MaxContext => {
                let Some(input) = form.input_of(*field) else {
                    continue;
                };
                if input.is_empty() && *field == Field::MaxContext && !is_sel {
                    spans.push(Span::styled(
                        i18n::MAX_CONTEXT_UNSET.to_string(),
                        theme.warn(),
                    ));
                } else if input.is_empty() && *field == Field::Aliases {
                    spans.push(Span::styled(
                        i18n::fill(i18n::ALIASES_DEFAULT, &[form.name.value()]),
                        theme.dim(),
                    ));
                } else {
                    spans.extend(input.render_line(is_sel, input_width).spans);
                }
            }
        }
        if let Some(error) = form.validate(core, *field) {
            spans.push(Span::styled(format!("  ✗ {error}"), theme.err()));
        }
        lines.push(Line::from(spans));
        // key 字段的辅助提示独立成行，长输入也不会遮挡
        if *field == Field::Key && is_sel {
            let mut hint = vec![
                Span::raw("  ".repeat(10)),
                Span::styled("^t", theme.accent()),
                Span::styled(format!(" {}", i18n::HINT_TOGGLE_MASK), theme.dim()),
            ];
            if let Some(url) = &form.provider().key_url {
                hint.push(Span::styled(format!("  ·  {url}"), theme.dim()));
            }
            lines.push(Line::from(hint));
        }
    }

    // 错误 / 键提示
    lines.push(Line::default());
    if let Some(error) = &form.error {
        lines.push(Line::from(Span::styled(format!(" ✗ {error}"), theme.err())));
        lines.push(Line::default());
    }

    // 底部生效预览：这次创建实际会得到什么
    lines.push(Line::from(Span::styled(i18n::PV_TITLE, theme.accent())));
    let entry = |label: &str, value: String, style: ratatui::style::Style| {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(pad(label, 10), theme.dim()),
            Span::styled(value, style),
        ])
    };
    let name = if form.name.value().trim().is_empty() {
        "<name>".to_string()
    } else {
        form.name.value().trim().to_string()
    };
    let value_width = (inner.width as usize).saturating_sub(14);
    lines.push(entry(
        i18n::PV_COMMAND,
        fit_left(
            &core
                .donn
                .bin_dir()
                .map(|dir| dir.join(&name).display().to_string())
                .unwrap_or_else(|e| format!("error: {e}")),
            value_width,
        ),
        theme.ok(),
    ));
    let config_path = match form.isolation {
        Isolation::Shared => core.donn.home().main_claude_dir(),
        Isolation::Full => core.donn.home().claude_config_dir(&name),
    };
    let mut config_line = fit_left(&config_path.display().to_string(), value_width);
    if form.isolation == Isolation::Shared {
        config_line = format!("{config_line}  {}", i18n::ISO_SHARED_TAG);
    }
    lines.push(entry(
        i18n::PV_CONFIG_DIR,
        config_line,
        if form.isolation == Isolation::Shared {
            theme.warn()
        } else {
            ratatui::style::Style::default()
        },
    ));
    lines.push(entry(
        i18n::PV_ENDPOINT,
        form.effective_base_url(),
        ratatui::style::Style::default(),
    ));
    let auth_env = match form.provider().auth_mode {
        AuthMode::ApiKey => "ANTHROPIC_API_KEY",
        AuthMode::AuthToken => "ANTHROPIC_AUTH_TOKEN",
        AuthMode::None => "-",
    };
    lines.push(entry(i18n::PV_AUTH_ENV, auth_env.to_string(), theme.dim()));
    let models: Vec<(ModelSlot, String)> = ModelSlot::ALL
        .into_iter()
        .filter_map(|slot| form.effective_model(slot).map(|model| (slot, model)))
        .collect();
    if !models.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(i18n::PV_MODELS, theme.dim()),
        ]));
        lines.extend(models.into_iter().map(|(slot, model)| {
            Line::from(vec![
                Span::raw("    "),
                Span::styled(pad(slot.label(), 10), theme.dim()),
                Span::raw("  "),
                Span::styled(model, theme.dim()),
            ])
        }));
    }
    // package 会由 render 按 sonnet 生效 id 注入：预览里给一眼
    if let Some(id) = form.effective_model(ModelSlot::Sonnet)
        && let Some(choice) = form.provider().choice_for_model(&id)
    {
        let pkg = choice.package_env();
        if let Some(max) = pkg.get(donn_core::keys::MAX_CONTEXT) {
            lines.push(entry(i18n::PV_CONTEXT, max.clone(), theme.dim()));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}
