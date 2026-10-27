//! 全局 knob：donn 认识的 Claude Code 开关和选项。一个 key 对应一个值的放 [`BOOL_KNOBS`] / [`VALUE_KNOBS`]
//! 表，带条件或要成对写 key 的放 [`Knobs`] 的类型化字段。每个 knob 写哪个 key、什么时候写，
//! 见 docs/GLOBAL-SETTINGS.md；落盘在 `config.toml [defaults.knobs]`（[`crate::config`]）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::keys;

/// 简单 knob 落到哪种 Claude Code key。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobTarget {
    /// settings.json `env` 里的 key；布尔 knob 的生效值与 Claude 默认不同时写 `"1"`。
    Env(&'static str),
    /// settings.json 顶层字段；布尔 knob 写 `true` / `false`，标量原样写。
    Setting(&'static str),
}

/// 设置面板的分组；顺序就是面板顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobGroup {
    /// session 和 model 行为。
    Session,
    /// 只对第三方 endpoint 有意义的项。
    Endpoint,
    /// 署名和 connectors。
    Privacy,
}

impl KnobGroup {
    pub const ALL: [KnobGroup; 3] = [KnobGroup::Session, KnobGroup::Endpoint, KnobGroup::Privacy];
}

/// 一个布尔值直接映射一个 key、没有特殊优先级的 knob。`claude_default` 是 key 不存在时
/// Claude Code 的行为；生效值与它相同时不写 key，显式设成默认值则删掉 preset 写的同名 key。
/// 新增这类 knob 时，这里加一行，[`crate::config::CONFIG_DOC`] 加一行，CLI 层的 `i18n::knob_label` 加行名。
#[derive(Debug, Clone, Copy)]
pub struct BoolKnob {
    /// config.toml `[defaults.knobs]` 里的 key 名。
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

/// 留在 [`Knobs`] 类型化字段里的 knob。`ALL` 的顺序就是设置面板的分组内顺序；
/// 布尔开关的字段省略值见 [`TypedKnob::on_when_unset`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKnob {
    /// 默认 permission mode；写 `permissions.defaultMode` 和确认标记。
    PermissionMode,
    /// MCP tool search（env），三态：不设置就不写 key。
    ToolSearch,
    /// agent teams：donn 产品默认开，写 [`keys::AGENT_TEAMS`]。
    AgentTeams,
    /// 第三方 endpoint 请求超时毫秒（env，仅配置了 endpoint 的 provider）。
    ApiTimeoutMs,
    /// 禁非必要网络流量：默认关，只对配置了 endpoint 的 profile 写 [`keys::NONESSENTIAL_TRAFFIC`]。
    DisableNonessentialTraffic,
    /// 隐藏 Claude 署名：默认关，写 `attribution`。
    HideAttribution,
}

impl TypedKnob {
    /// 面板分组内的顺序：session、endpoint、privacy。
    pub const ALL: [TypedKnob; 6] = [
        TypedKnob::PermissionMode,
        TypedKnob::ToolSearch,
        TypedKnob::AgentTeams,
        TypedKnob::ApiTimeoutMs,
        TypedKnob::DisableNonessentialTraffic,
        TypedKnob::HideAttribution,
    ];

    pub fn field(self) -> &'static str {
        match self {
            TypedKnob::PermissionMode => "permission_mode",
            TypedKnob::ToolSearch => "tool_search",
            TypedKnob::AgentTeams => "agent_teams",
            TypedKnob::ApiTimeoutMs => "api_timeout_ms",
            TypedKnob::DisableNonessentialTraffic => "disable_nonessential_traffic",
            TypedKnob::HideAttribution => "hide_attribution",
        }
    }

    /// 设置面板分组。
    pub fn group(self) -> KnobGroup {
        match self {
            TypedKnob::PermissionMode | TypedKnob::ToolSearch | TypedKnob::AgentTeams => {
                KnobGroup::Session
            }
            TypedKnob::ApiTimeoutMs | TypedKnob::DisableNonessentialTraffic => KnobGroup::Endpoint,
            TypedKnob::HideAttribution => KnobGroup::Privacy,
        }
    }

    /// 字段省略时的生效值；`None` = 省略时不写 key（tool search）或不是布尔 knob。
    pub fn on_when_unset(self) -> Option<bool> {
        match self {
            TypedKnob::AgentTeams => Some(true),
            TypedKnob::ToolSearch | TypedKnob::PermissionMode | TypedKnob::ApiTimeoutMs => None,
            TypedKnob::DisableNonessentialTraffic | TypedKnob::HideAttribution => Some(false),
        }
    }

    /// 默认值写进 env 的布尔开关；tool search 和署名不在这条通路里。
    pub fn default_env_key(self) -> Option<&'static str> {
        match self {
            TypedKnob::AgentTeams => Some(keys::AGENT_TEAMS),
            TypedKnob::DisableNonessentialTraffic => Some(keys::NONESSENTIAL_TRAFFIC),
            _ => None,
        }
    }

    /// 只对配置了 endpoint 的 profile 生效。
    pub fn requires_endpoint(self) -> bool {
        matches!(self, TypedKnob::DisableNonessentialTraffic)
    }

    /// 布尔 knob 的生效值；非布尔 knob 返回 `None`。`ToolSearch` 的 `None` 表示跟随上游。
    pub fn bool_value(self, knobs: &Knobs) -> Option<bool> {
        match self {
            TypedKnob::ToolSearch => knobs.tool_search,
            TypedKnob::AgentTeams => Some(knobs.agent_teams.unwrap_or(true)),
            TypedKnob::DisableNonessentialTraffic => {
                Some(knobs.disable_nonessential_traffic.unwrap_or(false))
            }
            TypedKnob::HideAttribution => Some(knobs.hide_attribution.unwrap_or(false)),
            TypedKnob::PermissionMode | TypedKnob::ApiTimeoutMs => None,
        }
    }

    /// 显式写入的值；`None` = 跟随省略时的默认。
    pub fn bool_override(self, knobs: &Knobs) -> Option<bool> {
        match self {
            TypedKnob::ToolSearch => knobs.tool_search,
            TypedKnob::AgentTeams => knobs.agent_teams,
            TypedKnob::DisableNonessentialTraffic => knobs.disable_nonessential_traffic,
            TypedKnob::HideAttribution => knobs.hide_attribution,
            TypedKnob::PermissionMode | TypedKnob::ApiTimeoutMs => None,
        }
    }

    fn bool_slot(self, knobs: &mut Knobs) -> Option<&mut Option<bool>> {
        match self {
            TypedKnob::ToolSearch => Some(&mut knobs.tool_search),
            TypedKnob::AgentTeams => Some(&mut knobs.agent_teams),
            TypedKnob::DisableNonessentialTraffic => Some(&mut knobs.disable_nonessential_traffic),
            TypedKnob::HideAttribution => Some(&mut knobs.hide_attribution),
            TypedKnob::PermissionMode | TypedKnob::ApiTimeoutMs => None,
        }
    }

    /// 翻到 `next`；与省略时的值相同就回到省略，不给 provider 写多余的 key。
    pub fn set_bool(self, knobs: &mut Knobs, next: bool) -> bool {
        let Some(default) = self.on_when_unset() else {
            return false;
        };
        let Some(slot) = self.bool_slot(knobs) else {
            return false;
        };
        *slot = (next != default).then_some(next);
        true
    }

    /// 只合并与 `base` 不同的字段；`current` 是落盘前重读的配置。
    pub fn apply_if_changed(self, current: &mut Knobs, base: &Knobs, value: &Knobs) {
        match self {
            TypedKnob::PermissionMode => {
                if value.permission_mode != base.permission_mode {
                    current.permission_mode = value.permission_mode.clone();
                }
            }
            TypedKnob::ToolSearch => {
                if value.tool_search != base.tool_search {
                    current.tool_search = value.tool_search;
                }
            }
            TypedKnob::AgentTeams => {
                if value.agent_teams != base.agent_teams {
                    current.agent_teams = value.agent_teams;
                }
            }
            TypedKnob::ApiTimeoutMs => {
                if value.api_timeout_ms != base.api_timeout_ms {
                    current.api_timeout_ms = value.api_timeout_ms;
                }
            }
            TypedKnob::DisableNonessentialTraffic => {
                if value.disable_nonessential_traffic != base.disable_nonessential_traffic {
                    current.disable_nonessential_traffic = value.disable_nonessential_traffic;
                }
            }
            TypedKnob::HideAttribution => {
                if value.hide_attribution != base.hide_attribution {
                    current.hide_attribution = value.hide_attribution;
                }
            }
        }
    }

    /// 类型化字段的取值校验；每个需要校验的变体都在这里有一支。
    pub fn validate(self, knobs: &Knobs) -> Result<()> {
        match self {
            TypedKnob::PermissionMode => {
                if let Some(mode) = &knobs.permission_mode
                    && !keys::PERMISSION_MODES.contains(&mode.as_str())
                {
                    return Err(Error::InvalidInput(format!(
                        "invalid permission_mode `{mode}` (expected one of {:?})",
                        keys::PERMISSION_MODES
                    )));
                }
            }
            TypedKnob::ApiTimeoutMs => {
                if let Some(ms) = knobs.api_timeout_ms
                    && ms > keys::API_TIMEOUT_MAX
                {
                    return Err(Error::InvalidInput(format!(
                        "api_timeout_ms must be at most {} (values above overflow the upstream timer and fail immediately)",
                        keys::API_TIMEOUT_MAX
                    )));
                }
            }
            TypedKnob::ToolSearch
            | TypedKnob::AgentTeams
            | TypedKnob::DisableNonessentialTraffic
            | TypedKnob::HideAttribution => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    /// 非负整数。
    Number,
    /// 有限取值；TUI 用选择框，非法存量值不写 key。
    Enum(&'static [&'static str]),
}

/// 一个标量直接映射一个 key 的 knob；不设置就不写 key（跟随 Claude Code）。
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

/// 全局 knob：donn 认识的 Claude Code 开关和选项，类型化定义，TUI 直接渲染成
/// 开关或选择器。`None` = 跟随默认；默认值见各 `*_on()` 取值方法。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Knobs {
    /// 实验特性 agent teams（env）。默认开。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_teams: Option<bool>,
    /// MCP tool search（env），三态：不设置就不写 key，`true` / `false` 照写。见 [`keys::TOOL_SEARCH`]。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_search: Option<bool>,
    /// 默认 permission mode；`None` = bypassPermissions。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// commit/PR 隐藏 Claude 署名（settings 的 attribution 置空）。默认关。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_attribution: Option<bool>,
    /// 第三方 endpoint 请求超时毫秒（env，只对配置了 endpoint 的 provider）。默认 600000。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_timeout_ms: Option<u64>,
    /// 禁非必要网络流量（env，只对配置了 endpoint 的 provider）。默认关。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_nonessential_traffic: Option<bool>,
    /// 简单 knob（[`BOOL_KNOBS`] / [`VALUE_KNOBS`]）和未知 key，按 config.toml 原样保留。
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// donn 认识的全部 knob key 名：类型化字段加上两张表。
pub fn knob_fields() -> impl Iterator<Item = &'static str> {
    TypedKnob::ALL
        .into_iter()
        .map(TypedKnob::field)
        .chain(BOOL_KNOBS.iter().map(|k| k.field))
        .chain(VALUE_KNOBS.iter().map(|k| k.field))
}

impl Knobs {
    pub fn permission_mode(&self) -> &str {
        self.permission_mode
            .as_deref()
            .unwrap_or(keys::BYPASS_PERMISSIONS)
    }
    pub fn api_timeout(&self) -> u64 {
        self.api_timeout_ms.unwrap_or(600_000)
    }

    /// 简单布尔 knob 的生效值（不设置 = Claude 默认）。
    pub fn flag(&self, knob: &BoolKnob) -> bool {
        self.extra
            .get(knob.field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(knob.claude_default)
    }

    /// 简单标量 knob 的值；不设置 = `None`（加载时已经校验类型）。
    pub fn value(&self, knob: &ValueKnob) -> Option<&serde_json::Value> {
        self.extra.get(knob.field)
    }

    pub fn is_explicit(&self, field: &str) -> bool {
        self.extra.contains_key(field)
    }

    /// 只合并与 `base` 不同的字段；进 lock 后重读的配置作为 `self`。
    pub fn apply_changes_from(&mut self, base: &Knobs, value: &Knobs) {
        for knob in TypedKnob::ALL {
            knob.apply_if_changed(self, base, value);
        }
        let keys: std::collections::BTreeSet<&String> =
            base.extra.keys().chain(value.extra.keys()).collect();
        for key in keys {
            if value.extra.get(key) != base.extra.get(key) {
                self.set_extra(key, value.extra.get(key).cloned());
            }
        }
    }

    /// 取值校验：类型化字段按各自的规则校验，简单 knob 必须符合各自的类型或枚举表。
    /// 未知 key 放行。
    pub fn validate(&self) -> Result<()> {
        for knob in TypedKnob::ALL {
            knob.validate(self)?;
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

    /// 标量 knob 值的文本形式：字符串原样，数字和布尔用 JSON 文本（env 值和面板显示共用）。
    pub fn scalar_text(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }

    /// `None` = 回到跟随默认（删 key）。
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
