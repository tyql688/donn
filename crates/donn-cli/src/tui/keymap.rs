//! Keymap：(焦点上下文, 按键) 到 Action 的唯一映射处；状态栏提示和帮助页都从这里取。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::i18n;

/// keymap 解析出的语义动作；dispatch 在 app.rs。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Esc：逐级返回（详情回左栏、doctor 收起），并清状态栏。
    Back,
    /// Tab：焦点在可见面板间轮转。
    FocusNext,
    FocusLeft,
    FocusRight,
    /// 直达详情（profiles 列表按 `e`）。
    FocusDetail,
    MoveUp,
    MoveDown,
    JumpTop,
    JumpBottom,
    ToggleDoctor,
    /// Enter：按焦点面板语义（启动 profile / 编辑详情行）。
    Activate,
    OpenAdd,
    RemoveProfile,
    SyncProfile,
    OpenEditor,
    RerunDoctor,
    /// ^t：详情页 api key 明文/掩码切换。
    ToggleKeyReveal,
    /// o：浏览器打开当前 provider 的取 key 控制台地址。
    OpenKeyUrl,
    /// y：打开复制选择器（启动命令、路径、profile env）。
    OpenCopy,
    /// S：全局设置面板（右栏按行编辑 config.toml，改动立即 sync 所有 profile）。
    OpenSettings,
    Help,
}

/// 焦点上下文。弹窗内部按键由弹窗自行处理，不走 keymap。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Profiles,
    Detail,
    Doctor,
    /// Add 阶段 1：选 provider（按键由表单直接处理，keymap 只提供状态栏提示）。
    AddPick,
    /// Add 阶段 2：填表。
    AddForm,
    /// 全局设置面板。
    Settings,
}

/// 解析按键。
pub fn lookup(context: Context, key: &KeyEvent) -> Option<Action> {
    // ^t key 明文切换需要 CONTROL，先于修饰键守卫处理
    if context == Context::Detail
        && key.code == KeyCode::Char('t')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Some(Action::ToggleKeyReveal);
    }
    // Ctrl/Alt 组合不映射到面板动作（Shift 只是大小写）：避免 Ctrl+S 之类误触 sync
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    // 全局键
    match key.code {
        KeyCode::Char('q') => return Some(Action::Quit),
        KeyCode::Esc => return Some(Action::Back),
        KeyCode::Tab => return Some(Action::FocusNext),
        KeyCode::Char('h') | KeyCode::Left => return Some(Action::FocusLeft),
        KeyCode::Char('l') | KeyCode::Right => return Some(Action::FocusRight),
        KeyCode::Char('k') | KeyCode::Up => return Some(Action::MoveUp),
        KeyCode::Char('j') | KeyCode::Down => return Some(Action::MoveDown),
        KeyCode::Char('g') | KeyCode::Home => return Some(Action::JumpTop),
        KeyCode::Char('G') | KeyCode::End => return Some(Action::JumpBottom),
        KeyCode::Char('D') => return Some(Action::ToggleDoctor),
        KeyCode::Char('S') => return Some(Action::OpenSettings),
        KeyCode::Char('a') => return Some(Action::OpenAdd),
        KeyCode::Char('?') => return Some(Action::Help),
        KeyCode::Enter => return Some(Action::Activate),
        _ => {}
    }
    // 面板键
    match (context, key.code) {
        (Context::Profiles | Context::Detail, KeyCode::Char('d')) => Some(Action::RemoveProfile),
        (Context::Profiles | Context::Detail, KeyCode::Char('s')) => Some(Action::SyncProfile),
        (Context::Profiles, KeyCode::Char('e')) => Some(Action::FocusDetail),
        (Context::Detail, KeyCode::Char('E')) => Some(Action::OpenEditor),
        (Context::Profiles | Context::Detail, KeyCode::Char('y')) => Some(Action::OpenCopy),
        (Context::Profiles | Context::Detail, KeyCode::Char('o')) => Some(Action::OpenKeyUrl),
        (Context::Doctor, KeyCode::Char('r')) => Some(Action::RerunDoctor),
        _ => None,
    }
}

/// 状态栏按键提示（空闲时显示；按 `?` 查看全部）。
pub fn hints(context: Context) -> Vec<(&'static str, &'static str)> {
    match context {
        Context::Profiles => vec![
            ("enter", i18n::HINT_LAUNCH),
            ("e", i18n::HINT_EDIT),
            ("a", i18n::HINT_ADD),
            ("?", i18n::HINT_HELP),
            ("q", i18n::HINT_QUIT),
        ],
        Context::Detail | Context::Settings => vec![
            ("↑↓", i18n::KS_MOVE),
            ("enter", i18n::HINT_EDIT_ROW),
            ("esc", i18n::HINT_BACK),
            ("?", i18n::HINT_HELP),
        ],
        Context::Doctor => vec![
            ("r", i18n::HINT_RERUN),
            ("esc", i18n::HINT_CLOSE),
            ("?", i18n::HINT_HELP),
        ],
        Context::AddPick => vec![
            ("↑↓", i18n::HINT_PICK_PROVIDER),
            ("enter", i18n::HINT_CONFIRM_PROVIDER),
            ("tab", i18n::HINT_FILTER),
            ("esc", i18n::ADD_FOOTER_CANCEL),
            ("?", i18n::HINT_HELP),
        ],
        Context::AddForm => vec![
            ("↑↓", i18n::ADD_FOOTER_FIELD),
            ("enter", i18n::ADD_FOOTER_NEXT),
            ("^s", i18n::ADD_FOOTER_CREATE),
            ("esc", i18n::ADD_FOOTER_BACK),
            ("?", i18n::HINT_HELP),
        ],
    }
}

/// 帮助页速查表（`?`）。
pub fn cheatsheet() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            i18n::SEC_NAVIGATE,
            vec![
                ("j/k ↑/↓", i18n::KS_MOVE),
                ("g/G", i18n::KS_TOP_BOTTOM),
                ("h/l ←/→", i18n::KS_FOCUS_LR),
                ("tab", i18n::KS_CYCLE_PANES),
                ("esc", i18n::KS_BACK_CLOSE),
            ],
        ),
        (
            i18n::SEC_PROFILES,
            vec![
                ("enter", i18n::KS_LAUNCH),
                ("e", i18n::KS_EDIT_SELECTED),
                ("a", i18n::KS_ADD),
                ("d", i18n::KS_REMOVE),
            ],
        ),
        (
            i18n::SEC_DETAIL,
            vec![
                ("enter", i18n::KS_EDIT_ROW),
                ("^t", i18n::HINT_TOGGLE_MASK),
                ("s", i18n::KS_SYNC),
                ("E", i18n::KS_OPEN_EDITOR),
            ],
        ),
        (
            i18n::SEC_OTHER,
            vec![
                ("o", i18n::HINT_KEY_URL),
                ("y", i18n::KS_COPY),
                ("click", i18n::KS_LINK),
                ("D", i18n::KS_TOGGLE_DOCTOR),
                ("r", i18n::KS_RERUN_DOCTOR),
                ("S", i18n::HINT_SETTINGS),
                ("q", i18n::KS_QUIT),
            ],
        ),
    ]
}
