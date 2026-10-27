//! render：把 `(spec, preset, secret, defaults)` 变成 `Rendered` 的纯函数。
//!
//! 生成 settings.json / `.claude.json` 中归 donn 管理的那部分内容。后写的层覆盖先写的层，
//! 层序就是 `render_with_sources` 函数体的顺序，文字说明在 docs/ARCHITECTURE.md。
//! provider 的兼容性差异用 preset 的 flags 表达，这里不特判任何 provider。

use std::collections::BTreeMap;

use crate::config::Defaults;
use crate::keys::{self, ModelSlot, SlotMap};
use crate::knobs::{BOOL_KNOBS, KnobTarget, Knobs, TypedKnob, VALUE_KNOBS};
use crate::preset::{AuthMode, ModelChoice, Preset};
use crate::secret::Secret;
use crate::spec::{Footprint, Isolation, ProfileSpec};

/// 渲染结果：donn 这次要写的内容。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rendered {
    /// settings.json 顶层 `env` 对象内的 key-value（有序）。
    pub env: Vec<(String, String)>,
    /// settings.json 顶层字段：settings 类 knob 和 `[defaults.settings]`。shared 模式为空。
    pub settings_top: Vec<(String, serde_json::Value)>,
    /// `permissions.defaultMode` 的值（permission mode knob；None = 不管理这个子 key）。
    pub permissions_default_mode: Option<String>,
    /// `.claude.json` 顶层由 donn 管理的 key 名。
    pub claude_json: Vec<String>,
    /// 写入 api_key 时，确认屏白名单需要的 key 后 20 位。
    pub approved_key_suffix: Option<String>,
}

impl Rendered {
    /// 本次实际写入的全部 key 名，用来生成 spec 的新 footprint。
    pub fn footprint(&self) -> Footprint {
        Footprint {
            settings_env: self.env.iter().map(|(k, _)| k.clone()).collect(),
            settings_top: self.settings_top.iter().map(|(k, _)| k.clone()).collect(),
            claude_json: self.claude_json.clone(),
            permissions_top: self
                .permissions_default_mode
                .iter()
                .map(|_| keys::PERMISSIONS_DEFAULT_MODE.to_string())
                .collect(),
        }
    }
}

/// 生效 endpoint：spec 覆盖 > preset。展示层和 render 共用这个函数，优先级只在这里定义一次。
pub fn effective_base_url(spec: &ProfileSpec, preset: &Preset) -> Option<String> {
    spec.intent
        .base_url
        .clone()
        .filter(|u| !u.is_empty())
        .or_else(|| preset.base_url.clone())
}

/// `[defaults.env]` 里写的 model slot key（全局默认层）。
pub fn global_models(defaults: &Defaults) -> SlotMap {
    let mut out = SlotMap::default();
    for slot in ModelSlot::ALL {
        out.set(slot, defaults.env.get(slot.env_key()).cloned());
    }
    out
}

/// 生效 model slot：spec 覆盖 > `[defaults.env]` > preset。结果与 [`render`] 写入的 slot 一致，
/// 展示层只能调用这个函数。
pub fn effective_models(spec: &ProfileSpec, preset: &Preset, defaults: &Defaults) -> SlotMap {
    spec.intent
        .models
        .over(&global_models(defaults).over(&preset.models))
}

/// 生效值来自哪一层，供 UI 标注 `(global)` / `(default)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    Profile,
    Global,
    Default,
}

/// donn 配置的思考强度：最终渲染的 env 优先，其次是 settings 里的 `effortLevel`。
/// 模型支持的档位、settings 的适用范围和会话内的限制由 Claude Code 解析。
pub fn effective_effort(
    spec: &ProfileSpec,
    preset: &Preset,
    defaults: &Defaults,
) -> Option<(String, ValueSource)> {
    if let Some(value) = effective_env(spec, preset, defaults, keys::EFFORT) {
        return Some(value);
    }
    let setting = match spec.isolation {
        Isolation::Full => settings_layer(defaults)
            .0
            .into_iter()
            .find(|(key, _)| key == keys::EFFORT_SETTING)
            .and_then(|(_, value)| value.as_str().map(str::to_string)),
        Isolation::Shared => shared_overlay(spec, preset, defaults)
            .get(keys::EFFORT_SETTING)
            .and_then(|value| value.as_str().map(str::to_string)),
    };
    setting.map(|level| (level, ValueSource::Global))
}

/// donn 配置的上下文窗口：值和来源都取最终渲染的 env。变量是否适用于当前模型由 Claude Code 解析。
pub fn effective_max_context(
    spec: &ProfileSpec,
    preset: &Preset,
    defaults: &Defaults,
) -> Option<(u64, ValueSource)> {
    let (raw, source) = effective_env(spec, preset, defaults, keys::MAX_CONTEXT)?;
    Some((raw.parse().ok()?, source))
}

fn effective_env(
    spec: &ProfileSpec,
    preset: &Preset,
    defaults: &Defaults,
    key: &str,
) -> Option<(String, ValueSource)> {
    let (rendered, sources) = render_with_sources(spec, preset, None, defaults);
    let (_, value) = rendered.env.into_iter().find(|(k, _)| k == key)?;
    Some((value, *sources.get(key)?))
}

/// 按优先级渲染。所有 env 值都是字符串（Claude Code 接受数字型 env 的字符串形式）。
pub fn render(
    spec: &ProfileSpec,
    preset: &Preset,
    secret: Option<&Secret>,
    defaults: &Defaults,
) -> Rendered {
    render_with_sources(spec, preset, secret, defaults).0
}

fn render_with_sources(
    spec: &ProfileSpec,
    preset: &Preset,
    secret: Option<&Secret>,
    defaults: &Defaults,
) -> (Rendered, BTreeMap<String, ValueSource>) {
    let mut env = EnvBuilder::default();

    // endpoint：spec 覆盖 preset
    let base_url = effective_base_url(spec, preset);
    if let Some(url) = &base_url {
        let source = if spec.intent.base_url.as_ref().is_some_and(|u| !u.is_empty()) {
            ValueSource::Profile
        } else {
            ValueSource::Default
        };
        env.set(keys::BASE_URL, url, source);
    }

    // 全局 knob 第一遍：生效值取用户值或默认值，这是最低层，preset 可以覆盖。
    // tool search 不在这遍处理：用户没拨动时不写 key，由 Claude Code 按 endpoint 主机判断（见 [`keys::TOOL_SEARCH`]）。
    if base_url.is_some() {
        let source = if defaults.knobs.api_timeout_ms.is_some() {
            ValueSource::Global
        } else {
            ValueSource::Default
        };
        env.set(
            keys::API_TIMEOUT,
            &defaults.knobs.api_timeout().to_string(),
            source,
        );
    }
    for knob in TypedKnob::ALL {
        let Some(key) = knob.default_env_key() else {
            continue;
        };
        let applies = base_url.is_some() || !knob.requires_endpoint();
        if applies && knob.bool_value(&defaults.knobs) == Some(true) {
            let source = if knob.bool_override(&defaults.knobs).is_some() {
                ValueSource::Global
            } else {
                ValueSource::Default
            };
            env.set(key, "1", source);
        }
    }

    // 认证：spec.auth.mode 决定 secret 写进哪个 key
    let mut api_key_written: Option<&Secret> = None;
    match spec.auth.mode {
        AuthMode::ApiKey => {
            if let Some(s) = secret {
                env.set(keys::API_KEY, s.reveal(), ValueSource::Profile);
                api_key_written = Some(s);
            }
        }
        AuthMode::AuthToken => {
            if let Some(s) = secret {
                env.set(keys::AUTH_TOKEN, s.reveal(), ValueSource::Profile);
            }
            // 写空的 API_KEY 屏蔽 shell 继承的游离 key；它会优先于 AUTH_TOKEN，导致认证错乱
            env.set(keys::API_KEY, "", ValueSource::Default);
        }
        AuthMode::None => {}
    }
    if preset.flags.auth_token_also_sets_api_key
        && let Some(s) = secret
    {
        env.set(keys::API_KEY, s.reveal(), ValueSource::Profile);
        api_key_written = Some(s);
    }

    // model 和 package 跟着所属层一起写入，避免高层换了 sonnet 后低层窗口配置还残留。
    let spec_sonnet = spec.intent.models.get(ModelSlot::Sonnet);
    let defaults_sonnet = defaults.env.get(ModelSlot::Sonnet.env_key());
    write_slots(&mut env, &preset.models, ValueSource::Default);
    for (k, v) in &preset.env {
        env.set(k, v, ValueSource::Default);
    }
    if spec_sonnet.is_none() && defaults_sonnet.is_none() {
        write_package(&mut env, preset, preset.models.get(ModelSlot::Sonnet));
    }

    // 全局 knob 第二遍：只有用户显式拨过的 env knob 才压过 preset。
    if let Some(ms) = defaults.knobs.api_timeout_ms
        && base_url.is_some()
    {
        env.set(keys::API_TIMEOUT, &ms.to_string(), ValueSource::Global);
    }
    for knob in TypedKnob::ALL {
        let Some(key) = knob.default_env_key() else {
            continue;
        };
        if knob.requires_endpoint() && base_url.is_none() {
            continue;
        }
        match knob.bool_override(&defaults.knobs) {
            Some(true) => env.set(key, "1", ValueSource::Global),
            Some(false) => env.remove(key),
            None => {}
        }
    }
    match defaults.knobs.tool_search {
        Some(true) => env.set(keys::TOOL_SEARCH, "true", ValueSource::Global),
        Some(false) => env.set(keys::TOOL_SEARCH, "false", ValueSource::Global),
        None => {}
    }
    for knob in BOOL_KNOBS {
        if let KnobTarget::Env(key) = knob.target {
            if defaults.knobs.flag(knob) != knob.claude_default {
                env.set(key, "1", ValueSource::Global);
            } else if defaults.knobs.is_explicit(knob.field) {
                env.remove(key);
            }
        }
    }
    for knob in VALUE_KNOBS {
        if let KnobTarget::Env(key) = knob.target
            && let Some(value) = defaults.knobs.value(knob)
        {
            env.set(key, &Knobs::scalar_text(value), ValueSource::Global);
        }
    }
    if spec_sonnet.is_none() {
        write_package(&mut env, preset, defaults_sonnet.map(String::as_str));
    }
    for (k, v) in &defaults.env {
        env.set(k, v, ValueSource::Global);
    }
    write_slots(&mut env, &spec.intent.models, ValueSource::Profile);
    write_package(&mut env, preset, spec_sonnet);
    // 用户给生效 sonnet 定义过窗口：这个数字压过 preset package，按同一套规则展开成配套 env
    if let Some(sonnet) = effective_models(spec, preset, defaults).get(ModelSlot::Sonnet)
        && let Some(max_context) = spec.intent.model_windows.get(sonnet)
    {
        let custom = ModelChoice {
            id: sonnet.to_string(),
            max_context: Some(*max_context),
            ..Default::default()
        };
        for (key, value) in custom.package_env() {
            env.set(&key, &value, ValueSource::Profile);
        }
    }
    for (k, v) in &spec.intent.env {
        env.set(k, v, ValueSource::Profile);
    }

    // shared 模式下 Claude 读的是 ~/.claude 的 settings.json，这一层改走 [`shared_overlay`]
    let (settings_top, permissions_default_mode) = match spec.isolation {
        Isolation::Full => settings_layer(defaults),
        Isolation::Shared => (Vec::new(), None),
    };

    // donn 常量最后写：即使覆盖里出现同名 key，也以 donn 为准
    env.set(keys::AUTOUPDATER, "1", ValueSource::Default);
    // shared 模式显式停用 claude.ai connectors。第三方 env 认证下它本来就不可用；
    // 显式停用会走 Claude Code 的静默分支，避免出现 "auth source takes precedence" 常驻横幅。
    if spec.isolation == Isolation::Shared {
        env.set(keys::CLAUDEAI_MCP, "0", ValueSource::Default);
    }

    // .claude.json：onboarding 恒写；只有写入非空 api_key 时才写确认屏白名单
    let mut claude_json = vec![keys::ONBOARDING.to_string()];
    let approved_key_suffix = api_key_written.map(|s| {
        claude_json.push(keys::API_RESPONSES.to_string());
        s.suffix20()
    });

    let (env, sources) = env.into_parts();
    (
        Rendered {
            env,
            settings_top,
            permissions_default_mode,
            claude_json,
            approved_key_suffix,
        },
        sources,
    )
}

/// settings.json 顶层这一层：settings 类 knob < `[defaults.settings]`（同名 key 覆盖），
/// 再加上 permission mode。只取决于全局配置，与 profile 无关。
fn settings_layer(defaults: &Defaults) -> (Vec<(String, serde_json::Value)>, Option<String>) {
    let mut top: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    if TypedKnob::HideAttribution.bool_value(&defaults.knobs) == Some(true) {
        top.insert(keys::ATTRIBUTION.into(), keys::attribution_hidden());
    }
    // default 档和非法存量值都不管理；bypass 档额外写确认屏标记。
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

/// shared 模式的 `--settings` overlay：`settings_layer` 的全部内容，再加上 `model`。
/// `~/.claude` 里持久化的 /model 选择会盖过 slot 映射，用 overlay 里的 model 压回 profile 的 sonnet。
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

fn write_slots(env: &mut EnvBuilder, models: &SlotMap, source: ValueSource) {
    for slot in ModelSlot::ALL {
        if let Some(model) = models.get(slot) {
            env.set(slot.env_key(), model, source);
        }
    }
}

/// 启动时要从继承的 shell env 里剥掉的 key：donn 会写的全部 env key，加上
/// [`keys::SHELL_OVERRIDE_KEYS`]。不剥的话，shell 里 export 过的同名变量会让「knob 关掉」失效。
pub fn managed_env_keys() -> Vec<&'static str> {
    let mut out: std::collections::BTreeSet<&'static str> = keys::MANAGED_ENV_KEYS
        .into_iter()
        .chain(ModelSlot::ALL.iter().map(|slot| slot.env_key()))
        .chain(keys::SHELL_OVERRIDE_KEYS)
        .collect();
    let knob_targets =
        (BOOL_KNOBS.iter().map(|k| k.target)).chain(VALUE_KNOBS.iter().map(|k| k.target));
    for target in knob_targets {
        if let KnobTarget::Env(key) = target {
            out.insert(key);
        }
    }
    out.into_iter().collect()
}

fn write_package(env: &mut EnvBuilder, preset: &Preset, sonnet_id: Option<&str>) {
    if let Some(choice) = sonnet_id.and_then(|id| preset.choice_for_model(id)) {
        // package 的值来自 preset；model 的选择位置只决定写入顺序。
        for (key, value) in choice.package_env() {
            env.set(&key, &value, ValueSource::Default);
        }
    }
}

/// 同 key 后写同时覆盖值和来源；输出按 key 名排序。
#[derive(Default)]
struct EnvBuilder(BTreeMap<String, (String, ValueSource)>);

impl EnvBuilder {
    fn set(&mut self, key: &str, value: &str, source: ValueSource) {
        self.0.insert(key.to_string(), (value.to_string(), source));
    }

    /// 移除 key（用户显式关掉 knob 时压过 preset 的声明）。
    fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn into_parts(self) -> (Vec<(String, String)>, BTreeMap<String, ValueSource>) {
        self.0
            .into_iter()
            .map(|(key, (value, source))| ((key.clone(), value), (key, source)))
            .unzip()
    }
}
