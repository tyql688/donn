//! App：dashboard 单一状态 + Action 分发。
//! Core = 数据与面板状态；App = Core + modal 栈（分离以便弹窗回调可变借用 Core）。

use donn_core::{Donn, ProfileCard, SyncReport};

use crate::tui::action::Action;
use crate::tui::components::list::{Nav, SelectList};
use crate::tui::components::modal::{Confirm, Modal, Select};
use crate::tui::components::status_bar::Status;
use crate::tui::i18n;
use crate::tui::keymap::Context;
use crate::tui::panes::add_form::{AddForm, Stage};
use crate::tui::panes::detail::DetailPane;
use crate::tui::panes::doctor::DoctorPane;
use crate::tui::panes::settings::SettingsPane;
use crate::tui::theme::Theme;
use ratatui::layout::{Position, Rect};

/// 焦点面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneId {
    Left,
    Detail,
    Doctor,
}

/// 事件循环需要感知的跨界动作。
pub enum LoopCmd {
    Quit,
    Launch(String),
    EditSettings(String),
}

/// 渲染期记录的列表命中区域（鼠标点击/滚轮路由用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct HitAreas {
    pub left_list: Rect,
    pub detail_list: Rect,
    pub doctor_list: Rect,
}

pub struct Core {
    pub donn: Donn,
    pub theme: Theme,
    clipboard: crate::tui::clipboard::Clipboard,
    pub status: Status,
    pub focus: PaneId,
    pub profiles: SelectList<ProfileCard>,
    pub detail: DetailPane,
    /// Some = doctor 抽屉展开。
    pub doctor: Option<DoctorPane>,
    /// Some = Add 面板模式：左栏渠道列表联动，右栏表单。
    pub add: Option<AddForm>,
    /// Some = 全局设置面板（右栏行编辑 config.toml）。
    pub settings: Option<SettingsPane>,
    pub hit: HitAreas,
    /// 上一帧屏幕上的可点链接（见 [`crate::tui::links`]）。
    pub links: Vec<crate::tui::links::Link>,
}

pub struct App {
    pub core: Core,
    pub modals: Vec<Box<dyn Modal>>,
}

impl Core {
    pub fn new(donn: Donn) -> Self {
        let mut core = Self {
            donn,
            theme: Theme::default(),
            clipboard: crate::tui::clipboard::Clipboard::default(),
            status: Status::default(),
            focus: PaneId::Left,
            profiles: SelectList::default(),
            detail: DetailPane::default(),
            doctor: None,
            add: None,
            settings: None,
            hit: HitAreas::default(),
            links: Vec::new(),
        };
        core.refresh();
        core
    }

    /// 重载 profile 列表与详情（任何写操作后调用）。
    pub fn refresh(&mut self) {
        match self.donn.cards() {
            Ok(cards) => self.profiles.replace(cards),
            Err(e) => self.status.error(e.to_string()),
        }
        self.reload_detail();
    }

    /// 详情跟随左面板当前选中的 profile。
    /// 加载失败只写详情面板（`detail.error`），不刷状态栏——切换列表时反复弹错会淹没键提示。
    pub fn reload_detail(&mut self) {
        let name = self.profiles.current().map(|c| c.name.clone());
        self.detail.load(&self.donn, name.as_deref());
    }

    pub fn selected_profile(&self) -> Option<&str> {
        self.profiles.current().map(|c| c.name.as_str())
    }

    /// Detail 焦点是否合法：右栏正在展示 profile 详情。
    pub fn detail_reachable(&self) -> bool {
        self.settings.is_none() && self.add.is_none()
    }

    /// 关闭全局设置面板（若开着），回到左栏。
    pub fn close_settings(&mut self) {
        if self.settings.take().is_some() {
            self.focus = PaneId::Left;
        }
    }

    /// 焦点上下文（keymap / 状态栏提示用）。
    pub fn context(&self) -> Context {
        if let Some(form) = &self.add {
            return match form.stage() {
                Stage::PickProvider => Context::AddPick,
                Stage::EditForm => Context::AddForm,
            };
        }
        if self.settings.is_some() {
            return Context::Settings;
        }
        match self.focus {
            PaneId::Left => Context::Profiles,
            PaneId::Detail => Context::Detail,
            PaneId::Doctor => Context::Doctor,
        }
    }

    /// 进入 Add：左栏渠道列表 + 右栏表单（仅在 add 流程内）。
    pub fn enter_add(&mut self, preset_key: Option<String>) {
        self.settings = None;
        self.doctor = None; // 与 settings 一样：避免抽屉抢焦点/鼠标
        self.add = Some(AddForm::new(self, preset_key));
        self.focus = PaneId::Left;
    }

    /// 退出 Add，回到 profile 列表。
    pub fn exit_add(&mut self) {
        self.add = None;
        self.focus = PaneId::Left;
        self.reload_detail();
    }

    /// 焦点面板的列表导航（键盘与滚轮共用）。
    /// 设置面板独占右栏时，导航只作用于设置行，doctor 焦点不参与。
    pub fn nav_focused(&mut self, nav: Nav) {
        if self.settings.is_some() {
            if let Some(pane) = &mut self.settings {
                pane.nav(nav);
            }
            return;
        }
        match self.focus {
            PaneId::Left => {
                self.profiles.nav(nav);
                self.reload_detail();
            }
            PaneId::Detail => self.detail.rows.nav(nav),
            PaneId::Doctor => {
                if let Some(doctor) = &mut self.doctor {
                    doctor.checks.nav(nav);
                }
            }
        }
    }

    /// SyncReport → 状态栏文案。
    pub fn report_sync(&mut self, report: &SyncReport) {
        if !report.overwritten.is_empty() {
            let msg = i18n::fill(i18n::ST_OVERWROTE, &[&report.overwritten.join(", ")]);
            self.status.warn(msg);
        } else {
            self.status.ok(i18n::ST_SAVED);
        }
    }

    /// 焦点和各列表的选中位置：判断一次点击有没有「选中了什么」。
    fn selection(&self) -> (PaneId, [Option<usize>; 5]) {
        (
            self.focus,
            [
                Some(self.profiles.selected()),
                Some(self.detail.rows.selected()),
                self.settings.as_ref().map(|pane| pane.rows.selected()),
                self.doctor.as_ref().map(|pane| pane.checks.selected()),
                self.add.as_ref().map(|form| form.providers().selected()),
            ],
        )
    }

    /// 用系统默认应用打开 URL。
    pub fn open_url(&mut self, url: &str) {
        match open::that(url) {
            Ok(()) => self.status.ok(i18n::fill(i18n::ST_OPENED_URL, &[url])),
            Err(e) => self.status.error(e.to_string()),
        }
    }

    pub fn copy_text(&mut self, label: &str, text: String) {
        match self.clipboard.set_text(text) {
            Ok(()) => {
                let message = i18n::fill(i18n::ST_COPIED, &[label]);
                self.status.ok(message);
            }
            Err(error) => self.status.error(error),
        }
    }
}

impl App {
    pub fn new(donn: Donn) -> Self {
        Self {
            core: Core::new(donn),
            modals: Vec::new(),
        }
    }

    /// 分发面板层动作。返回需要事件循环处理的跨界命令。
    pub fn dispatch(&mut self, action: Action) -> Option<LoopCmd> {
        // 全局设置：独占右栏。只响应导航/激活/帮助/退出与 S·Esc 关闭；
        // 避免 Focus* / doctor / 删除 等与设置面板叠在一起抢焦点。
        if self.core.settings.is_some() {
            return self.dispatch_settings(action);
        }

        let core = &mut self.core;
        match action {
            Action::Quit => return Some(LoopCmd::Quit),
            // Esc：逐级返回。详情→左栏；doctor 抽屉关闭。
            Action::Back => {
                core.status.clear();
                match core.focus {
                    PaneId::Detail => core.focus = PaneId::Left,
                    PaneId::Doctor => {
                        core.doctor = None;
                        core.focus = PaneId::Left;
                    }
                    // 抽屉开着时 Esc 直接收起（不必先 Tab 过去）
                    PaneId::Left => core.doctor = None,
                }
            }
            Action::FocusNext => {
                core.focus = match core.focus {
                    PaneId::Left if core.detail_reachable() => PaneId::Detail,
                    PaneId::Left if core.doctor.is_some() => PaneId::Doctor,
                    PaneId::Left => PaneId::Left,
                    PaneId::Detail if core.doctor.is_some() => PaneId::Doctor,
                    PaneId::Detail => PaneId::Left,
                    PaneId::Doctor => PaneId::Left,
                };
            }
            Action::FocusLeft => core.focus = PaneId::Left,
            Action::FocusRight => {
                if core.focus == PaneId::Left && core.detail_reachable() {
                    core.focus = PaneId::Detail;
                }
            }
            Action::FocusDetail => {
                if core.detail_reachable() {
                    core.focus = PaneId::Detail;
                }
            }
            Action::MoveUp => core.nav_focused(Nav::By(-1)),
            Action::MoveDown => core.nav_focused(Nav::By(1)),
            Action::JumpTop => core.nav_focused(Nav::Top),
            Action::JumpBottom => core.nav_focused(Nav::Bottom),
            Action::Help => {
                self.modals
                    .push(Box::new(crate::tui::modals::help::HelpModal));
            }
            Action::ToggleDoctor => {
                if core.doctor.take().is_some() {
                    if core.focus == PaneId::Doctor {
                        core.focus = PaneId::Left;
                    }
                } else {
                    core.doctor = Some(DoctorPane::run(&core.donn));
                    core.focus = PaneId::Doctor;
                }
            }
            Action::RerunDoctor => {
                if core.doctor.is_some() {
                    core.doctor = Some(DoctorPane::run(&core.donn));
                }
            }
            Action::OpenSettings => {
                // Add form owns the right pane; don't stack settings under it
                if core.add.is_some() {
                    return None;
                }
                // 独占右栏：收起 doctor
                core.doctor = None;
                match SettingsPane::new(&core.donn) {
                    Ok(pane) => {
                        core.settings = Some(pane);
                        core.focus = PaneId::Detail;
                    }
                    Err(e) => core.status.error(e),
                }
                core.status.clear();
            }
            Action::OpenAdd => {
                core.enter_add(None);
            }
            Action::Activate => return self.activate(),
            Action::RemoveProfile => {
                if let Some(card) = core.profiles.current() {
                    let name = card.name.clone();
                    match core.donn.spec(&name) {
                        Ok(spec) => {
                            // 运行中会话：确认框确认了也删不掉，提前拦截并提示
                            match core.donn.live_check(&name) {
                                donn_core::LiveCheck::Running => {
                                    let msg = i18n::fill(i18n::ST_PROFILE_IN_USE, &[&name]);
                                    core.status.error(msg);
                                }
                                live => {
                                    // full 模式用 Select（无 body）：探测失败写状态栏；
                                    // shared 模式 Confirm 的 body 会附上同一提示
                                    if live == donn_core::LiveCheck::Unavailable
                                        && spec.isolation == donn_core::Isolation::Full
                                    {
                                        core.status.warn(i18n::REMOVE_LIVE_UNKNOWN);
                                    }
                                    self.modals.push(confirm_remove(name, spec.isolation, live));
                                }
                            }
                        }
                        Err(_) => {
                            // 坏 spec 也必须能从 UI 删除。隔离模式未知时提供“保留会话 / 全删”
                            // 两个明确选项；core 的 keep 路径按 full 保守处理。
                            match core.donn.live_check(&name) {
                                donn_core::LiveCheck::Running => {
                                    let msg = i18n::fill(i18n::ST_PROFILE_IN_USE, &[&name]);
                                    core.status.error(msg);
                                }
                                live => {
                                    if live == donn_core::LiveCheck::Unavailable {
                                        core.status.warn(i18n::REMOVE_LIVE_UNKNOWN);
                                    }
                                    self.modals.push(confirm_remove_broken(name));
                                }
                            }
                        }
                    }
                }
            }
            Action::SyncProfile => {
                if let Some(name) = core.selected_profile().map(str::to_string) {
                    match core.donn.sync(&name) {
                        Ok(report) => {
                            core.report_sync(&report);
                            core.refresh();
                        }
                        Err(e) => core.status.error(e.to_string()),
                    }
                }
            }
            Action::ToggleKeyReveal => {
                // 仅详情焦点时切换：避免列表焦点误触把 key 明文留在屏幕上
                if core.focus != PaneId::Detail {
                    return None;
                }
                if core.detail.revealed.take().is_none()
                    && let Some(name) = core.selected_profile().map(str::to_string)
                {
                    match core.donn.reveal_key(&name) {
                        Ok(secret) => core.detail.revealed = secret,
                        Err(e) => core.status.error(e.to_string()),
                    }
                }
            }
            Action::OpenEditor => {
                if let Some(name) = core.selected_profile() {
                    return Some(LoopCmd::EditSettings(name.to_string()));
                }
            }
            Action::OpenKeyUrl => {
                if let Some(err) = core.detail.error.clone() {
                    core.status.error(err);
                    return None;
                }
                let url = core
                    .detail
                    .view
                    .as_ref()
                    .and_then(|v| v.preset.key_url.clone());
                match url {
                    None => core.status.warn(i18n::ST_NO_KEY_URL),
                    Some(url) => core.open_url(&url),
                }
            }
            Action::OpenCopy => {
                if let Some(modal) = crate::tui::panes::detail::copy_modal(core) {
                    self.modals.push(modal);
                } else {
                    core.status.warn(i18n::ST_NOTHING_TO_COPY);
                }
            }
        }
        None
    }

    /// 设置面板打开时的动作子集。
    fn dispatch_settings(&mut self, action: Action) -> Option<LoopCmd> {
        let core = &mut self.core;
        match action {
            Action::Quit => return Some(LoopCmd::Quit),
            // S 再次 / Esc：关闭设置
            Action::Back | Action::OpenSettings => {
                core.status.clear();
                core.close_settings();
            }
            Action::MoveUp => core.nav_focused(Nav::By(-1)),
            Action::MoveDown => core.nav_focused(Nav::By(1)),
            Action::JumpTop => core.nav_focused(Nav::Top),
            Action::JumpBottom => core.nav_focused(Nav::Bottom),
            Action::Activate => {
                if let Some(modal) = crate::tui::panes::settings::activate_row(core) {
                    self.modals.push(modal);
                }
            }
            Action::Help => {
                self.modals
                    .push(Box::new(crate::tui::modals::help::HelpModal));
            }
            Action::OpenAdd => {
                core.enter_add(None);
            }
            // 焦点切换、doctor、删除、sync… 一律忽略，避免半开状态
            _ => {}
        }
        None
    }

    /// Enter 的面板语义。
    fn activate(&mut self) -> Option<LoopCmd> {
        let core = &mut self.core;
        match core.focus {
            PaneId::Left => {
                return core
                    .selected_profile()
                    .map(|name| LoopCmd::Launch(name.to_string()));
            }
            PaneId::Detail => {
                if let Some(modal) = crate::tui::panes::detail::activate_row(core) {
                    self.modals.push(modal);
                }
            }
            PaneId::Doctor => {}
        }
        None
    }
}

/// 命中区解析出的鼠标手势。
enum Gesture {
    Click,
    Scroll(i32),
}

impl App {
    /// 鼠标。右键点链接 = 复制。左键先按普通点击处理（选中行、切焦点）；点的是链接、
    /// 且这一下什么都没选中改变，才打开它——想选中 `base_url` 那行不会顺手弹出浏览器。
    pub fn on_mouse(&mut self, event: crossterm::event::MouseEvent) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let pos = Position::new(event.column, event.row);
        let link = match event.kind {
            MouseEventKind::Down(button) => {
                crate::tui::links::at(&self.core.links, pos).map(|url| (button, url.to_string()))
            }
            _ => None,
        };
        if let Some((MouseButton::Right, url)) = link {
            self.core.copy_text(i18n::COPY_LINK, url);
            return;
        }
        let before = self.core.selection();
        self.on_pointer(event, pos);
        if let Some((MouseButton::Left, url)) = link
            && self.core.selection() == before
        {
            self.core.open_url(&url);
        }
    }

    /// 点击选中并切焦点，滚轮滚动列表。弹窗打开时忽略（弹窗键盘优先）。
    fn on_pointer(&mut self, event: crossterm::event::MouseEvent, pos: Position) {
        use crossterm::event::{MouseButton, MouseEventKind};
        let core = &mut self.core;
        if !self.modals.is_empty() {
            return;
        }
        let gesture = match event.kind {
            MouseEventKind::Down(MouseButton::Left) => Gesture::Click,
            MouseEventKind::ScrollUp => Gesture::Scroll(-1),
            MouseEventKind::ScrollDown => Gesture::Scroll(1),
            _ => return,
        };

        // 左栏（Add 模式下点击/滚动 = 选渠道；设置模式下点击 = 关闭设置并选 profile）
        if core.hit.left_list.contains(pos) {
            if let Some(form) = core.add.as_mut() {
                match gesture {
                    Gesture::Click => {
                        form.providers_mut().click(core.hit.left_list, pos);
                        form.click_provider(form.providers().selected());
                        core.focus = PaneId::Left;
                    }
                    Gesture::Scroll(d) => form.scroll_providers(d),
                }
                return;
            }
            // 设置独占右栏：点/滚左栏 = 退出设置并导航 profile
            if core.settings.is_some() {
                core.close_settings();
                match gesture {
                    Gesture::Click => {
                        core.profiles.click(core.hit.left_list, pos);
                        core.reload_detail();
                    }
                    Gesture::Scroll(d) => {
                        core.profiles.move_by(d);
                        core.reload_detail();
                    }
                }
                return;
            }
            match gesture {
                Gesture::Click => {
                    core.focus = PaneId::Left;
                    core.profiles.click(core.hit.left_list, pos);
                    core.reload_detail();
                }
                Gesture::Scroll(d) => {
                    core.focus = PaneId::Left;
                    core.profiles.move_by(d);
                    core.reload_detail();
                }
            }
            return;
        }

        // 详情/设置行区（设置面板复用同一右栏区域）
        if core.hit.detail_list.contains(pos) && core.add.is_none() {
            if let Some(pane) = &mut core.settings {
                match gesture {
                    Gesture::Click => pane.click(core.hit.detail_list, pos),
                    Gesture::Scroll(d) => pane.nav(Nav::By(d)),
                }
                return;
            }
            if !core.detail_reachable() {
                return;
            }
            match gesture {
                Gesture::Click => {
                    core.focus = PaneId::Detail;
                    core.detail.rows.click(core.hit.detail_list, pos);
                }
                Gesture::Scroll(d) => {
                    core.focus = PaneId::Detail;
                    core.detail.rows.move_by(d);
                }
            }
            return;
        }

        // doctor 抽屉（设置 / Add 独占时不抢滚轮/点击）
        if core.settings.is_none()
            && core.add.is_none()
            && core.hit.doctor_list.contains(pos)
            && let Some(doctor) = &mut core.doctor
        {
            match gesture {
                Gesture::Click => {
                    core.focus = PaneId::Doctor;
                    doctor.checks.click(core.hit.doctor_list, pos);
                }
                Gesture::Scroll(d) => {
                    core.focus = PaneId::Doctor;
                    doctor.checks.move_by(d);
                }
            }
        }
    }
}

/// 删除确认弹窗。
/// full 模式会话在 profile 目录里 → 选择保留或一并删除；
/// shared 模式会话本就在 ~/.claude 不受影响 → 普通二次确认。
/// 调用前已拦截 Running；`live` 为调用方探测结果，避免重复 lsof。
fn confirm_remove(
    name: String,
    isolation: donn_core::Isolation,
    live: donn_core::LiveCheck,
) -> Box<dyn Modal> {
    let probe_note =
        (live == donn_core::LiveCheck::Unavailable).then(|| i18n::REMOVE_LIVE_UNKNOWN.to_string());
    if isolation == donn_core::Isolation::Full {
        let title = i18n::fill(i18n::REMOVE_Q, &[&name]);
        let options = vec![
            i18n::REMOVE_KEEP_SESSIONS.to_string(),
            i18n::REMOVE_PURGE.to_string(),
        ];
        return Box::new(Select::new(title, options, 0, move |core, picked| {
            do_remove(core, &name, picked == 0);
        }));
    }
    let mut body = vec![
        i18n::fill(i18n::REMOVE_Q, &[&name]),
        i18n::REMOVE_DETAIL.to_string(),
    ];
    if let Some(note) = probe_note {
        body.push(note);
    }
    Box::new(Confirm::new(i18n::REMOVE_TITLE, body, move |core| {
        do_remove(core, &name, false);
    }))
}

/// profile.toml 不可读时 isolation 未知：让用户显式选保留会话还是整目录删除。
fn confirm_remove_broken(name: String) -> Box<dyn Modal> {
    Box::new(Select::new(
        i18n::fill(i18n::REMOVE_Q, &[&name]),
        vec![
            i18n::REMOVE_KEEP_SESSIONS.to_string(),
            i18n::REMOVE_PURGE.to_string(),
        ],
        0,
        move |core, picked| {
            do_remove(core, &name, picked == 0);
        },
    ))
}

fn do_remove(core: &mut Core, name: &str, keep_sessions: bool) {
    match core.donn.remove(name, false, keep_sessions) {
        Ok(()) => {
            let msg = i18n::fill(i18n::ST_REMOVED, &[name]);
            core.status.ok(msg);
            core.refresh();
        }
        Err(e) => core.status.error(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::components::modal::ModalOutcome;
    use crossterm::event::KeyCode;
    use donn_core::{DonnHome, ProfileDraft};
    use tempfile::TempDir;

    #[test]
    fn broken_profile_can_be_removed_from_the_tui_while_keeping_sessions() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        let donn = Donn::with_home(home.clone()).unwrap();
        donn.create(&ProfileDraft {
            name: "broken".into(),
            preset: "official".into(),
            ..Default::default()
        })
        .unwrap();
        let session = home.claude_config_dir("broken").join("session.jsonl");
        std::fs::write(&session, "history").unwrap();
        std::fs::write(home.spec_file("broken"), "not [valid toml").unwrap();

        let mut app = App::new(donn);
        app.dispatch(Action::RemoveProfile);
        assert_eq!(
            app.modals.len(),
            1,
            "broken profile must reach a delete choice"
        );

        let mut modal = app.modals.pop().unwrap();
        let outcome = modal.handle(KeyCode::Enter.into(), &mut app.core);
        assert!(matches!(outcome, ModalOutcome::Close));
        assert!(!home.spec_file("broken").exists());
        assert!(session.exists(), "default delete choice keeps session data");
    }
}
