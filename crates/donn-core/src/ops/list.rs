//! 读路径：dashboard 需要的全部视图，从 spec 和 preset 推导。

use crate::claude::{self, SettingsView};
use crate::config::Defaults;
use crate::error::Result;
use crate::keys::SlotMap;
use crate::preset::{Preset, PresetCatalog};
use crate::render::{self, ValueSource, effective_base_url, effective_models, global_models};
use crate::secret::{KeyState, Secret};
use crate::spec::ProfileSpec;

use super::Donn;
use super::update::{Drift, drift_for};

/// 左面板一行需要的信息。
#[derive(Debug, Clone)]
pub struct ProfileCard {
    pub name: String,
    pub preset: String,
    /// 生效 endpoint（spec 覆盖 > preset）。
    pub base_url: Option<String>,
    /// 生效 model（spec 覆盖 > `[defaults.env]` > preset）。
    pub models: SlotMap,
    pub aliases: Vec<String>,
    pub key: KeyState,
    /// Some = spec、settings 或 preset 加载失败的原因。坏 profile 必须出现在列表里，
    /// 可见也能诊断（和 doctor 同源），绝不静默消失。
    pub broken: Option<String>,
}

/// 右面板详情：spec 加上推导出来的信息。
#[derive(Debug, Clone)]
pub struct ProfileView {
    pub spec: ProfileSpec,
    pub preset: Preset,
    pub key: KeyState,
    pub base_url: Option<String>,
    /// 生效 model（spec 覆盖 > `[defaults.env]` > preset）。
    pub models: SlotMap,
    /// `[defaults.env]` 提供的 slot；展示层据此把来源标成全局默认，而不是 preset 默认。
    pub global_models: SlotMap,
    /// 生效的思考强度与来源。
    pub effective_effort: Option<(String, ValueSource)>,
    /// 生效的上下文窗口与来源。
    pub effective_max_context: Option<(u64, ValueSource)>,
    pub drift: Vec<Drift>,
}

impl Donn {
    /// 显式取回已经配置的 key（只供 UI 明文查看，例如 `^t` 小眼睛）。
    /// 返回 `Secret`：调用方决定什么时候 reveal，绝不落日志。
    pub fn reveal_key(&self, name: &str) -> Result<Option<Secret>> {
        let settings = claude::read_json(&self.home.settings_file(name))?;
        Ok(SettingsView::new(&settings).secret())
    }

    pub fn cards(&self) -> Result<Vec<ProfileCard>> {
        let catalog = self.presets();
        let defaults = self.config()?.defaults;
        let mut out = Vec::new();
        for name in self.profile_names()? {
            out.push(match self.card(&catalog, &defaults, &name) {
                Ok(card) => card,
                // 单个 profile 坏了不拖垮整个列表，但必须以坏状态可见
                Err(e) => ProfileCard {
                    name: name.clone(),
                    preset: String::new(),
                    base_url: None,
                    models: SlotMap::default(),
                    aliases: Vec::new(),
                    key: KeyState::NotNeeded,
                    broken: Some(e.to_string()),
                },
            });
        }
        Ok(out)
    }

    fn card(
        &self,
        catalog: &PresetCatalog,
        defaults: &Defaults,
        name: &str,
    ) -> Result<ProfileCard> {
        let spec = self.spec(name)?;
        let preset = catalog.get(&spec.preset)?;
        let settings = claude::read_json(&self.home.settings_file(name))?;
        // 只校验能不能解析：坏 .claude.json 会让 sync 失败，必须以 broken 状态可见
        claude::read_json(&self.home.claude_json_file(name))?;
        Ok(ProfileCard {
            base_url: effective_base_url(&spec, preset),
            models: effective_models(&spec, preset, defaults),
            key: SettingsView::new(&settings).key_state(preset.auth_mode.needs_key()),
            aliases: spec.wrapper.aliases.clone(),
            preset: spec.preset.clone(),
            name: name.to_string(),
            broken: None,
        })
    }

    pub fn inspect(&self, name: &str) -> Result<ProfileView> {
        let spec = self.spec(name)?;
        let catalog = self.presets();
        let preset = catalog.get(&spec.preset)?.clone();
        let settings = claude::read_json(&self.home.settings_file(name))?;
        claude::read_json(&self.home.claude_json_file(name))?;
        let defaults = self.config()?.defaults;
        let drift = drift_for(&spec, &preset, &settings, &defaults)?;
        Ok(ProfileView {
            key: SettingsView::new(&settings).key_state(preset.auth_mode.needs_key()),
            base_url: effective_base_url(&spec, &preset),
            models: effective_models(&spec, &preset, &defaults),
            global_models: global_models(&defaults),
            effective_effort: render::effective_effort(&spec, &preset, &defaults),
            effective_max_context: render::effective_max_context(&spec, &preset, &defaults),
            drift,
            spec,
            preset,
        })
    }
}
