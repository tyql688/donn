//! 读路径：dashboard 所需的全部视图，从 spec + preset 推导。

use crate::claude::{self, view::SettingsView};
use crate::config::GlobalConfig;
use crate::error::Result;
use crate::keys::SlotMap;
use crate::preset::Preset;
use crate::render::{effective_base_url, effective_models, global_models};
use crate::secret::{KeyState, Secret};
use crate::spec::ProfileSpec;

use super::Donn;
use super::update::{Drift, drift_for};

/// 左面板一行所需信息。
#[derive(Debug, Clone)]
pub struct ProfileCard {
    pub name: String,
    pub preset: String,
    /// 生效端点（spec 覆盖 > preset）。
    pub base_url: Option<String>,
    /// 生效模型（spec 覆盖 > `[defaults.env]` > preset）。
    pub models: SlotMap,
    pub aliases: Vec<String>,
    pub key: KeyState,
    /// Some = spec/settings/preset 加载失败的原因。坏 profile 必须进列表
    /// 可见可诊断（与 doctor 同源），绝不静默消失。
    pub broken: Option<String>,
}

/// 右面板详情：spec + 推导信息。
#[derive(Debug, Clone)]
pub struct ProfileView {
    pub spec: ProfileSpec,
    pub preset: Preset,
    pub key: KeyState,
    pub base_url: Option<String>,
    /// 生效模型（spec 覆盖 > `[defaults.env]` > preset）。
    pub models: SlotMap,
    /// `[defaults.env]` 提供的槽位；展示层据此把来源标成全局默认而不是 preset 默认。
    pub global_models: SlotMap,
    pub drift: Vec<Drift>,
}

impl Donn {
    /// 显式取回已配置的 key（仅供 UI 明文查看，如 ^t 小眼睛）。
    /// 返回 Secret：调用方决定何时 reveal，绝不落日志。
    pub fn reveal_key(&self, name: &str) -> Result<Option<Secret>> {
        let settings = claude::read_json(&self.home.settings_file(name))?;
        Ok(SettingsView::new(&settings).secret())
    }

    pub fn cards(&self) -> Result<Vec<ProfileCard>> {
        let catalog = self.presets();
        let defaults = GlobalConfig::load(&self.home)?.defaults;
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
        catalog: &crate::preset::PresetCatalog,
        defaults: &crate::config::Defaults,
        name: &str,
    ) -> Result<ProfileCard> {
        let spec = self.spec(name)?;
        let preset = catalog.get(&spec.preset)?;
        let settings = claude::read_json(&self.home.settings_file(name))?;
        // 只校验可解析：坏 .claude.json 会让 sync 失败，必须以 broken 状态可见
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
        let defaults = GlobalConfig::load(&self.home)?.defaults;
        let drift = drift_for(&spec, &preset, &settings, &defaults)?;
        Ok(ProfileView {
            key: SettingsView::new(&settings).key_state(preset.auth_mode.needs_key()),
            base_url: effective_base_url(&spec, &preset),
            models: effective_models(&spec, &preset, &defaults),
            global_models: global_models(&defaults),
            drift,
            spec,
            preset,
        })
    }
}
