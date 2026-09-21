//! 全局旋钮：donn 认识的 Claude Code 开关/选项。一键一值的放 [`BOOL_KNOBS`] / [`VALUE_KNOBS`]
//! 表，带条件或要成对写键的是 [`Knobs`] 的类型化字段。每个旋钮写哪个键、什么时候写，
//! 见 docs/GLOBAL-SETTINGS.md；落盘在 `config.toml [defaults.knobs]`（[`crate::config`]）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::keys;

/// 简单旋钮落到哪种 Claude Code 键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobTarget {
    /// settings.json `env` 内的键；布尔旋钮生效值与 Claude 默认不同时写 `"1"`。
    Env(&'static str),
    /// settings.json 顶层字段；布尔旋钮写 `true`/`false`，标量原样写。
    Setting(&'static str),
}

/// 设置面板的分组；顺序即面板顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobGroup {
    /// 会话与模型行为。
    Session,
    /// 只对第三方端点有意义的项。
    Endpoint,
    /// 署名与 connectors。
    Privacy,
}

impl KnobGroup {
    pub const ALL: [KnobGroup; 3] = [KnobGroup::Session, KnobGroup::Endpoint, KnobGroup::Privacy];
}

/// 一个布尔直接映射一个键、无特殊优先级的旋钮。`claude_default` 是键不存在时
/// Claude Code 的行为——生效值与之相同就不写键（显式设成默认值则删掉 preset 写的同名键）。
/// 新增此类旋钮 = 这里加一行 + [`crate::config::CONFIG_DOC`] 加一行 + CLI 层 `i18n::knob_label` 加行名。
#[derive(Debug, Clone, Copy)]
pub struct BoolKnob {
    /// config.toml `[defaults.knobs]` 键名。
    pub field: &'static str,
    pub group: KnobGroup,
    pub target: KnobTarget,
    pub claude_default: bool,
}

pub const BOOL_KNOBS: &[BoolKnob] = &[
    BoolKnob {
        field: "thinking",
        group: KnobGroup::Session,
        target: KnobTarget::Setting(keys::ALWAYS_THINKING),
        claude_default: true,
    },
    BoolKnob {
        field: "auto_compact",
        group: KnobGroup::Session,
        target: KnobTarget::Setting(keys::AUTO_COMPACT_ENABLED),
        claude_default: true,
    },
    BoolKnob {
        field: "model_fallback",
        group: KnobGroup::Session,
        target: KnobTarget::Env(keys::NO_MODEL_FALLBACK),
        claude_default: true,
    },
    BoolKnob {
        field: "subagent_model_force",
        group: KnobGroup::Session,
        target: KnobTarget::Env(keys::SUBAGENT_MODEL_FORCE),
        claude_default: false,
    },
    BoolKnob {
        field: "disable_experimental_betas",
        group: KnobGroup::Endpoint,
        target: KnobTarget::Env(keys::DISABLE_EXPERIMENTAL_BETAS),
        claude_default: false,
    },
    BoolKnob {
        field: "disable_unknown_model_window_enforcement",
        group: KnobGroup::Endpoint,
        target: KnobTarget::Env(keys::DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT),
        claude_default: false,
    },
    BoolKnob {
        field: "disable_connectors",
        group: KnobGroup::Privacy,
        target: KnobTarget::Setting(keys::CLAUDEAI_CONNECTORS),
        claude_default: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    /// 非负整数。
    Number,
    /// 有限取值；TUI 用选择框，非法存量值不写键。
    Enum(&'static [&'static str]),
}

/// 一个标量直接映射一个键的旋钮；未设置 = 不写键（跟随 Claude Code）。
#[derive(Debug, Clone, Copy)]
pub struct ValueKnob {
    pub field: &'static str,
    pub group: KnobGroup,
    pub target: KnobTarget,
    pub kind: ValueKind,
}

impl ValueKnob {
    /// 值是否符合 `kind`（Enum 只认表内取值）。
    pub fn accepts(&self, value: &serde_json::Value) -> bool {
        match self.kind {
            ValueKind::Text => value.is_string(),
            ValueKind::Number => value.is_u64(),
            ValueKind::Enum(levels) => value.as_str().is_some_and(|s| levels.contains(&s)),
        }
    }
}

pub const VALUE_KNOBS: &[ValueKnob] = &[
    ValueKnob {
        field: "effort",
        group: KnobGroup::Session,
        target: KnobTarget::Setting(keys::EFFORT_SETTING),
        kind: ValueKind::Enum(&keys::EFFORT_SETTING_LEVELS),
    },
    ValueKnob {
        field: "max_effort",
        group: KnobGroup::Session,
        target: KnobTarget::Setting(keys::MAX_EFFORT_SETTING),
        kind: ValueKind::Enum(&keys::MAX_EFFORT_LEVELS),
    },
    ValueKnob {
        field: "language",
        group: KnobGroup::Session,
        target: KnobTarget::Setting(keys::LANGUAGE),
        kind: ValueKind::Text,
    },
    ValueKnob {
        field: "max_output_tokens",
        group: KnobGroup::Session,
        target: KnobTarget::Env(keys::MAX_OUTPUT_TOKENS),
        kind: ValueKind::Number,
    },
];

/// 全局旋钮：donn 认识的 Claude Code 开关/选项，类型化定义（TUI 直接渲染成
/// 开关/选择器）。`None` = 跟随默认；默认值见各 `*_on()` 取值方法。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Knobs {
    /// 实验特性 agent teams（env）。默认：开。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_teams: Option<bool>,
    /// MCP tool search（env），三态：不设 = 不写键，`true` / `false` 照写。见 [`keys::TOOL_SEARCH`]。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_search: Option<bool>,
    /// 默认权限模式；`None` = bypassPermissions。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// commit/PR 隐藏 Claude 署名（settings attribution 置空）。默认：关。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_attribution: Option<bool>,
    /// 第三方端点请求超时毫秒（env，仅配了端点的渠道）。默认：600000。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_timeout_ms: Option<u64>,
    /// 禁非必要网络流量（env，仅配了端点的渠道）。默认：关。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_nonessential_traffic: Option<bool>,
    /// 简单旋钮（[`BOOL_KNOBS`] / [`VALUE_KNOBS`]）与未知键，按 config.toml 原样保留。
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Knobs {
    pub fn agent_teams_on(&self) -> bool {
        self.agent_teams.unwrap_or(true)
    }
    pub fn permission_mode(&self) -> &str {
        self.permission_mode
            .as_deref()
            .unwrap_or(crate::keys::BYPASS_PERMISSIONS)
    }
    pub fn hide_attribution_on(&self) -> bool {
        self.hide_attribution.unwrap_or(false)
    }
    pub fn api_timeout(&self) -> u64 {
        self.api_timeout_ms.unwrap_or(600_000)
    }
    pub fn disable_nonessential_traffic_on(&self) -> bool {
        self.disable_nonessential_traffic.unwrap_or(false)
    }

    /// 简单布尔旋钮的生效值（未设置 = Claude 默认）。
    pub fn flag(&self, knob: &BoolKnob) -> bool {
        self.extra
            .get(knob.field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(knob.claude_default)
    }

    /// 简单标量旋钮的值；未设置 = `None`（加载时已校验类型）。
    pub fn value(&self, knob: &ValueKnob) -> Option<&serde_json::Value> {
        self.extra.get(knob.field)
    }

    pub fn is_explicit(&self, field: &str) -> bool {
        self.extra.contains_key(field)
    }

    /// 取值校验：权限模式必须在枚举内，简单旋钮必须符合各自类型/枚举表；
    /// api_timeout_ms 不得超上游 i32 上限（超出后请求立即失败）。未知键放行。
    pub fn validate(&self) -> Result<()> {
        if let Some(mode) = &self.permission_mode
            && !keys::PERMISSION_MODES.contains(&mode.as_str())
        {
            return Err(Error::InvalidInput(format!(
                "invalid permission_mode `{mode}` (expected one of {:?})",
                keys::PERMISSION_MODES
            )));
        }
        if let Some(ms) = self.api_timeout_ms
            && ms > keys::API_TIMEOUT_MAX
        {
            return Err(Error::InvalidInput(format!(
                "api_timeout_ms must be at most {} (values above overflow the upstream timer and fail immediately)",
                keys::API_TIMEOUT_MAX
            )));
        }
        for knob in BOOL_KNOBS {
            if let Some(value) = self.extra.get(knob.field)
                && !value.is_boolean()
            {
                return Err(Error::InvalidInput(format!(
                    "knob `{}` must be true or false, got {value}",
                    knob.field
                )));
            }
        }
        for knob in VALUE_KNOBS {
            if let Some(value) = self.extra.get(knob.field)
                && !knob.accepts(value)
            {
                let expected = match knob.kind {
                    ValueKind::Text => "a string".to_string(),
                    ValueKind::Number => "a non-negative integer".to_string(),
                    ValueKind::Enum(levels) => format!("one of {levels:?}"),
                };
                return Err(Error::InvalidInput(format!(
                    "knob `{}` must be {expected}, got {value}",
                    knob.field
                )));
            }
        }
        Ok(())
    }

    /// 标量旋钮值的文本形式：字符串原样，数字/布尔用 JSON 文本（env 值与面板显示共用）。
    pub fn scalar_text(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }

    /// `None` = 回到跟随默认（删键）。
    pub fn set_extra(&mut self, field: &str, value: Option<serde_json::Value>) {
        match value {
            Some(value) => {
                self.extra.insert(field.to_string(), value);
            }
            None => {
                self.extra.remove(field);
            }
        }
    }
}
