//! render：`(spec, preset, secret, defaults) → Rendered`，纯函数。
//!
//! 生成 settings.json / .claude.json 中 donn 拥有的那部分内容。层序（后写的盖先写的）
//! 就是 [`render`] 函数体的顺序，文字说明在 docs/ARCHITECTURE.md。
//! preset 的兼容性怪癖由 flags 数据表达，这里不特判任何 provider。

use crate::config::Defaults;
use crate::keys::{self, ModelSlot, SlotMap};
use crate::knobs::{BOOL_KNOBS, KnobTarget, Knobs, VALUE_KNOBS};
use crate::preset::{AuthMode, Preset};
use crate::secret::Secret;
use crate::spec::{Footprint, Isolation, ProfileSpec};

/// 渲染结果：待写入的 donn 拥有内容。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rendered {
    /// settings.json 顶层 `env` 对象内的键值（有序）。
    pub env: Vec<(String, String)>,
    /// settings.json 顶层字段：settings 类旋钮 + `[defaults.settings]`。shared 模式为空。
    pub settings_top: Vec<(String, serde_json::Value)>,
    /// `permissions.defaultMode` 的值（权限模式旋钮；None = 不管理该子键）。
    pub permissions_default_mode: Option<String>,
    /// `.claude.json` 顶层 managed 键名。
    pub claude_json: Vec<String>,
    /// api_key 写入时，确认屏白名单需要的 key 后 20 位。
    pub approved_key_suffix: Option<String>,
}

impl Rendered {
    /// 本次实际写入的全部键名 → spec 的新 footprint。
    pub fn footprint(&self) -> Footprint {
        Footprint {
            settings_env: self.env.iter().map(|(k, _)| k.clone()).collect(),
            settings_top: self.settings_top.iter().map(|(k, _)| k.clone()).collect(),
            claude_json: self.claude_json.clone(),
            permissions_top: self
                .permissions_default_mode
                .iter()
                .map(|_| crate::keys::PERMISSIONS_DEFAULT_MODE.to_string())
                .collect(),
        }
    }
}

/// 生效端点（spec 覆盖 > preset）。展示层与渲染共用——优先级只在此定义一次。
pub fn effective_base_url(spec: &ProfileSpec, preset: &Preset) -> Option<String> {
    spec.intent
        .base_url
        .clone()
        .filter(|u| !u.is_empty())
        .or_else(|| preset.base_url.clone())
}

/// `[defaults.env]` 里写的模型槽键（全局默认层）。
pub fn global_models(defaults: &Defaults) -> SlotMap {
    let mut out = SlotMap::default();
    for slot in ModelSlot::ALL {
        out.set(slot, defaults.env.get(slot.env_key()).cloned());
    }
    out
}

/// 生效模型槽位：spec 覆盖 > `[defaults.env]` > preset。与 [`render`] 写入的槽位一致，
/// 展示层只能用这个函数。
pub fn effective_models(spec: &ProfileSpec, preset: &Preset, defaults: &Defaults) -> SlotMap {
    spec.intent
        .models
        .over(&global_models(defaults).over(&preset.models))
}

/// 按优先级渲染。所有 env 值为字符串类型（Claude Code 对数字型 env 兼容字符串）。
pub fn render(
    spec: &ProfileSpec,
    preset: &Preset,
    secret: Option<&Secret>,
    defaults: &Defaults,
) -> Rendered {
    let mut env = EnvBuilder::default();

    // 端点：spec 覆盖 > preset
    let base_url = effective_base_url(spec, preset);
    if let Some(url) = &base_url {
        env.set(keys::BASE_URL, url);
    }

    // 全局旋钮·第一遍：生效值是用户值 ?? 默认值；这是最低层，preset 可覆盖。
    // tool search 不在这一遍：未拨动时不写键，由上游按端点主机判定（见 [`keys::TOOL_SEARCH`]）。
    if base_url.is_some() {
        env.set(keys::API_TIMEOUT, &defaults.knobs.api_timeout().to_string());
        if defaults.knobs.disable_nonessential_traffic_on() {
            env.set(keys::NONESSENTIAL_TRAFFIC, "1");
        }
    }
    if defaults.knobs.agent_teams_on() {
        env.set(keys::AGENT_TEAMS, "1");
    }

    // 认证：spec.auth.mode 决定 secret 落到哪个键
    let mut api_key_written: Option<&Secret> = None;
    match spec.auth.mode {
        AuthMode::ApiKey => {
            if let Some(s) = secret {
                env.set(keys::API_KEY, s.reveal());
                api_key_written = Some(s);
            }
        }
        AuthMode::AuthToken => {
            if let Some(s) = secret {
                env.set(keys::AUTH_TOKEN, s.reveal());
            }
            // 写空 API_KEY 屏蔽 shell 继承的游离 key——它会优先于 AUTH_TOKEN 导致认证错乱
            env.set(keys::API_KEY, "");
        }
        AuthMode::None => {}
    }
    if preset.flags.auth_token_also_sets_api_key
        && let Some(s) = secret
    {
        env.set(keys::API_KEY, s.reveal());
        api_key_written = Some(s);
    }

    // 模型/套餐与各自所属层一起落下，避免高层换了 sonnet、低层窗口配置仍残留。
    let spec_sonnet = spec.intent.models.get(ModelSlot::Sonnet);
    let defaults_sonnet = defaults.env.get(ModelSlot::Sonnet.env_key());
    write_slots(&mut env, &preset.models);
    for (k, v) in &preset.env {
        env.set(k, v);
    }
    if spec_sonnet.is_none() && defaults_sonnet.is_none() {
        write_package(&mut env, preset, preset.models.get(ModelSlot::Sonnet));
    }

    // 全局旋钮·第二遍：只有用户显式拨过的 env 旋钮压过 preset。
    if let Some(ms) = defaults.knobs.api_timeout_ms
        && base_url.is_some()
    {
        env.set(keys::API_TIMEOUT, &ms.to_string());
    }
    if base_url.is_some() {
        match defaults.knobs.disable_nonessential_traffic {
            Some(true) => env.set(keys::NONESSENTIAL_TRAFFIC, "1"),
            Some(false) => env.remove(keys::NONESSENTIAL_TRAFFIC),
            None => {}
        }
    }
    match defaults.knobs.agent_teams {
        Some(true) => env.set(keys::AGENT_TEAMS, "1"),
        Some(false) => env.remove(keys::AGENT_TEAMS),
        None => {}
    }
    match defaults.knobs.tool_search {
        Some(true) => env.set(keys::TOOL_SEARCH, "true"),
        Some(false) => env.set(keys::TOOL_SEARCH, "false"),
        None => {}
    }
    for knob in BOOL_KNOBS {
        if let KnobTarget::Env(key) = knob.target {
            if defaults.knobs.flag(knob) != knob.claude_default {
                env.set(key, "1");
            } else if defaults.knobs.is_explicit(knob.field) {
                env.remove(key);
            }
        }
    }
    for knob in VALUE_KNOBS {
        if let KnobTarget::Env(key) = knob.target
            && let Some(value) = defaults.knobs.value(knob)
        {
            env.set(key, &Knobs::scalar_text(value));
        }
    }
    if spec_sonnet.is_none() {
        write_package(&mut env, preset, defaults_sonnet.map(String::as_str));
    }
    for (k, v) in &defaults.env {
        env.set(k, v);
    }
    write_slots(&mut env, &spec.intent.models);
    write_package(&mut env, preset, spec_sonnet);
    // 用户给生效 sonnet 定义过窗口：压过 preset 套餐的数字，按同一套规则展开成配套 env
    if let Some(sonnet) = effective_models(spec, preset, defaults).get(ModelSlot::Sonnet)
        && let Some(max_context) = spec.intent.model_windows.get(sonnet)
    {
        let custom = crate::preset::ModelChoice {
            id: sonnet.to_string(),
            max_context: Some(*max_context),
            ..Default::default()
        };
        for (key, value) in custom.package_env() {
            env.set(&key, &value);
        }
    }
    for (k, v) in &spec.intent.env {
        env.set(k, v);
    }

    // shared 模式 Claude 读的是 ~/.claude 的 settings.json，这一层改走 [`shared_overlay`]
    let (settings_top, permissions_default_mode) = match spec.isolation {
        Isolation::Full => settings_layer(defaults),
        Isolation::Shared => (Vec::new(), None),
    };

    // donn 常量最后写：即使覆盖里出现同名键也以 donn 为准
    env.set(keys::AUTOUPDATER, "1");
    // 共享模式：显式停用 claude.ai connectors（第三方 env 认证下本就不可用；
    // 显式停用走 Claude Code 的静默分支，避免"auth source takes precedence"常驻横幅）
    if spec.isolation == Isolation::Shared {
        env.set(keys::CLAUDEAI_MCP, "0");
    }

    // .claude.json：onboarding 恒写；确认屏白名单仅当写了非空 api_key
    let mut claude_json = vec![keys::ONBOARDING.to_string()];
    let approved_key_suffix = api_key_written.map(|s| {
        claude_json.push(keys::API_RESPONSES.to_string());
        s.suffix20()
    });

    Rendered {
        env: env.into_pairs(),
        settings_top,
        permissions_default_mode,
        claude_json,
        approved_key_suffix,
    }
}

/// settings.json 顶层这一层：settings 类旋钮 < `[defaults.settings]`（同名键覆盖），
/// 外加权限模式。只取决于全局配置，与 profile 无关。
fn settings_layer(defaults: &Defaults) -> (Vec<(String, serde_json::Value)>, Option<String>) {
    let mut top: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    if defaults.knobs.hide_attribution_on() {
        top.insert(keys::ATTRIBUTION.into(), keys::attribution_hidden());
    }
    // default 档和非法存量值均不管理；bypass 档额外写确认屏标记。
    let mode = defaults.knobs.permission_mode();
    let mode =
        (mode != "default" && keys::PERMISSION_MODES.contains(&mode)).then(|| mode.to_string());
    if mode.as_deref() == Some(keys::BYPASS_PERMISSIONS) {
        top.insert(keys::SKIP_DANGEROUS_PROMPT.into(), serde_json::json!(true));
    }
    for knob in BOOL_KNOBS {
        if let KnobTarget::Setting(key) = knob.target
            && defaults.knobs.flag(knob) != knob.claude_default
        {
            top.insert(key.into(), serde_json::json!(defaults.knobs.flag(knob)));
        }
    }
    for knob in VALUE_KNOBS {
        if let KnobTarget::Setting(key) = knob.target
            && let Some(value) = defaults.knobs.value(knob)
        {
            top.insert(key.into(), value.clone());
        }
    }
    for (k, v) in &defaults.settings {
        top.insert(k.clone(), v.clone());
    }
    top.retain(|k, _| !keys::RESERVED_TOP_KEYS.contains(&k.as_str()));
    (top.into_iter().collect(), mode)
}

/// shared 模式的 `--settings` overlay：[`settings_layer`] 的全部内容，加上 `model`——
/// `~/.claude` 里持久化的 /model 选择会盖过槽位映射，用它压回 profile 的 sonnet。
pub fn shared_overlay(
    spec: &ProfileSpec,
    preset: &Preset,
    defaults: &Defaults,
) -> serde_json::Map<String, serde_json::Value> {
    let (top, mode) = settings_layer(defaults);
    let mut overlay: serde_json::Map<_, _> = top.into_iter().collect();
    if let Some(mode) = mode {
        overlay.insert(
            "permissions".into(),
            serde_json::json!({keys::PERMISSIONS_DEFAULT_MODE: mode}),
        );
    }
    if !overlay.contains_key("model")
        && let Some(sonnet) = effective_models(spec, preset, defaults).get(ModelSlot::Sonnet)
    {
        overlay.insert("model".into(), sonnet.into());
    }
    overlay
}

fn write_slots(env: &mut EnvBuilder, models: &SlotMap) {
    for slot in ModelSlot::ALL {
        if let Some(model) = models.get(slot) {
            env.set(slot.env_key(), model);
        }
    }
}

/// 启动时要从继承的 shell 环境里剥掉的键：donn 会写的全部 env 键，加上
/// [`keys::SHELL_OVERRIDE_KEYS`]。不剥的话，shell 里 export 过的同名变量会让「旋钮关掉」失效。
pub fn managed_env_keys() -> Vec<&'static str> {
    let mut out: std::collections::BTreeSet<&'static str> = [
        keys::BASE_URL,
        keys::API_KEY,
        keys::AUTH_TOKEN,
        keys::AUTOUPDATER,
        keys::API_TIMEOUT,
        keys::NONESSENTIAL_TRAFFIC,
        keys::AGENT_TEAMS,
        keys::TOOL_SEARCH,
        keys::CLAUDEAI_MCP,
        keys::EFFORT,
        keys::AUTO_COMPACT,
        keys::MAX_CONTEXT,
        keys::SUBAGENT_MODEL,
        "CLAUDE_CONFIG_DIR",
    ]
    .into_iter()
    .chain(ModelSlot::ALL.iter().map(|slot| slot.env_key()))
    .chain(keys::SHELL_OVERRIDE_KEYS)
    .collect();
    for knob in BOOL_KNOBS {
        if let KnobTarget::Env(key) = knob.target {
            out.insert(key);
        }
    }
    for knob in VALUE_KNOBS {
        if let KnobTarget::Env(key) = knob.target {
            out.insert(key);
        }
    }
    out.into_iter().collect()
}

fn write_package(env: &mut EnvBuilder, preset: &Preset, sonnet_id: Option<&str>) {
    if let Some(choice) = sonnet_id.and_then(|id| preset.choice_for_model(id)) {
        for (key, value) in choice.package_env() {
            env.set(&key, &value);
        }
    }
}

/// 同键后写覆盖的 env 构建器；输出按键名排序。
#[derive(Default)]
struct EnvBuilder(std::collections::BTreeMap<String, String>);

impl EnvBuilder {
    fn set(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_string(), value.to_string());
    }

    /// 移除（用户显式关掉开关时压过 preset 声明）。
    fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn into_pairs(self) -> Vec<(String, String)> {
        self.0.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SlotMap;
    use crate::preset::PresetFlags;
    use crate::spec::{AuthSpec, Intent};
    use std::collections::BTreeMap;

    fn preset(auth_mode: AuthMode) -> Preset {
        Preset {
            key: "t".into(),
            label: "T".into(),
            base_url: Some("https://preset.example/api".into()),
            auth_mode,
            models: SlotMap {
                sonnet: Some("m-sonnet".into()),
                haiku: Some("m-haiku".into()),
                ..Default::default()
            },
            env: BTreeMap::from([("API_TIMEOUT_MS".into(), "600000".into())]),
            ..Default::default()
        }
    }

    fn spec(mode: AuthMode) -> ProfileSpec {
        ProfileSpec {
            name: "t".into(),
            preset: "t".into(),
            auth: AuthSpec { mode },
            ..Default::default()
        }
    }

    fn env_map(r: &Rendered) -> BTreeMap<String, String> {
        r.env.iter().cloned().collect()
    }

    fn secret() -> Secret {
        Secret::new("sk-ant-0123456789abcdefghijklmn").unwrap()
    }

    fn dft() -> Defaults {
        Defaults::default()
    }

    #[test]
    fn auth_token_mode_full_path() {
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            Some(&secret()),
            &dft(),
        );
        let env = env_map(&r);
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://preset.example/api");
        assert_eq!(
            env["ANTHROPIC_AUTH_TOKEN"],
            "sk-ant-0123456789abcdefghijklmn"
        );
        assert_eq!(
            env["ANTHROPIC_API_KEY"], "",
            "空 API_KEY 屏蔽 shell 游离 key"
        );
        assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "m-sonnet");
        assert!(
            !env.contains_key("ANTHROPIC_DEFAULT_OPUS_MODEL"),
            "无值槽位不写"
        );
        assert_eq!(env["DISABLE_AUTOUPDATER"], "1");
        assert_eq!(r.approved_key_suffix, None, "auth_token 模式没有确认屏");
        assert_eq!(r.claude_json, vec!["hasCompletedOnboarding"]);
    }

    #[test]
    fn api_key_mode_writes_approved_suffix() {
        let r = render(
            &spec(AuthMode::ApiKey),
            &preset(AuthMode::ApiKey),
            Some(&secret()),
            &dft(),
        );
        let env = env_map(&r);
        assert_eq!(env["ANTHROPIC_API_KEY"], "sk-ant-0123456789abcdefghijklmn");
        assert!(!env.contains_key("ANTHROPIC_AUTH_TOKEN"));
        assert_eq!(
            r.approved_key_suffix.as_deref(),
            Some("456789abcdefghijklmn")
        );
        assert_eq!(
            r.claude_json,
            vec!["hasCompletedOnboarding", "customApiKeyResponses"]
        );
    }

    #[test]
    fn none_mode_writes_no_auth() {
        let mut p = preset(AuthMode::None);
        p.base_url = None;
        p.models = SlotMap::default();
        p.env.clear();
        let r = render(&spec(AuthMode::None), &p, None, &dft());
        let env = env_map(&r);
        assert_eq!(env.len(), 2, "no auth keys, only donn defaults: {env:?}");
        assert_eq!(env["DISABLE_AUTOUPDATER"], "1");
        assert_eq!(env["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1");
        assert!(
            !env.contains_key("ENABLE_TOOL_SEARCH"),
            "默认不写键：由上游按端点主机判定（true 会强发 beta 头）"
        );
    }

    #[test]
    fn flags_table() {
        // auth_token_also_sets_api_key（ollama）→ 两个都写，且触发确认屏白名单
        let mut p = preset(AuthMode::AuthToken);
        p.flags = PresetFlags {
            auth_token_also_sets_api_key: true,
        };
        let r = render(&spec(AuthMode::AuthToken), &p, Some(&secret()), &dft());
        let env = env_map(&r);
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], env["ANTHROPIC_API_KEY"]);
        assert!(r.approved_key_suffix.is_some());

        // auth_token 默认置空 API_KEY 且不写确认屏
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            Some(&secret()),
            &dft(),
        );
        let env = env_map(&r);
        assert_eq!(env["ANTHROPIC_API_KEY"], "");
        assert_eq!(r.approved_key_suffix, None);
    }

    #[test]
    fn precedence_spec_over_preset_constants_last() {
        let mut s = spec(AuthMode::AuthToken);
        s.intent = Intent {
            base_url: Some("https://user.example".into()),
            models: SlotMap {
                sonnet: Some("user-sonnet".into()),
                ..Default::default()
            },
            model_windows: BTreeMap::new(),
            env: BTreeMap::from([
                ("API_TIMEOUT_MS".into(), "9".into()),
                ("MY_VAR".into(), "1".into()),
                ("DISABLE_AUTOUPDATER".into(), "0".into()), // 常量恒写覆盖
            ]),
        };
        let r = render(&s, &preset(AuthMode::AuthToken), Some(&secret()), &dft());
        let env = env_map(&r);
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://user.example");
        assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "user-sonnet");
        assert_eq!(
            env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "m-haiku",
            "未覆盖槽位用 preset"
        );
        assert_eq!(env["API_TIMEOUT_MS"], "9", "spec 覆盖 preset.env");
        assert_eq!(env["MY_VAR"], "1");
        assert_eq!(env["DISABLE_AUTOUPDATER"], "1", "donn 常量最后生效");
    }

    #[test]
    fn third_party_defaults_follow_base_url() {
        // 有端点 → 注入默认层
        let mut p = preset(AuthMode::AuthToken);
        p.env.clear();
        let r = render(&spec(AuthMode::AuthToken), &p, None, &dft());
        let env = env_map(&r);
        assert_eq!(env["API_TIMEOUT_MS"], "600000");
        assert!(!env.contains_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));

        // preset.env 覆盖默认层
        let mut p = preset(AuthMode::AuthToken);
        p.env = BTreeMap::from([("API_TIMEOUT_MS".into(), "300000".into())]);
        let r = render(&spec(AuthMode::AuthToken), &p, None, &dft());
        assert_eq!(env_map(&r)["API_TIMEOUT_MS"], "300000");

        // 无端点（official 直连）→ 不注入
        let mut p = preset(AuthMode::None);
        p.base_url = None;
        p.env.clear();
        let r = render(&spec(AuthMode::None), &p, None, &dft());
        let env = env_map(&r);
        assert!(!env.contains_key("API_TIMEOUT_MS"));
        assert!(!env.contains_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));

        // spec 填了 base_url（custom 渠道）→ 注入
        let mut s = spec(AuthMode::AuthToken);
        s.intent.base_url = Some("https://relay.example".into());
        let mut p = preset(AuthMode::AuthToken);
        p.base_url = None;
        p.env.clear();
        let r = render(&s, &p, None, &dft());
        assert_eq!(env_map(&r)["API_TIMEOUT_MS"], "600000");
    }

    #[test]
    fn user_defaults_between_preset_and_spec() {
        let mut d = Defaults::default();
        d.env.insert("API_TIMEOUT_MS".into(), "300000".into()); // 覆盖 preset 的 600000
        d.env.insert("MY_GLOBAL".into(), "g".into());
        d.settings
            .insert("spinnerTipsEnabled".into(), serde_json::Value::Bool(false));
        d.settings
            .insert("env".into(), serde_json::json!({"HACK": "1"})); // 保留键，忽略
        d.settings
            .insert("permissions".into(), serde_json::json!({})); // 保留键，忽略

        // defaults 覆盖 preset
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &d,
        );
        let env = env_map(&r);
        assert_eq!(env["API_TIMEOUT_MS"], "300000");
        assert_eq!(env["MY_GLOBAL"], "g");
        // 用户新增 settings 字段写入（+ 权限直通默认标记）；保留键 env/permissions 被过滤
        assert_eq!(
            r.settings_top,
            vec![
                (
                    "skipDangerousModePermissionPrompt".to_string(),
                    serde_json::json!(true)
                ),
                (
                    "spinnerTipsEnabled".to_string(),
                    serde_json::Value::Bool(false)
                ),
            ]
        );
        assert_eq!(
            r.footprint().settings_top,
            vec![
                "skipDangerousModePermissionPrompt".to_string(),
                "spinnerTipsEnabled".to_string()
            ]
        );

        // 用户自由 settings 可写 attribution
        let mut d2 = Defaults::default();
        d2.settings
            .insert("attribution".into(), serde_json::json!({"commit": "x"}));
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &d2,
        );
        assert_eq!(
            r.settings_top,
            vec![
                (
                    "attribution".to_string(),
                    serde_json::json!({"commit": "x"})
                ),
                (
                    "skipDangerousModePermissionPrompt".to_string(),
                    serde_json::json!(true)
                ),
            ]
        );

        // spec 覆盖 defaults
        let mut s = spec(AuthMode::AuthToken);
        s.intent.env = BTreeMap::from([("MY_GLOBAL".into(), "per-profile".into())]);
        let r = render(&s, &preset(AuthMode::AuthToken), None, &d);
        assert_eq!(env_map(&r)["MY_GLOBAL"], "per-profile");
    }

    #[test]
    fn knobs_defaults_and_overrides() {
        // 默认：第三方超时、agent teams 注入；非必要流量/tool search 默认不写。
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &dft(),
        );
        let env = env_map(&r);
        assert_eq!(env["API_TIMEOUT_MS"], "600000");
        assert!(!env.contains_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
        assert_eq!(env["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1");
        assert!(
            !env.contains_key("ENABLE_TOOL_SEARCH"),
            "默认不写键：由上游按端点主机判定"
        );
        assert_eq!(
            r.settings_top,
            vec![(
                "skipDangerousModePermissionPrompt".to_string(),
                serde_json::json!(true)
            )],
            "权限直通默认开：跳过危险模式确认屏"
        );
        assert_eq!(
            r.permissions_default_mode.as_deref(),
            Some("bypassPermissions")
        );
        assert_eq!(r.footprint().permissions_top, vec!["defaultMode"]);

        // 用户拨动旋钮：team/tool search 关、权限交还 default、隐藏署名、
        // effort=medium 上限 high、connectors/thinking/非必要流量禁用、超时改。
        let mut knobs = Knobs {
            agent_teams: Some(false),
            tool_search: Some(false),
            permission_mode: Some("default".into()),
            hide_attribution: Some(true),
            api_timeout_ms: Some(300_000),
            disable_nonessential_traffic: Some(true),
            extra: Default::default(),
        };
        knobs.set_extra("thinking", Some(serde_json::json!(false)));
        knobs.set_extra("effort", Some(serde_json::json!("medium")));
        knobs.set_extra("max_effort", Some(serde_json::json!("high")));
        knobs.set_extra("disable_connectors", Some(serde_json::json!(true)));
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &d,
        );
        let env = env_map(&r);
        assert!(!env.contains_key("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"));
        assert_eq!(env["ENABLE_TOOL_SEARCH"], "false");
        assert_eq!(env["API_TIMEOUT_MS"], "300000");
        assert_eq!(env["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
        assert_eq!(r.permissions_default_mode, None, "权限直通关闭后不管理");
        assert!(r.footprint().permissions_top.is_empty());
        assert_eq!(
            r.settings_top,
            vec![
                (
                    "alwaysThinkingEnabled".to_string(),
                    serde_json::json!(false)
                ),
                (
                    "attribution".to_string(),
                    serde_json::json!({"commit": "", "pr": "", "sessionUrl": false})
                ),
                (
                    "disableClaudeAiConnectors".to_string(),
                    serde_json::json!(true)
                ),
                ("effortLevel".to_string(), serde_json::json!("medium")),
                ("maxEffortLevel".to_string(), serde_json::json!("high")),
            ],
            "settings 类旋钮全部写入"
        );

        // official（无端点）：超时/遥测不注入
        let mut p = preset(AuthMode::None);
        p.base_url = None;
        p.env.clear();
        let r = render(&spec(AuthMode::None), &p, None, &dft());
        let env = env_map(&r);
        assert!(!env.contains_key("API_TIMEOUT_MS"));
        assert!(!env.contains_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
        assert_eq!(r.settings_top.len(), 1, "仅权限直通标记");

        let d = Defaults {
            knobs: Knobs {
                api_timeout_ms: Some(300_000),
                disable_nonessential_traffic: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let r = render(&spec(AuthMode::None), &p, None, &d);
        let env = env_map(&r);
        assert!(!env.contains_key("API_TIMEOUT_MS"));
        assert!(!env.contains_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
    }

    #[test]
    fn tool_search_explicit_values_still_written() {
        // 显式 true 恒写（发 beta 头，代理需支持 tool_reference）；显式 false 写 "false" 压 preset
        let mut knobs = Knobs {
            tool_search: Some(true),
            ..Default::default()
        };
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        assert_eq!(
            env_map(&render(
                &spec(AuthMode::AuthToken),
                &preset(AuthMode::AuthToken),
                None,
                &d,
            ))["ENABLE_TOOL_SEARCH"],
            "true"
        );

        knobs = Knobs {
            tool_search: Some(false),
            ..Default::default()
        };
        let mut p = preset(AuthMode::AuthToken);
        p.env.insert("ENABLE_TOOL_SEARCH".into(), "true".into());
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        assert_eq!(
            env_map(&render(&spec(AuthMode::AuthToken), &p, None, &d))["ENABLE_TOOL_SEARCH"],
            "false",
            "显式 false 压过 preset 声明的同名键（官方主机 unset 即 defer，删键关不掉）"
        );
    }

    #[test]
    fn simple_knobs_write_only_non_default_values() {
        let by_field = |field: &str| BOOL_KNOBS.iter().find(|k| k.field == field).unwrap();
        let mut knobs = Knobs::default();
        knobs.set_extra("disable_experimental_betas", Some(serde_json::json!(true)));
        knobs.set_extra("model_fallback", Some(serde_json::json!(false)));
        knobs.set_extra("auto_compact", Some(serde_json::json!(false)));
        knobs.set_extra("disable_connectors", Some(serde_json::json!(true)));
        knobs.set_extra("thinking", Some(serde_json::json!(true))); // 显式 = 默认，不写
        knobs.set_extra("language", Some(serde_json::json!("中文")));
        knobs.set_extra("max_output_tokens", Some(serde_json::json!(64000)));
        knobs.set_extra("max_effort", Some(serde_json::json!("high")));
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &d,
        );
        let env = env_map(&r);
        assert_eq!(env["CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS"], "1");
        assert_eq!(env["CLAUDE_CODE_NO_MODEL_FALLBACK"], "1");
        assert_eq!(env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"], "64000");
        let top: BTreeMap<String, serde_json::Value> = r.settings_top.iter().cloned().collect();
        assert_eq!(top["autoCompactEnabled"], serde_json::json!(false));
        assert_eq!(top["disableClaudeAiConnectors"], serde_json::json!(true));
        assert!(!top.contains_key("alwaysThinkingEnabled"));
        assert_eq!(top["language"], serde_json::json!("中文"));
        assert_eq!(top["maxEffortLevel"], serde_json::json!("high"));
        assert!(by_field("thinking").claude_default);

        // 显式设成默认值：压掉 preset.env 里的同名键
        let mut p = preset(AuthMode::AuthToken);
        p.env
            .insert("CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS".into(), "1".into());
        let mut knobs = Knobs::default();
        knobs.set_extra("disable_experimental_betas", Some(serde_json::json!(false)));
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        let r = render(&spec(AuthMode::AuthToken), &p, None, &d);
        assert!(!env_map(&r).contains_key("CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS"));
    }

    #[test]
    fn effective_models_match_rendered_slots_including_global_env() {
        let mut d = Defaults::default();
        d.env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".into(),
            "global-sonnet".into(),
        );
        let mut s = spec(AuthMode::AuthToken);
        s.intent
            .models
            .set(ModelSlot::Haiku, Some("my-haiku".into()));
        let p = preset(AuthMode::AuthToken);
        let effective = effective_models(&s, &p, &d);
        assert_eq!(effective.get(ModelSlot::Sonnet), Some("global-sonnet"));
        assert_eq!(effective.get(ModelSlot::Haiku), Some("my-haiku"));
        let env = env_map(&render(&s, &p, None, &d));
        for slot in ModelSlot::ALL {
            assert_eq!(
                env.get(slot.env_key()).map(String::as_str),
                effective.get(slot),
                "{slot:?}"
            );
        }
    }

    #[test]
    fn shared_isolation_writes_no_settings_class_keys() {
        let mut s = spec(AuthMode::AuthToken);
        s.isolation = Isolation::Shared;
        let mut knobs = Knobs::default();
        knobs.set_extra("language", Some(serde_json::json!("中文")));
        let d = Defaults {
            knobs,
            ..Default::default()
        };
        let r = render(&s, &preset(AuthMode::AuthToken), None, &d);
        assert!(r.settings_top.is_empty());
        assert_eq!(r.permissions_default_mode, None);
        assert!(r.footprint().settings_top.is_empty());
        assert_eq!(env_map(&r)["ENABLE_CLAUDEAI_MCP_SERVERS"], "0");
    }

    #[test]
    fn footprint_lists_written_keys() {
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            Some(&secret()),
            &dft(),
        );
        let fp = r.footprint();
        assert_eq!(
            fp.settings_env,
            r.env.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()
        );
        assert_eq!(fp.claude_json, vec!["hasCompletedOnboarding"]);
    }

    #[test]
    fn package_env_from_effective_sonnet() {
        let mut p = preset(AuthMode::AuthToken);
        p.models.sonnet = Some("k3[1m]".into());
        p.model_choices = vec![
            crate::preset::ModelChoice {
                id: "k3[1m]".into(),
                max_context: Some(1_048_576),
                ..Default::default()
            },
            crate::preset::ModelChoice {
                id: "k3".into(),
                max_context: Some(262_144),
                ..Default::default()
            },
        ];
        // create / Follow: package from preset default
        let env = env_map(&render(&spec(AuthMode::AuthToken), &p, None, &dft()));
        assert_eq!(env[keys::MAX_CONTEXT], "1048576");
        assert_eq!(env[keys::SUBAGENT_MODEL], "k3[1m]");

        // sonnet override → different package
        let mut s = spec(AuthMode::AuthToken);
        s.intent.models.sonnet = Some("k3".into());
        let env = env_map(&render(&s, &p, None, &dft()));
        assert_eq!(env[keys::MAX_CONTEXT], "262144");
        assert_eq!(env[keys::SUBAGENT_MODEL], "k3");

        // intent still wins over package
        s.intent.env = BTreeMap::from([(keys::MAX_CONTEXT.into(), "999".into())]);
        assert_eq!(
            env_map(&render(&s, &p, None, &dft()))[keys::MAX_CONTEXT],
            "999"
        );

        // freeform id with no choice → no package keys
        s.intent.models.sonnet = Some("unknown-model".into());
        s.intent.env.clear();
        let env = env_map(&render(&s, &p, None, &dft()));
        assert!(!env.contains_key(keys::MAX_CONTEXT));
    }

    #[test]
    fn no_secret_writes_no_auth_key() {
        let r = render(
            &spec(AuthMode::AuthToken),
            &preset(AuthMode::AuthToken),
            None,
            &dft(),
        );
        assert!(!env_map(&r).contains_key("ANTHROPIC_AUTH_TOKEN"));
    }

    #[test]
    fn custom_model_window_expands_like_a_preset_package_and_user_env_still_wins() {
        let mut s = spec(AuthMode::AuthToken);
        s.intent
            .models
            .set(ModelSlot::Sonnet, Some("my-gateway/model-x".into()));
        s.intent
            .model_windows
            .insert("my-gateway/model-x".into(), 2_000_000);
        // 别的模型的条目不影响当前 sonnet
        s.intent.model_windows.insert("other/model".into(), 1);
        let env = env_map(&render(&s, &preset(AuthMode::AuthToken), None, &dft()));
        assert_eq!(env["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "2000000");
        assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "1000000", "封顶");
        assert_eq!(env["CLAUDE_CODE_SUBAGENT_MODEL"], "my-gateway/model-x");

        s.intent
            .env
            .insert("CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(), "5".into());
        let env = env_map(&render(&s, &preset(AuthMode::AuthToken), None, &dft()));
        assert_eq!(env["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "5");
    }

    #[test]
    fn managed_env_keys_are_unique_and_cover_slots_and_knobs() {
        let managed = managed_env_keys();
        let unique: std::collections::HashSet<_> = managed.iter().copied().collect();
        assert_eq!(managed.len(), unique.len(), "duplicate managed key");
        for slot in ModelSlot::ALL {
            assert!(unique.contains(slot.env_key()), "{slot:?}");
        }
        for const_key in [
            keys::BASE_URL,
            keys::API_KEY,
            keys::AUTH_TOKEN,
            keys::EFFORT,
            keys::AUTO_COMPACT,
            keys::MAX_CONTEXT,
            keys::SUBAGENT_MODEL,
            "CLAUDE_CONFIG_DIR",
        ] {
            assert!(unique.contains(const_key), "{const_key}");
        }
        // 旋钮表里的 Env 目标全部在册
        for knob in BOOL_KNOBS {
            if let KnobTarget::Env(key) = knob.target {
                assert!(unique.contains(key), "{}", knob.field);
            }
        }
        for knob in VALUE_KNOBS {
            if let KnobTarget::Env(key) = knob.target {
                assert!(unique.contains(key), "{}", knob.field);
            }
        }
    }
}
