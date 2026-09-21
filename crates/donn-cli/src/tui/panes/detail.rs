//! 右面板：选中 profile 的实时详情 + 逐行编辑入口。
//! 行列表在 load 时构建一次（不逐帧重建）；Enter 弹出对应编辑弹窗。

use donn_core::keys::ModelSlot;
use donn_core::{KeyState, ProfileView, Secret, SpecChange};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph};

use crate::tui::app::{Core, PaneId};
use crate::tui::components::list::SelectList;
use crate::tui::components::modal::{Confirm, Modal, Prompt, Select};
use crate::tui::components::{draw_list, fit_left, fit_spans, pad};
use crate::tui::i18n;
use crate::tui::theme::Theme;

/// 可交互的详情行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Key,
    BaseUrl,
    Effort,
    Isolation,
    Model(ModelSlot),
    MaxContext,
    Env(String),
    AddEnv,
    Alias(String),
    AddAlias,
}

#[derive(Default)]
pub struct DetailPane {
    pub view: Option<ProfileView>,
    pub rows: SelectList<Row>,
    /// Some = api key 行明文显示（^t 切换；换选中/重载即恢复掩码）。
    pub revealed: Option<Secret>,
    /// Some = inspect 失败原因。坏 profile 的详情面板展示错误本身
    /// （列表切换时不刷状态栏，避免淹没键提示）。
    pub error: Option<String>,
}

impl DetailPane {
    /// 加载（或清空）详情。inspect 失败记录进 `error`。
    pub fn load(&mut self, donn: &donn_core::Donn, name: Option<&str>) {
        self.revealed = None;
        self.error = None;
        self.view = None;
        if let Some(n) = name {
            match donn.inspect(n) {
                Ok(view) => self.view = Some(view),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        let rows = match &self.view {
            None => Vec::new(),
            Some(view) => {
                let mut rows = Vec::new();
                if view.preset.auth_mode.needs_key() {
                    rows.push(Row::Key);
                }
                rows.push(Row::BaseUrl);
                rows.push(Row::Effort);
                rows.push(Row::Isolation);
                for slot in ModelSlot::ALL {
                    rows.push(Row::Model(slot));
                    // 窗口是 sonnet 那个模型的属性：紧跟在它下面
                    if slot == ModelSlot::Sonnet && view.models.get(slot).is_some() {
                        rows.push(Row::MaxContext);
                    }
                }
                rows.extend(
                    view.spec
                        .intent
                        .env
                        .keys()
                        .filter(|k| k.as_str() != donn_core::keys::EFFORT)
                        .cloned()
                        .map(Row::Env),
                );
                rows.push(Row::AddEnv);
                rows.extend(view.spec.wrapper.aliases.iter().cloned().map(Row::Alias));
                rows.push(Row::AddAlias);
                rows
            }
        };
        self.rows.replace(rows);
    }
}

/// 提交单项 SpecChange 并汇报/刷新（各编辑弹窗回调共用）。
fn apply_edit(core: &mut Core, name: &str, change: SpecChange) -> Result<(), String> {
    apply_edits(core, name, [change])
}

/// 多项编辑一次 regenerate（如模型选择：槽位 + 清 package 残留键）。
fn apply_edits(
    core: &mut Core,
    name: &str,
    changes: impl IntoIterator<Item = SpecChange>,
) -> Result<(), String> {
    let report = core
        .donn
        .edit_many(name, changes)
        .map_err(|e| e.to_string())?;
    core.report_sync(&report);
    core.refresh();
    Ok(())
}

/// 编辑意图字段的通用输入弹窗：确认后把输入映射为 SpecChange 提交。
fn edit_prompt(
    title: String,
    current: String,
    hint: impl Into<String>,
    name: String,
    make: impl Fn(&str) -> SpecChange + 'static,
) -> Box<dyn Modal> {
    Box::new(
        Prompt::new(title, current, false, move |core, value| {
            apply_edit(core, &name, make(value))
        })
        .with_hint(hint),
    )
}

/// 自定义模型的窗口输入框预填值：现在主流编码模型起步就是 256K。
pub const DEFAULT_CUSTOM_WINDOW: &str = "262144";

/// 上下文窗口输入：空 = 不设，否则是正整数的 token 数。
pub fn parse_tokens(raw: &str) -> Result<Option<u64>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    // 上限 = TOML 整数上限，再大 profile.toml 存不下
    raw.parse::<u64>()
        .ok()
        .filter(|n| (1..=i64::MAX as u64).contains(n))
        .map(Some)
        .ok_or_else(|| i18n::V_MAX_CONTEXT_ERR.to_string())
}

/// 模型槽：可搜索可填入（全 provider 同一套）。
fn model_edit_modal(view: &ProfileView, slot: ModelSlot, name: &str) -> Box<dyn Modal> {
    let title = i18n::fill(
        i18n::EDIT_TITLE,
        &[&i18n::fill(i18n::ROW_MODEL, &[slot.label()]), name],
    );
    let current = view
        .spec
        .intent
        .models
        .get(slot)
        .unwrap_or_default()
        .to_string();
    let name = name.to_string();
    use crate::tui::modals::model_pick;
    let preset = model_pick::with_custom_models(&view.preset, &view.spec.intent.model_windows);
    let candidates = preset.clone();
    model_pick::open(title, &candidates, slot, &current, move |core, pick| {
        let change = model_pick::to_change(&preset, slot, pick);
        // sonnet 选了个没人知道窗口的模型：接着问它的窗口，模型和窗口一起保存；Esc = 整个不改
        if let SpecChange::Model(ModelSlot::Sonnet, Some(id)) = &change
            && model_pick::needs_window(&preset, id)
        {
            let id = id.clone();
            return Some(Box::new(
                Prompt::new(
                    i18n::fill(i18n::EDIT_TITLE, &[i18n::ROW_MAX_CONTEXT, &id]),
                    DEFAULT_CUSTOM_WINDOW.to_string(),
                    false,
                    move |core, value| {
                        let window = SpecChange::ModelWindow(id.clone(), parse_tokens(value)?);
                        apply_edits(core, &name, [change.clone(), window])
                    },
                )
                .with_hint(i18n::PROMPT_CUSTOM_WINDOW_HINT)
                .select_all(),
            ) as Box<dyn Modal>);
        }
        if let Err(e) = apply_edit(core, &name, change) {
            core.status.error(e);
        }
        None
    })
}

/// Enter：为当前行构造编辑弹窗。
pub fn activate_row(core: &mut Core) -> Option<Box<dyn Modal>> {
    let view = core.detail.view.clone()?;
    let name = view.spec.name.clone();
    let row = core.detail.rows.current()?.clone();
    match row {
        Row::Key => {
            // 提示直接给出取 key 的控制台地址
            let hint = view
                .preset
                .key_url
                .unwrap_or_else(|| i18n::PROMPT_KEY_HINT.to_string());
            Some(Box::new(
                Prompt::new(
                    i18n::fill(i18n::PROMPT_KEY_TITLE, &[&name]),
                    "",
                    true,
                    move |core, value| {
                        // trim：粘贴带尾随空格/换行的 key 直接存会认证失败
                        let Some(secret) = Secret::new(value.trim()) else {
                            return Err(i18n::KEY_EMPTY_ERR.to_string());
                        };
                        let report = core
                            .donn
                            .set_key(&name, secret)
                            .map_err(|e| e.to_string())?;
                        core.report_sync(&report);
                        core.refresh();
                        Ok(())
                    },
                )
                .with_hint(hint),
            ))
        }
        Row::BaseUrl => Some(edit_prompt(
            i18n::fill(i18n::EDIT_TITLE, &[i18n::ROW_BASE_URL, &name]),
            view.spec.intent.base_url.unwrap_or_default(),
            i18n::PROMPT_URL_HINT,
            name,
            |value| SpecChange::BaseUrl(Some(value.to_string()).filter(|v| !v.is_empty())),
        )),
        Row::Effort => {
            // Enter 打开单选弹窗（档位序列由 core 定义），确认才写盘。
            // 存量非法值原样列为第一项，用户能看见并就地切走。
            use donn_core::Effort;
            let raw = view
                .spec
                .intent
                .env
                .get(donn_core::keys::EFFORT)
                .map(String::as_str);
            let parsed = Effort::from_env(raw);
            let invalid = raw.is_some() && parsed.is_none();
            let (mut options, mut selected) = crate::tui::components::effort_options(
                i18n::EFFORT_AUTO,
                parsed.unwrap_or_default(),
            );
            if let Some(raw) = raw.filter(|_| invalid) {
                options.insert(0, i18n::fill(i18n::INVALID_VALUE, &[raw]));
                selected = 0;
            }
            Some(Box::new(Select::new(
                i18n::fill(i18n::EDIT_TITLE, &[i18n::F_EFFORT, &name]),
                options,
                selected,
                move |core, picked| {
                    if invalid && picked == 0 {
                        return;
                    }
                    let picked = picked - usize::from(invalid);
                    let change = match Effort::ALL[picked].env_value() {
                        Some(level) => {
                            SpecChange::SetEnv(donn_core::keys::EFFORT.into(), level.to_string())
                        }
                        None => SpecChange::RemoveEnv(donn_core::keys::EFFORT.into()),
                    };
                    if let Err(e) = apply_edit(core, &name, change) {
                        core.status.error(e);
                    }
                },
            )))
        }
        Row::Isolation => {
            let selected = usize::from(view.spec.isolation == donn_core::Isolation::Shared);
            Some(Box::new(Select::new(
                i18n::fill(i18n::EDIT_TITLE, &[i18n::F_ISOLATION, &name]),
                vec![i18n::ISO_FULL.to_string(), i18n::ISO_SHARED.to_string()],
                selected,
                move |core, picked| {
                    let isolation = if picked == 1 {
                        donn_core::Isolation::Shared
                    } else {
                        donn_core::Isolation::Full
                    };
                    if let Err(e) = apply_edit(core, &name, SpecChange::Isolation(isolation)) {
                        core.status.error(e);
                    }
                },
            )))
        }
        Row::Model(slot) => Some(model_edit_modal(&view, slot, &name)),
        Row::MaxContext => {
            let model = view.models.get(ModelSlot::Sonnet)?.to_string();
            let current = view.spec.intent.model_windows.get(&model);
            Some(Box::new(
                Prompt::new(
                    i18n::fill(i18n::EDIT_TITLE, &[i18n::ROW_MAX_CONTEXT, &model]),
                    current.map(u64::to_string).unwrap_or_default(),
                    false,
                    move |core, value| {
                        let window = SpecChange::ModelWindow(model.clone(), parse_tokens(value)?);
                        apply_edit(core, &name, window)
                    },
                )
                .with_hint(i18n::PROMPT_MAX_CONTEXT_HINT)
                .select_all(),
            ))
        }
        Row::Env(key) => {
            let field = i18n::fill(i18n::ROW_ENV, &[&key]);
            Some(edit_prompt(
                i18n::fill(i18n::EDIT_TITLE, &[&field, &name]),
                view.spec.intent.env.get(&key).cloned().unwrap_or_default(),
                i18n::PROMPT_ENV_HINT,
                name,
                move |value| {
                    if value.is_empty() {
                        SpecChange::RemoveEnv(key.clone())
                    } else {
                        SpecChange::SetEnv(key.clone(), value.to_string())
                    }
                },
            ))
        }
        Row::AddEnv => Some(Box::new(
            Prompt::new(
                i18n::fill(i18n::PROMPT_ENV_ADD_TITLE, &[&name]),
                "",
                false,
                move |core, value| {
                    let (key, val) = value
                        .split_once('=')
                        .ok_or_else(|| i18n::ENV_FORMAT_ERR.to_string())?;
                    let (key, val) = (key.trim(), val.trim());
                    if key.is_empty() {
                        return Err(i18n::ENV_FORMAT_ERR.into());
                    }
                    apply_edit(core, &name, SpecChange::SetEnv(key.into(), val.into()))
                },
            )
            .with_hint(i18n::ENV_FORMAT_ERR),
        )),
        Row::Alias(alias) => Some(Box::new(Confirm::new(
            i18n::REMOVE_ALIAS_TITLE,
            vec![i18n::fill(i18n::REMOVE_ALIAS_Q, &[&alias])],
            move |core| match core.donn.remove_alias(&name, &alias) {
                Ok(()) => {
                    let msg = i18n::fill(i18n::ST_ALIAS_REMOVED, &[&alias]);
                    core.status.ok(msg);
                    core.refresh();
                }
                Err(e) => core.status.error(e.to_string()),
            },
        ))),
        Row::AddAlias => Some(Box::new(
            Prompt::new(
                i18n::fill(i18n::PROMPT_ALIAS_ADD_TITLE, &[&name]),
                "",
                false,
                move |core, value| {
                    let path = core
                        .donn
                        .add_alias(&name, value)
                        .map_err(|e| e.to_string())?;
                    let msg = i18n::fill(i18n::ST_ALIAS_READY, &[&path.display().to_string()]);
                    core.status.ok(msg);
                    core.refresh();
                    Ok(())
                },
            )
            .with_hint(i18n::PROMPT_ALIAS_HINT),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyItem {
    label: String,
    display: String,
    value: String,
}

/// 复制菜单条目：启动命令、四个路径与 profile 显式 env 值；API key
/// 不在 spec env 中，故永远不会出现在此菜单。
fn copy_items(core: &Core) -> Vec<CopyItem> {
    let Some(name) = core.selected_profile() else {
        return Vec::new();
    };
    let home = core.donn.home();
    let absolute = |path: std::path::PathBuf| path.display().to_string();
    let display = |value: &str| home.tilde(std::path::Path::new(value));

    let Some(view) = core.detail.view.as_ref() else {
        let value = absolute(home.spec_file(name));
        return vec![CopyItem {
            label: i18n::SB_SPEC.to_string(),
            display: display(&value),
            value,
        }];
    };

    let command = view
        .spec
        .wrapper
        .aliases
        .first()
        .cloned()
        .unwrap_or_else(|| format!("donn run {name}"));
    let config_dir = if view.spec.isolation == donn_core::Isolation::Shared {
        home.main_claude_dir()
    } else {
        home.claude_config_dir(name)
    };
    let mut items = vec![CopyItem {
        label: i18n::COPY_COMMAND.to_string(),
        display: command.clone(),
        value: command,
    }];
    for (label, path) in [
        (i18n::SB_CONFIG, config_dir),
        (i18n::SB_SETTINGS, home.settings_file(name)),
        (i18n::SB_SPEC, home.spec_file(name)),
    ] {
        let value = absolute(path);
        items.push(CopyItem {
            label: label.to_string(),
            display: display(&value),
            value,
        });
    }
    if let Some(alias) = view.spec.wrapper.aliases.first()
        && let Ok(bin_dir) = core.donn.bin_dir()
    {
        let value = absolute(donn_core::wrapper::wrapper_path(&bin_dir, alias));
        items.push(CopyItem {
            label: i18n::SB_WRAPPER.to_string(),
            display: display(&value),
            value,
        });
    }
    items.extend(
        view.spec
            .intent
            .env
            .iter()
            .filter(|(key, _)| key.as_str() != donn_core::keys::EFFORT)
            .map(|(key, value)| CopyItem {
                label: i18n::fill(i18n::ROW_ENV, &[key]),
                display: value.clone(),
                value: value.clone(),
            }),
    );
    items
}

pub fn copy_modal(core: &Core) -> Option<Box<dyn Modal>> {
    let items = copy_items(core);
    if items.is_empty() {
        return None;
    }
    let options = items
        .iter()
        .map(|item| format!("{}  {}", pad(&item.label, 16), item.display))
        .collect();
    Some(Box::new(Select::new(
        i18n::COPY_TITLE,
        options,
        0,
        move |core, picked| {
            if let Some(item) = items.get(picked) {
                core.copy_text(&item.label, item.value.clone());
            }
        },
    )))
}

pub fn render(core: &mut Core, f: &mut Frame, area: Rect) {
    let focused = core.focus == PaneId::Detail;
    let theme = core.theme;
    let title = match core.detail.view.as_ref() {
        Some(view) => format!(" {} ", view.spec.name),
        None => i18n::DETAIL_EMPTY_TITLE.to_string(),
    };
    let block = theme.pane_block(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(view) = &core.detail.view else {
        // 坏 profile：错误原文上屏（含修复线索），而不是无解释的空白
        let paragraph = match &core.detail.error {
            Some(err) => Paragraph::new(vec![
                Line::from(Span::styled(i18n::DETAIL_BROKEN.to_string(), theme.err())),
                Line::default(),
                Line::from(Span::raw(err.clone())),
            ])
            .wrap(ratatui::widgets::Wrap { trim: false }),
            None => Paragraph::new(Line::from(Span::styled(
                i18n::SELECT_A_PROFILE,
                theme.dim(),
            ))),
        };
        f.render_widget(paragraph, inner);
        return;
    };

    // 头部：preset、时间戳、漂移/过期徽标
    let mut header = vec![Line::from(vec![
        Span::styled(pad(i18n::F_PROVIDER, 10), theme.dim()),
        Span::styled(view.preset.key.clone(), theme.accent()),
        Span::styled(format!("  ({})", view.preset.label), theme.dim()),
    ])];
    let mut badges: Vec<Span> = Vec::new();
    if !view.drift.is_empty() {
        let banner = i18n::fill(i18n::DRIFT_BANNER, &[&view.drift.len().to_string()]);
        badges.push(Span::styled(format!("{banner}  "), theme.warn()));
    }
    if !badges.is_empty() {
        header.push(Line::from(badges));
    }
    header.push(Line::default());

    // 信息区整宽贴底：侧栏会挤压行区并截断路径/URL，纵向平铺则两者都拿整行宽度
    let info = info_lines(core, view, &theme, inner.width);
    let rows_needed = core.detail.rows.len() as u16 + header.len() as u16;
    let info_height = if inner.height >= rows_needed + info.len() as u16 {
        info.len() as u16
    } else {
        0 // 高度不够时行区优先，信息区整体让位
    };

    // 行区：header 段落 + ratatui List（滚动/高亮原生处理）
    let header_height = header.len() as u16;
    let [header_area, list_area, info_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(header_height),
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(info_height),
    ])
    .areas(inner);
    f.render_widget(Paragraph::new(header), header_area);

    // `❯ ` 标记 2 列 + 标签列之外才是值的可用宽度
    let value_width = (list_area.width as usize).saturating_sub(2 + LABEL_WIDTH);
    let items: Vec<ListItem> = core
        .detail
        .rows
        .items
        .iter()
        .map(|row| {
            ListItem::from(render_row(
                &theme,
                view,
                row,
                core.detail.revealed.as_ref(),
                value_width,
            ))
        })
        .collect();
    core.hit.detail_list = list_area;
    let state = &mut core.detail.rows.state;
    draw_list(f, list_area, items, theme.highlight(focused), state, None);

    if info_height > 0 {
        f.render_widget(Paragraph::new(info), info_area);
    }
}

/// 只读信息区：整宽贴在行区底部。
/// 路径/URL 独占整行（左省略保尾部）；零散元信息合并为一条 dim 行。
fn info_lines(core: &Core, view: &ProfileView, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let name = &view.spec.name;
    let label_w = 10usize;
    let value_w = (width as usize).saturating_sub(label_w);
    let tilde = |p: std::path::PathBuf| fit_left(&core.donn.home().tilde(&p), value_w);
    let entry = |label: &str, spans: Vec<Span<'static>>| {
        let mut line = vec![Span::styled(pad(label, label_w), theme.dim())];
        line.extend(spans);
        Line::from(line)
    };
    let shared = view.spec.isolation == donn_core::Isolation::Shared;

    let mut lines = vec![Line::from(Span::styled(
        "─".repeat(width as usize),
        theme.dim(),
    ))];

    // 配置目录 / spec / 取 key：各占整行
    let mut config = vec![Span::raw(if shared {
        tilde(core.donn.home().main_claude_dir())
    } else {
        tilde(core.donn.home().claude_config_dir(name))
    })];
    if shared {
        config.push(Span::styled(
            format!("  {}", i18n::ISO_SHARED_TAG),
            theme.warn(),
        ));
    }
    lines.push(entry(i18n::SB_CONFIG, config));
    lines.push(entry(
        i18n::SB_SETTINGS,
        vec![Span::styled(
            tilde(core.donn.home().settings_file(name)),
            theme.dim(),
        )],
    ));
    lines.push(entry(
        i18n::SB_SPEC,
        vec![Span::styled(
            tilde(core.donn.home().spec_file(name)),
            theme.dim(),
        )],
    ));
    let wrapper_path = view
        .spec
        .wrapper
        .aliases
        .first()
        .and_then(|alias| {
            core.donn
                .bin_dir()
                .ok()
                .map(|bin| donn_core::wrapper::wrapper_path(&bin, alias))
        })
        .map(tilde)
        .unwrap_or_else(|| "-".into());
    lines.push(entry(
        i18n::SB_WRAPPER,
        vec![Span::styled(wrapper_path, theme.dim())],
    ));
    if let Some(url) = &view.preset.key_url {
        lines.push(entry(
            i18n::SB_KEYS_AT,
            vec![Span::styled(fit_left(url, value_w), theme.dim())],
        ));
    }

    // 元信息一行：创建 · 更新 · 认证 · 托管键数 · 启动命令
    let date = |ts: &str| ts.split('T').next().unwrap_or(ts).to_string();
    let auth_short = match view.spec.auth.mode {
        donn_core::AuthMode::ApiKey => "api_key",
        donn_core::AuthMode::AuthToken => "auth_token",
        donn_core::AuthMode::None => "oauth login",
    };
    let command = view
        .spec
        .wrapper
        .aliases
        .first()
        .cloned()
        .unwrap_or_else(|| format!("donn run {name}"));
    let mut meta = vec![
        format!("{} {}", i18n::SB_CREATED, date(&view.spec.created_at)),
        format!("{} {}", i18n::SB_UPDATED, date(&view.spec.updated_at)),
    ];
    meta.push(auth_short.to_string());
    meta.push(format!(
        "{} · {} settings · {} claude · {} permissions",
        i18n::fill(
            i18n::SB_OWNS,
            &[&view.spec.footprint.settings_env.len().to_string()],
        ),
        view.spec.footprint.settings_top.len(),
        view.spec.footprint.claude_json.len(),
        view.spec.footprint.permissions_top.len(),
    ));
    meta.push(format!("{} {}", i18n::SB_COMMAND, command));
    lines.push(Line::from(Span::styled(meta.join("  ·  "), theme.dim())));
    lines
}

/// 详情行标签列宽。
const LABEL_WIDTH: usize = 18;

fn render_row(
    theme: &Theme,
    view: &ProfileView,
    row: &Row,
    revealed: Option<&Secret>,
    value_width: usize,
) -> Line<'static> {
    let label_style = theme.dim();
    let (label, value): (String, Vec<Span>) = match row {
        Row::Key => {
            let value = match (&view.key, revealed) {
                // ^t 明文查看（reveal 是显式调用，值只进屏幕不进日志）
                (KeyState::Present { .. }, Some(secret)) => {
                    Span::styled(secret.reveal().to_string(), theme.warn())
                }
                (KeyState::Present { tail4 }, None) => {
                    Span::styled(format!("sk-***{tail4}"), theme.ok())
                }
                (KeyState::Absent, _) => Span::styled(i18n::KEY_NOT_SET.to_string(), theme.err()),
                (KeyState::NotNeeded, _) => {
                    Span::styled(i18n::KEY_NOT_NEEDED.to_string(), theme.dim())
                }
            };
            (i18n::ROW_API_KEY.into(), vec![value])
        }
        Row::Effort => {
            // 生效：intent 覆盖 > preset.env > 无（显示 auto）
            let value = match view.spec.intent.env.get(donn_core::keys::EFFORT) {
                Some(level) => match donn_core::Effort::from_env(Some(level)) {
                    Some(_) => vec![Span::styled(level.clone(), theme.accent())],
                    None => {
                        vec![Span::styled(
                            i18n::fill(i18n::INVALID_VALUE, &[level]),
                            theme.err(),
                        )]
                    }
                },
                None => match view.preset.env.get(donn_core::keys::EFFORT) {
                    Some(level) => vec![
                        Span::styled(level.clone(), theme.dim()),
                        Span::styled(format!("  {}", i18n::FROM_DEFAULT), theme.dim()),
                    ],
                    None => {
                        vec![Span::styled(
                            i18n::EFFORT_AUTO_SHORT.to_string(),
                            theme.dim(),
                        )]
                    }
                },
            };
            (i18n::F_EFFORT.into(), value)
        }
        Row::Isolation => {
            let value = match view.spec.isolation {
                donn_core::Isolation::Full => Span::raw(i18n::ISO_FULL_TAG.to_string()),
                donn_core::Isolation::Shared => {
                    Span::styled(i18n::ISO_SHARED_TAG.to_string(), theme.warn())
                }
            };
            (i18n::F_ISOLATION.into(), vec![value])
        }
        Row::BaseUrl => {
            let overridden = view.spec.intent.base_url.is_some();
            let mut spans = vec![Span::raw(
                view.base_url.clone().unwrap_or_else(|| "-".into()),
            )];
            if !overridden {
                spans.push(Span::styled(
                    format!("  {}", i18n::FROM_DEFAULT),
                    theme.dim(),
                ));
            }
            (i18n::ROW_BASE_URL.into(), spans)
        }
        Row::Model(slot) => {
            let mut spans = vec![Span::raw(view.models.get(*slot).unwrap_or("-").to_string())];
            // 来源标记：profile 覆盖不标；[defaults.env] 标全局；否则是 preset 默认
            if view.spec.intent.models.get(*slot).is_none() && view.models.get(*slot).is_some() {
                let tag = if view.global_models.get(*slot).is_some() {
                    i18n::FROM_GLOBAL
                } else {
                    i18n::FROM_DEFAULT
                };
                spans.push(Span::styled(format!("  {tag}"), theme.dim()));
            }
            (i18n::fill(i18n::ROW_MODEL, &[slot.label()]), spans)
        }
        Row::MaxContext => {
            // 当前 sonnet 这个模型的窗口：用户定义的 > preset 候选自带的 > 交给 Claude Code
            let sonnet = view.models.get(ModelSlot::Sonnet).unwrap_or_default();
            let preset_window = view
                .preset
                .choice_for_model(sonnet)
                .and_then(|choice| choice.max_context);
            let spans = match (view.spec.intent.model_windows.get(sonnet), preset_window) {
                (Some(tokens), _) => vec![Span::raw(tokens.to_string())],
                (None, Some(tokens)) => vec![
                    Span::raw(tokens.to_string()),
                    Span::styled(format!("  {}", i18n::FROM_DEFAULT), theme.dim()),
                ],
                (None, None) => vec![Span::styled(
                    i18n::MAX_CONTEXT_UNSET.to_string(),
                    theme.warn(),
                )],
            };
            (format!("  {}", i18n::ROW_MAX_CONTEXT), spans)
        }
        Row::Env(key) => {
            let value = view.spec.intent.env.get(key).cloned().unwrap_or_default();
            (i18n::fill(i18n::ROW_ENV, &[key]), vec![Span::raw(value)])
        }
        Row::AddEnv => (
            i18n::ROW_ENV_ADD.into(),
            vec![Span::styled(i18n::ADD_ELLIPSIS.to_string(), theme.dim())],
        ),
        Row::Alias(alias) => (i18n::ROW_ALIAS.into(), vec![Span::raw(alias.clone())]),
        Row::AddAlias => (
            i18n::ROW_ALIAS_ADD.into(),
            vec![Span::styled(i18n::ADD_ELLIPSIS.to_string(), theme.dim())],
        ),
    };
    let mut spans = vec![Span::styled(pad(&label, LABEL_WIDTH), label_style)];
    spans.extend(fit_spans(value, value_width));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_input_is_a_positive_integer_or_empty() {
        assert_eq!(parse_tokens(""), Ok(None));
        assert_eq!(parse_tokens(" 262144 "), Ok(Some(262_144)));
        assert_eq!(
            parse_tokens("9223372036854775807"),
            Ok(Some(i64::MAX as u64))
        );
        for bad in ["0", "abc", "-5", "256k", "1.5", "9223372036854775808"] {
            assert!(parse_tokens(bad).is_err(), "{bad}");
        }
    }
    use crossterm::event::KeyCode;
    use donn_core::{Donn, DonnHome, Isolation, ProfileDraft};

    #[test]
    fn isolation_row_opens_a_picker_and_changes_only_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let donn = Donn::with_home(DonnHome::for_test(dir.path())).unwrap();
        donn.create(&ProfileDraft {
            name: "official".into(),
            preset: "official".into(),
            ..Default::default()
        })
        .unwrap();
        let mut core = Core::new(donn);
        core.detail.rows.select_where(|row| *row == Row::Isolation);
        assert_eq!(core.detail.rows.current(), Some(&Row::Isolation));

        let mut modal = activate_row(&mut core).expect("isolation must open a select modal");
        assert_eq!(
            core.donn.spec("official").unwrap().isolation,
            Isolation::Full,
            "opening the picker must not toggle immediately"
        );
        let _ = modal.handle(KeyCode::Down.into(), &mut core);
        let _ = modal.handle(KeyCode::Enter.into(), &mut core);
        assert_eq!(
            core.donn.spec("official").unwrap().isolation,
            Isolation::Shared
        );
    }

    #[test]
    fn copy_menu_contains_launch_paths_and_explicit_env_without_effort() {
        let dir = tempfile::tempdir().unwrap();
        let home = DonnHome::for_test(dir.path());
        let donn = Donn::with_home(home.clone()).unwrap();
        donn.create(&ProfileDraft {
            name: "copy-profile".into(),
            preset: "official".into(),
            env: vec![
                ("CUSTOM_VALUE".into(), "copy-me".into()),
                (donn_core::keys::EFFORT.into(), "high".into()),
            ],
            ..Default::default()
        })
        .unwrap();
        let core = Core::new(donn);

        let items = copy_items(&core);
        assert_eq!(items[0].value, "copy-profile");
        assert!(items.iter().any(|item| item.value == "copy-me"));
        assert!(
            items
                .iter()
                .any(|item| item.value
                    == home.claude_config_dir("copy-profile").display().to_string())
        );
        assert!(
            items
                .iter()
                .any(|item| item.value == home.settings_file("copy-profile").display().to_string())
        );
        assert!(
            items
                .iter()
                .any(|item| item.value == home.spec_file("copy-profile").display().to_string())
        );
        assert!(items.iter().any(|item| {
            item.value
                == donn_core::wrapper::wrapper_path(&home.default_bin_dir(), "copy-profile")
                    .display()
                    .to_string()
        }));
        assert!(
            items
                .iter()
                .all(|item| item.label != i18n::ROW_ENV.replace("{}", donn_core::keys::EFFORT))
        );
    }
}
