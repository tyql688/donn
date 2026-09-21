//! Add 面板模式：`a` 之后 dashboard 整体切换——左栏渠道列表（选择实时联动），
//! 右栏整块是表单 + 底部生效预览。
//!
//! 本文件只有表单状态与按键逻辑；渲染在 [`view`]。

pub mod view;

pub use view::render;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use donn_core::keys::{Effort, ModelSlot};
use donn_core::preset::Preset;
use donn_core::{Defaults, Isolation, ProfileDraft, Secret, SlotMap};
use std::collections::BTreeSet;

use crate::tui::app::Core;
use crate::tui::components::list::SelectList;
use crate::tui::components::text_input::TextInput;
use crate::tui::i18n::{self, fill};
use crate::tui::modals::preset_pick::PresetPick;

/// Add 的两个阶段：先选渠道，再填表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 焦点在左栏渠道列表：↑↓ 浏览、enter 选定、tab 过滤。
    PickProvider,
    /// 渠道已选定：右栏填表；esc 返回重选渠道。
    EditForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Key,
    BaseUrl,
    Model(ModelSlot),
    MaxContext,
    Effort,
    Isolation,
    Aliases,
    Create,
}

/// 表单处理结果。
pub enum FormOutcome {
    Keep,
    /// 退出 Add 模式（取消或创建成功）。
    Close,
    /// 叠加弹窗（渠道过滤选择器 / 思考强度单选）。
    OpenPicker(Box<dyn crate::tui::components::modal::Modal>),
}

pub struct AddForm {
    stage: Stage,
    /// 创建时选的渠道（列表状态只在 Add 里，不单独做「presets 浏览模式」）。
    providers: SelectList<Preset>,
    /// 打开表单时的全局默认快照：预览要按 `[defaults.env]` 的槽位算生效模型。
    defaults: Defaults,
    existing_names: BTreeSet<String>,
    name: TextInput,
    key: TextInput,
    base_url: TextInput,
    slots: [TextInput; ModelSlot::ALL.len()],
    /// sonnet 那个模型的上下文窗口。这行只在 sonnet 是没人知道窗口的模型时出现，
    /// 出现时预填默认值；用户动过就不再自动改。
    max_context: TextInput,
    window_touched: bool,
    effort: Effort,
    isolation: Isolation,
    aliases: TextInput,
    field_idx: usize,
    error: Option<String>,
    /// name/base_url 一旦手动编辑过就不再跟随渠道预填。
    name_touched: bool,
    base_url_touched: bool,
    /// 在选渠道阶段退出 Add：有改动时第一次 Esc 只提示，第二次才丢弃。
    pending_cancel: bool,
    /// 已填内容归属的渠道 key。Esc 回列表再确认**同一**渠道 = 内容保留；
    /// 确认**不同**渠道 = 覆盖字段全部重置——key/模型/别名绝不跨渠道残留。
    filled_for: Option<String>,
}

impl AddForm {
    pub fn new(core: &Core, preset_key: Option<String>) -> Self {
        let items = core.donn.presets().all().to_vec();
        let mut providers = SelectList::new(items);
        if let Some(key) = preset_key
            && let Some(idx) = providers.items.iter().position(|p| p.key == key)
        {
            providers.select(idx);
        }
        let mut form = Self {
            stage: Stage::PickProvider,
            providers,
            defaults: core.donn.config().map(|c| c.defaults).unwrap_or_default(),
            existing_names: core
                .profiles
                .items
                .iter()
                .map(|card| card.name.clone())
                .collect(),
            name: TextInput::default(),
            key: TextInput::masked(""),
            base_url: TextInput::default(),
            slots: Default::default(),
            max_context: TextInput::default(),
            window_touched: false,
            effort: Effort::Auto,
            isolation: Isolation::default(),
            aliases: TextInput::default(),
            field_idx: 0,
            error: None,
            name_touched: false,
            base_url_touched: false,
            pending_cancel: false,
            filled_for: None,
        };
        form.prefill_from_provider();
        form
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn providers_mut(&mut self) -> &mut SelectList<Preset> {
        &mut self.providers
    }

    pub fn providers(&self) -> &SelectList<Preset> {
        &self.providers
    }

    /// 鼠标点击左栏：选中渠道并回到选渠道阶段。
    pub fn click_provider(&mut self, idx: usize) {
        self.providers.select(idx);
        self.prefill_from_provider();
        self.stage = Stage::PickProvider;
        self.error = None;
    }

    /// 鼠标滚轮：选渠道阶段滚动列表。
    pub fn scroll_providers(&mut self, delta: i32) {
        if self.stage != Stage::PickProvider {
            return;
        }
        self.providers.move_by(delta);
        self.prefill_from_provider();
    }

    /// 过滤选择器回写：选中即进入填表。
    pub fn set_provider_key(&mut self, key: &str) {
        if let Some(idx) = self.providers.items.iter().position(|p| p.key == key) {
            self.providers.select(idx);
            self.enter_edit();
        }
    }

    /// 确认当前渠道，进入填表阶段。
    fn enter_edit(&mut self) {
        let key = self.provider().key.clone();
        if self.filled_for.as_deref() != Some(key.as_str()) {
            self.reset_overrides();
            self.filled_for = Some(key);
        }
        self.stage = Stage::EditForm;
        self.sync_window();
        self.field_idx = 0;
        self.error = None;
    }

    fn reset_overrides(&mut self) {
        self.key = TextInput::masked("");
        self.slots = Default::default();
        self.max_context = TextInput::default();
        self.window_touched = false;
        self.effort = Effort::Auto;
        self.isolation = Isolation::default();
        self.aliases = TextInput::default();
        self.name_touched = false;
        self.base_url_touched = false;
        self.pending_cancel = false;
        self.prefill_from_provider();
    }

    pub(crate) fn provider(&self) -> &Preset {
        let i = self
            .providers
            .selected()
            .min(self.providers.items.len().saturating_sub(1));
        &self.providers.items[i]
    }

    fn prefill_from_provider(&mut self) {
        let p = self.provider().clone();
        if !self.name_touched {
            self.name.set(
                if p.key == "custom" || self.existing_names.contains(&p.key) {
                    String::new()
                } else {
                    p.key
                },
            );
        }
        if !self.base_url_touched {
            // 空值表示跟随 preset；默认 URL 只作为预览/占位显示，不伪装成覆盖。
            self.base_url.set(String::new());
        }
    }

    fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Name];
        if self.provider().auth_mode.needs_key() {
            fields.push(Field::Key);
        }
        fields.push(Field::BaseUrl);
        for slot in ModelSlot::ALL {
            fields.push(Field::Model(slot));
            // 窗口是 sonnet 那个模型的属性：紧跟在它下面，需要时才出现
            if slot == ModelSlot::Sonnet && self.window_visible() {
                fields.push(Field::MaxContext);
            }
        }
        fields.push(Field::Effort);
        fields.push(Field::Isolation);
        fields.push(Field::Aliases);
        fields.push(Field::Create);
        fields
    }

    fn current_field(&self) -> Field {
        let fields = self.fields();
        fields[self.field_idx.min(fields.len() - 1)]
    }

    fn input_of(&mut self, field: Field) -> Option<&mut TextInput> {
        match field {
            Field::Name => Some(&mut self.name),
            Field::Key => Some(&mut self.key),
            Field::BaseUrl => Some(&mut self.base_url),
            Field::Model(slot) => Some(&mut self.slots[slot.index()]),
            Field::MaxContext => Some(&mut self.max_context),
            Field::Aliases => Some(&mut self.aliases),
            _ => None,
        }
    }

    /// 生效的 sonnet id（填了用填的，否则 preset 默认）。
    fn sonnet(&self) -> String {
        let typed = self.slots[ModelSlot::Sonnet.index()].value().trim();
        if typed.is_empty() {
            self.provider()
                .models
                .get(ModelSlot::Sonnet)
                .unwrap_or_default()
                .to_string()
        } else {
            typed.to_string()
        }
    }

    fn needs_window(&self) -> bool {
        crate::tui::modals::model_pick::needs_window(self.provider(), &self.sonnet())
    }

    fn window_visible(&self) -> bool {
        self.needs_window() || !self.max_context.is_empty()
    }

    /// sonnet 变了之后调。窗口是模型的属性：换到 preset 候选就清掉（用户填过的也清，
    /// 那是给上一个模型填的）；换到没人知道窗口的模型就预填默认值，用户亲手改过则留着。
    pub(crate) fn sync_window(&mut self) {
        if !self.needs_window() {
            self.max_context.set("");
            self.window_touched = false;
        } else if !self.window_touched {
            self.max_context
                .set(crate::tui::panes::detail::DEFAULT_CUSTOM_WINDOW);
        }
    }

    fn window(&self) -> Result<Option<u64>, String> {
        crate::tui::panes::detail::parse_tokens(self.max_context.value())
    }

    fn alias_list(&self) -> Vec<String> {
        self.aliases
            .value()
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn dirty(&self) -> bool {
        self.name_touched
            || self.base_url_touched
            || !self.key.is_empty()
            || !self.aliases.is_empty()
            || self.slots.iter().any(|s| !s.is_empty())
            || !self.max_context.is_empty()
            || self.effort != Effort::Auto
            || self.isolation != Isolation::default()
    }

    /// 生效值（表单覆盖 > preset）。
    fn effective_base_url(&self) -> String {
        let v = self.base_url.value().trim();
        if v.is_empty() {
            self.provider().base_url.clone().unwrap_or_default()
        } else {
            v.to_string()
        }
    }

    /// 生效模型：表单填写 > `[defaults.env]` > preset（与 render 同序）。
    fn effective_model(&self, slot: ModelSlot) -> Option<String> {
        let v = self.slots[slot.index()].value().trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
        donn_core::render::global_models(&self.defaults)
            .over(&self.provider().models)
            .get(slot)
            .map(str::to_string)
    }

    fn validate(&self, core: &Core, field: Field) -> Option<String> {
        match field {
            Field::Name => {
                let name = self.name.value().trim();
                if name.is_empty() {
                    return Some(i18n::V_NAME_REQUIRED.into());
                }
                if donn_core::home::validate_name(name).is_err() {
                    return Some(i18n::V_NAME_FORMAT.into());
                }
                if core.donn.exists(name) {
                    return Some(fill(i18n::V_NAME_EXISTS, &[name]));
                }
                None
            }
            Field::BaseUrl => {
                if self.provider().base_url.is_none()
                    && self.provider().auth_mode.needs_key()
                    && self.base_url.value().trim().is_empty()
                {
                    return Some(i18n::V_BASE_URL_REQUIRED.into());
                }
                None
            }
            Field::MaxContext => {
                crate::tui::panes::detail::parse_tokens(self.max_context.value()).err()
            }
            Field::Aliases => {
                let bin_dir = match core.donn.bin_dir() {
                    Ok(bin_dir) => bin_dir,
                    Err(e) => return Some(e.to_string()),
                };
                for alias in self.alias_list() {
                    if let Err(e) = donn_core::wrapper::check_alias_conflict(
                        &bin_dir,
                        &alias,
                        self.name.value(),
                    ) {
                        return Some(e.to_string());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn first_error(&self, core: &Core) -> Option<String> {
        self.fields()
            .into_iter()
            .find_map(|f| self.validate(core, f))
    }

    fn submit(&mut self, core: &mut Core) -> Result<(), String> {
        if let Some(error) = self.first_error(core) {
            return Err(error);
        }
        let preset = self.provider().clone();
        let mut models = SlotMap::default();
        for slot in ModelSlot::ALL {
            let value = self.slots[slot.index()].value().trim().to_string();
            // 与 preset 相同视为未覆盖（保持跟随 preset 更新）
            if !value.is_empty() && preset.models.get(slot) != Some(value.as_str()) {
                models.set(slot, Some(value));
            }
        }
        let base_url = Some(self.base_url.value().trim().to_string())
            .filter(|u| !u.is_empty() && preset.base_url.as_deref() != Some(u.as_str()));
        let name = self.name.value().trim().to_string();
        // 模型套餐 env 不写 intent：create/sync 时 render 按 sonnet 生效 id 注入
        let env = self
            .effort
            .env_value()
            .map(|level| vec![(donn_core::keys::EFFORT.to_string(), level.to_string())])
            .unwrap_or_default();
        let draft = ProfileDraft {
            name: name.clone(),
            preset: preset.key,
            // trim：粘贴带尾随空格/换行的 key 直接存会认证失败
            key: Secret::new(self.key.value().trim()),
            base_url,
            models,
            model_windows: self
                .window()?
                .map(|tokens| (self.sonnet(), tokens))
                .into_iter()
                .collect(),
            env,
            isolation: self.isolation,
            aliases: self.alias_list(),
        };
        let needs_key = preset.auth_mode.needs_key();
        let key_missing = needs_key && self.key.value().trim().is_empty();
        let receipt = core.donn.create(&draft).map_err(|e| e.to_string())?;
        let command = receipt
            .wrapper_paths
            .first()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name.clone());
        let mut warnings = Vec::new();
        if key_missing {
            warnings.push(fill(i18n::ST_CREATED_NO_KEY, &[&name]));
        }
        if !receipt.bin_dir_in_path {
            warnings.push(fill(
                i18n::ST_NOT_ON_PATH,
                &[&receipt.bin_dir.display().to_string()],
            ));
        }
        if warnings.is_empty() {
            let msg = fill(i18n::ST_PROFILE_READY, &[&command]);
            core.status.ok(msg);
        } else {
            core.status.warn(warnings.join(" · "));
        }
        core.refresh();
        core.profiles.select_where(|c| c.name == name);
        core.reload_detail();
        Ok(())
    }

    pub fn handle(&mut self, key: KeyEvent, core: &mut Core) -> FormOutcome {
        if self.stage == Stage::PickProvider {
            return self.handle_pick(key, core);
        }
        let field = self.current_field();
        let field_count = self.fields().len();
        if key.code != KeyCode::Esc {
            self.pending_cancel = false;
        }
        match key.code {
            KeyCode::Char('?') => {
                return FormOutcome::OpenPicker(Box::new(crate::tui::modals::help::HelpModal));
            }
            // Esc：返回渠道选择（不丢已填内容）
            KeyCode::Esc => {
                self.stage = Stage::PickProvider;
                self.error = None;
                core.focus = crate::tui::app::PaneId::Left;
                return FormOutcome::Keep;
            }
            // ^t：api key 明文/掩码切换（小眼睛）
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.key.toggle_mask();
                return FormOutcome::Keep;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return match self.submit(core) {
                    Ok(()) => FormOutcome::Close,
                    Err(error) => {
                        self.error = Some(error);
                        FormOutcome::Keep
                    }
                };
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.field_idx = self.field_idx.saturating_sub(1);
                return FormOutcome::Keep;
            }
            KeyCode::Down => {
                self.field_idx = (self.field_idx + 1).min(field_count - 1);
                return FormOutcome::Keep;
            }
            KeyCode::Enter => {
                if field == Field::Create {
                    return match self.submit(core) {
                        Ok(()) => FormOutcome::Close,
                        Err(error) => {
                            self.error = Some(error);
                            FormOutcome::Keep
                        }
                    };
                }
                // 思考强度：Enter 打开单选弹窗（与详情页一致），选中回写表单
                if field == Field::Effort {
                    let (options, selected) =
                        crate::tui::components::effort_options(i18n::EFFORT_AUTO, self.effort);
                    return FormOutcome::OpenPicker(Box::new(
                        crate::tui::components::modal::Select::new(
                            i18n::F_EFFORT,
                            options,
                            selected,
                            |core, picked| {
                                if let Some(form) = core.add.as_mut() {
                                    form.effort = Effort::ALL[picked];
                                }
                            },
                        ),
                    ));
                }
                if field == Field::Isolation {
                    let selected = usize::from(self.isolation == Isolation::Shared);
                    return FormOutcome::OpenPicker(Box::new(
                        crate::tui::components::modal::Select::new(
                            i18n::F_ISOLATION,
                            vec![i18n::ISO_FULL.to_string(), i18n::ISO_SHARED.to_string()],
                            selected,
                            |core, picked| {
                                if let Some(form) = core.add.as_mut() {
                                    form.isolation = if picked == 1 {
                                        Isolation::Shared
                                    } else {
                                        Isolation::Full
                                    };
                                }
                            },
                        ),
                    ));
                }
                // 模型槽：可搜索可填入
                if let Field::Model(slot) = field {
                    let current = self.slots[slot.index()].value().to_string();
                    let title = i18n::fill(
                        i18n::EDIT_TITLE,
                        &[
                            &i18n::fill(i18n::ROW_MODEL, &[slot.label()]),
                            self.name.value(),
                        ],
                    );
                    return FormOutcome::OpenPicker(crate::tui::modals::model_pick::open(
                        title,
                        self.provider(),
                        slot,
                        &current,
                        move |core, pick| {
                            if let Some(form) = core.add.as_mut() {
                                let preset = form.provider().clone();
                                crate::tui::modals::model_pick::apply_to_slots(
                                    &preset,
                                    &mut form.slots,
                                    slot,
                                    pick,
                                );
                                form.sync_window();
                            }
                            None
                        },
                    ));
                }
                self.field_idx = (self.field_idx + 1).min(field_count - 1);
                return FormOutcome::Keep;
            }
            _ => {}
        }
        if let Some(input) = self.input_of(field)
            && input.handle_key(&key)
        {
            self.error = None;
            match field {
                Field::Name => self.name_touched = true,
                Field::BaseUrl => self.base_url_touched = true,
                Field::MaxContext => self.window_touched = true,
                Field::Model(ModelSlot::Sonnet) => self.sync_window(),
                _ => {}
            }
        }
        FormOutcome::Keep
    }

    /// 阶段 1：选渠道。焦点语义在左栏列表。
    fn handle_pick(&mut self, key: KeyEvent, core: &mut Core) -> FormOutcome {
        if key.code != KeyCode::Esc {
            self.pending_cancel = false;
        }
        match key.code {
            KeyCode::Char('?') => {
                return FormOutcome::OpenPicker(Box::new(crate::tui::modals::help::HelpModal));
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if !self.dirty() || self.pending_cancel {
                    return FormOutcome::Close;
                }
                self.pending_cancel = true;
                self.error = Some(i18n::ESC_AGAIN_DISCARD.into());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.providers.move_by(-1);
                self.prefill_from_provider();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.providers.move_by(1);
                self.prefill_from_provider();
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.providers.select(0);
                self.prefill_from_provider();
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.providers
                    .select(self.providers.items.len().saturating_sub(1));
                self.prefill_from_provider();
            }
            KeyCode::Tab | KeyCode::Char('/') => {
                return FormOutcome::OpenPicker(Box::new(PresetPick::new(
                    self.providers.items.clone(),
                    &self.provider().key,
                )));
            }
            KeyCode::Char('o') => match self.provider().key_url.clone() {
                None => core.status.warn(i18n::ST_NO_KEY_URL),
                Some(url) => core.open_url(&url),
            },
            KeyCode::Enter => {
                self.enter_edit();
                core.focus = crate::tui::app::PaneId::Detail;
            }
            _ => {}
        }
        FormOutcome::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};
    use tempfile::TempDir;

    fn core(dir: &TempDir) -> Core {
        let donn = donn_core::Donn::with_home(donn_core::DonnHome::for_test(dir.path())).unwrap();
        Core::new(donn)
    }

    fn press(form: &mut AddForm, core: &mut Core, code: KeyCode) {
        form.handle(KeyEvent::from(code), core);
    }

    fn type_str(form: &mut AddForm, core: &mut Core, s: &str) {
        for ch in s.chars() {
            press(form, core, KeyCode::Char(ch));
        }
    }

    #[test]
    fn switching_preset_resets_overrides_same_preset_keeps() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        let mut form = AddForm::new(&core, Some("kimi-cn".into()));

        // 进 kimi 表单，填 key 与 haiku 槽覆盖
        press(&mut form, &mut core, KeyCode::Enter);
        assert_eq!(form.stage(), Stage::EditForm);
        while form.current_field() != Field::Key {
            press(&mut form, &mut core, KeyCode::Down);
        }
        type_str(&mut form, &mut core, "sk-kimi-secret");
        while form.current_field() != Field::Model(ModelSlot::Haiku) {
            press(&mut form, &mut core, KeyCode::Down);
        }
        type_str(&mut form, &mut core, "kimi-k3");
        assert!(!form.key.is_empty());

        // Esc 回列表，确认同一渠道 → 内容保留（原设计意图）
        press(&mut form, &mut core, KeyCode::Esc);
        press(&mut form, &mut core, KeyCode::Enter);
        assert_eq!(form.key.value(), "sk-kimi-secret");
        assert_eq!(form.slots[ModelSlot::Haiku.index()].value(), "kimi-k3");

        // Esc 回列表，换到另一渠道确认 → 覆盖字段全部重置，key 绝不跨渠道残留
        press(&mut form, &mut core, KeyCode::Esc);
        form.set_provider_key("zai");
        assert_eq!(form.stage(), Stage::EditForm);
        assert!(
            form.key.is_empty(),
            "api key must never leak across providers"
        );
        assert!(form.slots.iter().all(|s| s.is_empty()));
        assert!(form.aliases.is_empty());
        assert_eq!(form.effort, Effort::Auto);
        assert_eq!(form.name.value(), "zai", "name 重新预填为新渠道");
    }

    #[test]
    fn window_field_follows_the_sonnet_model() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        let mut form = AddForm::new(&core, Some("kimi-cn".into()));
        press(&mut form, &mut core, KeyCode::Enter);
        let sonnet = Field::Model(ModelSlot::Sonnet);
        let after_sonnet = |form: &AddForm| {
            let fields = form.fields();
            fields[fields.iter().position(|f| *f == sonnet).unwrap() + 1]
        };
        // preset 候选自带窗口：没有窗口行
        assert!(!form.fields().contains(&Field::MaxContext));

        while form.current_field() != sonnet {
            press(&mut form, &mut core, KeyCode::Down);
        }
        // 敲一个没人知道窗口的模型：窗口行紧跟 sonnet 冒出来，里面是真实的默认值
        type_str(&mut form, &mut core, "gw/x");
        assert_eq!(after_sonnet(&form), Field::MaxContext);
        assert_eq!(form.max_context.value(), "262144");
        assert_eq!(form.window(), Ok(Some(262_144)));

        // 往下一格就是它，直接改
        press(&mut form, &mut core, KeyCode::Down);
        assert_eq!(form.current_field(), Field::MaxContext);
        for _ in 0..6 {
            press(&mut form, &mut core, KeyCode::Backspace);
        }
        type_str(&mut form, &mut core, "1000000");
        assert_eq!(form.window(), Ok(Some(1_000_000)));

        // 用户改过之后，再动 sonnet 不会把他的值冲掉
        press(&mut form, &mut core, KeyCode::Up);
        type_str(&mut form, &mut core, "y");
        assert_eq!(form.max_context.value(), "1000000");
    }

    #[test]
    fn a_window_typed_for_one_model_is_not_saved_under_another() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        let mut form = AddForm::new(&core, Some("kimi-cn".into()));
        press(&mut form, &mut core, KeyCode::Enter);
        while form.current_field() != Field::Model(ModelSlot::Sonnet) {
            press(&mut form, &mut core, KeyCode::Down);
        }
        type_str(&mut form, &mut core, "gw/x");
        press(&mut form, &mut core, KeyCode::Down);
        type_str(&mut form, &mut core, "0"); // 亲手改过：2621440
        // sonnet 换成 preset 候选：给 gw/x 填的窗口不能落到它名下
        press(&mut form, &mut core, KeyCode::Up);
        for _ in 0.."gw/x".len() {
            press(&mut form, &mut core, KeyCode::Backspace);
        }
        type_str(&mut form, &mut core, "kimi-k3");
        assert!(!form.fields().contains(&Field::MaxContext));
        assert_eq!(form.window(), Ok(None));
    }

    #[test]
    fn a_preset_whose_default_model_has_no_window_starts_prefilled() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        let mut form = AddForm::new(&core, Some("ollama".into()));
        press(&mut form, &mut core, KeyCode::Enter);
        assert!(form.fields().contains(&Field::MaxContext));
        assert_eq!(form.max_context.value(), "262144");
    }

    #[test]
    fn window_field_disappears_when_sonnet_goes_back_to_a_preset_model() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        let mut form = AddForm::new(&core, Some("kimi-cn".into()));
        press(&mut form, &mut core, KeyCode::Enter);
        while form.current_field() != Field::Model(ModelSlot::Sonnet) {
            press(&mut form, &mut core, KeyCode::Down);
        }
        type_str(&mut form, &mut core, "z");
        assert!(form.fields().contains(&Field::MaxContext));
        press(&mut form, &mut core, KeyCode::Backspace);
        assert!(!form.fields().contains(&Field::MaxContext));
        assert_eq!(form.window(), Ok(None));
    }

    #[test]
    fn custom_and_existing_provider_names_start_empty_and_aliases_accept_spaces() {
        let dir = TempDir::new().unwrap();
        let mut core = core(&dir);
        core.donn
            .create(&ProfileDraft {
                name: "zai".into(),
                preset: "zai".into(),
                ..Default::default()
            })
            .unwrap();
        core.refresh();

        let existing = AddForm::new(&core, Some("zai".into()));
        assert!(existing.name.is_empty());
        assert!(existing.base_url.is_empty());

        let mut custom = AddForm::new(&core, Some("custom".into()));
        assert!(custom.name.is_empty());
        custom.aliases.set("one two,three");
        assert_eq!(custom.alias_list(), ["one", "two", "three"]);
    }
}
