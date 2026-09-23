//! 键注册表：donn 认识的全部 env / settings 键名，唯一定义处。
//! render / doctor / 读取视图都从这里取键名——新增托管键只改这一个文件。
//!
//! # 键的三类来源（每个键的注释里标注）
//! - **[官方]**：Claude Code 官方文档在册
//!   （<https://code.claude.com/docs/zh-CN/env-vars> 与 settings 文档）。
//! - **[官方·未在册]**：官方 env 文档未列，但在 claude 二进制里逆向验证过的真实键。
//!   上游行为变化风险高于在册键，doctor/升级时优先复查这批。
//! - **自定义**：用户在 TUI `env +` 里添加的任意键——donn 只做操作系统要求的
//!   名称/NUL 校验，其余原样注入，不在本注册表中。
//!
//! donn 自有的配置概念（preset、isolation、aliases、knobs…）不是键——
//! 它们存 profile.toml / config.toml，最终**渲染成**下列键。
//!
//! 未在册的只有 `CLAUDE_CODE_NO_MODEL_FALLBACK` 与 `.claude.json` 两个内部状态键
//! （hasCompletedOnboarding / customApiKeyResponses）。

use serde::{Deserialize, Serialize};

/// [官方] 端点。
pub const BASE_URL: &str = "ANTHROPIC_BASE_URL";
/// [官方] 认证（api_key 模式，发 X-Api-Key 头）。
pub const API_KEY: &str = "ANTHROPIC_API_KEY";
/// [官方] 认证（auth_token 模式，发 Authorization 头）。
pub const AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
/// [官方] 禁自动更新。隔离实例恒写，防止实例间版本漂移。
pub const AUTOUPDATER: &str = "DISABLE_AUTOUPDATER";
/// [官方] 请求超时毫秒（默认 600000，上限 2147483647——超出会溢出上游计时器）。
pub const API_TIMEOUT: &str = "API_TIMEOUT_MS";
/// [官方] `API_TIMEOUT_MS` 接受的最大值（i32 上限）。
pub const API_TIMEOUT_MAX: u64 = 2_147_483_647;
/// [官方] 禁非必要网络流量。该键设任意值均为禁，「关」只能删除键。
pub const NONESSENTIAL_TRAFFIC: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";
/// [官方] 实验特性：agent teams（上游默认关；donn 产品默认开，见 `Knobs::agent_teams_on`）。
pub const AGENT_TEAMS: &str = "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS";
/// [官方] MCP tool search（env）。unset = defer，但 `ANTHROPIC_BASE_URL` 指向非官方主机时全部
/// upfront；`"true"` 恒 defer 并发 beta 头（代理需支持 tool_reference）；`"auto"`/`"auto:N"`
/// 工具定义超上下文阈值才 defer；`"false"` 全部 upfront。设了
/// [`DISABLE_EXPERIMENTAL_BETAS`] 时本键被忽略。donn 默认不写键；显式 `true`/`false` 照写。
pub const TOOL_SEARCH: &str = "ENABLE_TOOL_SEARCH";

/// [官方·二进制 schema] settings.json 顶层键：commit/PR 署名（空串 = 不加）。
pub const ATTRIBUTION: &str = "attribution";
/// 「隐藏署名」旋钮写入的完整值。
pub fn attribution_hidden() -> serde_json::Value {
    serde_json::json!({"commit": "", "pr": "", "sessionUrl": false})
}
/// [官方·二进制 schema] settings.json 顶层键：持久化思考强度（档位见
/// [`EFFORT_SETTING_LEVELS`]）。v2.1.251 起 `/effort` 改存 `modelSettings`（按模型），
/// 同文件内按模型的存档优先于本键；env [`EFFORT`] 优先于本键与 `--effort` flag。
/// user settings（含 `CLAUDE_CONFIG_DIR/settings.json`）里的本键对 Opus 5.5 及之后的模型
/// 不生效，它们从自身默认档起步；project、local、managed、`--settings` 来源的本键对所有模型生效。
pub const EFFORT_SETTING: &str = "effortLevel";
/// [官方·二进制 schema] `effortLevel` 持久化设置的合法档位
/// （与 env 的 [`Effort`] 不同：无 max，auto = 不设）。
pub const EFFORT_SETTING_LEVELS: [&str; 4] = ["low", "medium", "high", "xhigh"];
/// [官方] settings.json 顶层键：`false` 为所有会话关闭 extended thinking
/// （thinking 默认开，`true` 无效果；Opus 5.5、Fable 等恒思考模型忽略此键）。
pub const ALWAYS_THINKING: &str = "alwaysThinkingEnabled";
/// [官方] settings.json 顶层键：思考强度上限（v2.1.267+），任何更高档位按上限运行；
/// 合法档位见 [`MAX_EFFORT_LEVELS`]，`max` = 不设上限。
pub const MAX_EFFORT_SETTING: &str = "maxEffortLevel";
/// [官方] `maxEffortLevel` 合法档位。
pub const MAX_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// settings.json 顶层键中 donn 另有专门通路管理、用户全局默认不得触碰的保留键。
pub const RESERVED_TOP_KEYS: [&str; 2] = ["env", "permissions"];
/// [官方·二进制 schema] settings.json 顶层键：显式停用 claude.ai connectors。
pub const CLAUDEAI_CONNECTORS: &str = "disableClaudeAiConnectors";
/// [官方·二进制 schema] settings.json 顶层键：已接受 bypass 确认屏，
/// 启动不再弹「危险模式」提示（与 permissions.defaultMode 旋钮成对写入）。
pub const SKIP_DANGEROUS_PROMPT: &str = "skipDangerousModePermissionPrompt";
/// [官方·二进制 schema] `permissions` 对象内的默认权限模式子键。
pub const PERMISSIONS_DEFAULT_MODE: &str = "defaultMode";
/// [官方] `permissions.defaultMode` 合法枚举。`auto`/`bypassPermissions` 只在 user、
/// managed、`--settings` 来源生效（project/local 写了不生效）；`manual` 是 `default`
/// 的别名（knob 不接受，用 `default` 档）。
pub const PERMISSION_MODES: [&str; 6] = [
    "default",
    "acceptEdits",
    "plan",
    "dontAsk",
    "auto",
    "bypassPermissions",
];
/// [官方·二进制 schema] `permissions.defaultMode` 的权限直通取值。
pub const BYPASS_PERMISSIONS: &str = "bypassPermissions";
/// [官方] claude.ai connectors 开关
/// （共享模式设 "0" 显式停用，走静默分支避免 env 认证优先横幅）。
pub const CLAUDEAI_MCP: &str = "ENABLE_CLAUDEAI_MCP_SERVERS";
/// [官方] 思考强度（low/medium/high/xhigh/max/auto；env 优先于持久化的 effortLevel）。
pub const EFFORT: &str = "CLAUDE_CODE_EFFORT_LEVEL";
/// [官方] 自动压缩触发窗口（token，`100000..=1000000`，只收纯整数）。超过上限的窗口按
/// [`AUTO_COMPACT_MAX`] 写；优先于 `--autocompact` flag 与 `autoCompactWindow` 设置键。
pub const AUTO_COMPACT: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
/// [官方] `CLAUDE_CODE_AUTO_COMPACT_WINDOW` 接受的最大值。
pub const AUTO_COMPACT_MAX: u64 = 1_000_000;
/// [官方] 最大上下文 token 数。与 [`AUTO_COMPACT`] 成对配置。
pub const MAX_CONTEXT: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";
/// [官方] subagent 默认模型（不设时部分渠道会静默失败）。
pub const SUBAGENT_MODEL: &str = "CLAUDE_CODE_SUBAGENT_MODEL";

// ── 简单旋钮映射的键（见 [`crate::knobs::BOOL_KNOBS`] / [`crate::knobs::VALUE_KNOBS`]）──
/// [官方] 去掉 anthropic-beta 头与 beta 工具字段；代理网关报 "Unexpected anthropic-beta" 时用。
pub const DISABLE_EXPERIMENTAL_BETAS: &str = "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS";
/// [官方] 单次回复最大输出 token（未知模型 id 默认 32000，超过模型上限按上限）。
pub const MAX_OUTPUT_TOKENS: &str = "CLAUDE_CODE_MAX_OUTPUT_TOKENS";
/// [官方] 对 Claude Code 不认识的模型 id（网关别名等）不按猜测的窗口提前压缩，
/// 只在 API 报 too-long 后压缩。preset 套餐给了 `max_context` 时不需要开。
pub const DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT: &str =
    "CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT";
/// [官方·未在册] 关闭模型不可用/被安全分类器拦截时的自动切换。
pub const NO_MODEL_FALLBACK: &str = "CLAUDE_CODE_NO_MODEL_FALLBACK";
/// [官方] 强制 subagent/teammate/workflow agent 都用 [`SUBAGENT_MODEL`]（v2.1.257+）。
pub const SUBAGENT_MODEL_FORCE: &str = "CLAUDE_CODE_SUBAGENT_MODEL_FORCE";
/// [官方] settings.json 顶层键：默认回复语言（任意语言名，原样进系统提示）。
pub const LANGUAGE: &str = "language";
/// [官方] settings.json 顶层键：上下文接近上限时自动压缩（默认 true）。
pub const AUTO_COMPACT_ENABLED: &str = "autoCompactEnabled";

/// [官方] donn 从不写、但启动时从继承的 shell 环境剥掉的键：留着会把会话改道到别的云厂商，
/// 或盖掉 profile 的模型映射。用户写进 `[defaults.env]` / `[intent.env]` 的照常注入。
pub const SHELL_OVERRIDE_KEYS: [&str; 7] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
];

/// 校验一条将交给操作系统的环境变量。名称只拒绝所有平台都无法可靠表示的
/// 空串、`=` 与 NUL；值只拒绝 NUL。错误信息绝不包含 value（它可能是 secret）。
pub fn validate_env_entry(key: &str, value: &str) -> crate::error::Result<()> {
    if key.is_empty() || key.as_bytes().contains(&b'=') || key.as_bytes().contains(&0) {
        return Err(crate::error::Error::InvalidInput(
            "environment variable name must not be empty or contain '=' or NUL".into(),
        ));
    }
    if value.as_bytes().contains(&0) {
        return Err(crate::error::Error::InvalidInput(format!(
            "environment variable '{key}' contains NUL"
        )));
    }
    Ok(())
}

/// 思考强度：类型化的 `CLAUDE_CODE_EFFORT_LEVEL` 取值序列。
/// `Auto` = 不写该键（跟随 Claude Code 自身设置）。选择弹窗按 [`Effort::ALL`] 顺序列出。
///
/// 合法值取自 claude 二进制的 `["low","medium","high","xhigh","max"]`；
/// `/effort` 里的 `ultracode` 不是合法 env 值（会话模式，内部映射 xhigh），不收录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effort {
    #[default]
    Auto,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 6] = [
        Effort::Auto,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];

    /// env 值；`Auto` 为 `None`（不写键）。
    pub fn env_value(self) -> Option<&'static str> {
        match self {
            Effort::Auto => None,
            Effort::Low => Some("low"),
            Effort::Medium => Some("medium"),
            Effort::High => Some("high"),
            Effort::Xhigh => Some("xhigh"),
            Effort::Max => Some("max"),
        }
    }

    /// 从 env 值解析。输入 `None`（键未设置）返回 `Some(Auto)`；
    /// 无法识别的值返回 `None`——调用方自行决定如何呈现/报错，不静默吞掉 typo。
    pub fn from_env(value: Option<&str>) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.env_value() == value)
    }

    /// 在 [`Effort::ALL`] 中的下标（UI 选中定位用）。
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|e| *e == self).unwrap_or(0)
    }
}
/// [官方·二进制 schema] `.claude.json` 顶层键：跳过首次向导。
pub const ONBOARDING: &str = "hasCompletedOnboarding";
/// [官方·二进制 schema] `.claude.json` 顶层键：API key 确认屏白名单。
pub const API_RESPONSES: &str = "customApiKeyResponses";

/// 模型槽位。带类型的枚举——拼错槽位名是编译错误，不是静默 no-op。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSlot {
    Sonnet,
    Opus,
    Fable,
    /// 也是后台任务（标题、摘要）用的模型。
    Haiku,
}

impl ModelSlot {
    pub const ALL: [ModelSlot; 4] = [
        ModelSlot::Sonnet,
        ModelSlot::Opus,
        ModelSlot::Fable,
        ModelSlot::Haiku,
    ];

    /// 对应的 settings.json env 键（全部 [官方]）。
    pub fn env_key(self) -> &'static str {
        match self {
            ModelSlot::Sonnet => "ANTHROPIC_DEFAULT_SONNET_MODEL",
            ModelSlot::Opus => "ANTHROPIC_DEFAULT_OPUS_MODEL",
            ModelSlot::Fable => "ANTHROPIC_DEFAULT_FABLE_MODEL",
            ModelSlot::Haiku => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        }
    }

    /// UI 展示名。
    pub fn label(self) -> &'static str {
        match self {
            ModelSlot::Sonnet => "sonnet",
            ModelSlot::Opus => "opus",
            ModelSlot::Fable => "fable",
            ModelSlot::Haiku => "haiku",
        }
    }

    /// 在 [`ModelSlot::ALL`] 中的下标（UI 并列存放各槽输入框时索引用）。
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// 模型槽映射，按 [`ModelSlot`] 索引（sonnet/opus/fable/haiku）。
/// serde 字段名与 preset TOML 的 `[preset.models]` 段一致。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotMap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku: Option<String>,
}

impl SlotMap {
    pub fn get(&self, slot: ModelSlot) -> Option<&str> {
        match slot {
            ModelSlot::Sonnet => self.sonnet.as_deref(),
            ModelSlot::Opus => self.opus.as_deref(),
            ModelSlot::Fable => self.fable.as_deref(),
            ModelSlot::Haiku => self.haiku.as_deref(),
        }
    }

    /// 空字符串视为清除。
    pub fn set(&mut self, slot: ModelSlot, value: Option<String>) {
        let target = match slot {
            ModelSlot::Sonnet => &mut self.sonnet,
            ModelSlot::Opus => &mut self.opus,
            ModelSlot::Fable => &mut self.fable,
            ModelSlot::Haiku => &mut self.haiku,
        };
        *target = value.filter(|v| !v.is_empty());
    }

    pub fn is_empty(&self) -> bool {
        ModelSlot::ALL.iter().all(|&s| self.get(s).is_none())
    }

    /// 已填槽位遍历。
    pub fn iter(&self) -> impl Iterator<Item = (ModelSlot, &str)> {
        ModelSlot::ALL
            .into_iter()
            .filter_map(|slot| self.get(slot).map(|v| (slot, v)))
    }

    /// `self` 覆盖 `base` 的逐槽合并结果（生效值 = 覆盖 > 底座）。
    pub fn over(&self, base: &SlotMap) -> SlotMap {
        let mut out = SlotMap::default();
        for slot in ModelSlot::ALL {
            out.set(
                slot,
                self.get(slot)
                    .or_else(|| base.get(slot))
                    .map(str::to_string),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_env_keys_are_distinct() {
        let keys: Vec<&str> = ModelSlot::ALL.iter().map(|s| s.env_key()).collect();
        let unique: std::collections::HashSet<_> = keys.iter().copied().collect();
        assert_eq!(keys.len(), unique.len());
    }

    #[test]
    fn effort_roundtrip() {
        // env 值往返：每个档位 env_value → from_env 恒等；index 与 ALL 一致
        for (i, e) in Effort::ALL.into_iter().enumerate() {
            assert_eq!(Effort::from_env(e.env_value()), Some(e));
            assert_eq!(e.index(), i);
        }
        // 未设置 = Auto；未知值不吞，返回 None 由调用方处理
        assert_eq!(Effort::from_env(None), Some(Effort::Auto));
        assert_eq!(Effort::from_env(Some("ultra")), None);
        assert_eq!(Effort::from_env(Some("max")), Some(Effort::Max));
    }

    #[test]
    fn slot_map_get_set_iter() {
        let mut m = SlotMap::default();
        assert!(m.is_empty());
        m.set(ModelSlot::Sonnet, Some("glm-5.1".into()));
        m.set(ModelSlot::Haiku, Some(String::new())); // 空 = 清除
        assert_eq!(m.get(ModelSlot::Sonnet), Some("glm-5.1"));
        assert_eq!(m.get(ModelSlot::Haiku), None);
        let filled: Vec<_> = m.iter().collect();
        assert_eq!(filled, vec![(ModelSlot::Sonnet, "glm-5.1")]);
    }

    #[test]
    fn slot_map_over_merges_per_slot() {
        let base = SlotMap {
            sonnet: Some("p-sonnet".into()),
            haiku: Some("p-haiku".into()),
            ..Default::default()
        };
        let over = SlotMap {
            sonnet: Some("user-sonnet".into()),
            ..Default::default()
        };
        let merged = over.over(&base);
        assert_eq!(merged.get(ModelSlot::Sonnet), Some("user-sonnet"));
        assert_eq!(merged.get(ModelSlot::Haiku), Some("p-haiku"));
        assert_eq!(merged.get(ModelSlot::Opus), None);
    }

    #[test]
    fn environment_entries_reject_only_os_invalid_data() {
        for (key, value) in [("A", "1"), ("A B", ""), ("_CUSTOM", "x=y")] {
            validate_env_entry(key, value).unwrap();
        }
        for (key, value) in [("", "1"), ("A=B", "1"), ("A\0B", "1"), ("A", "x\0y")] {
            assert!(validate_env_entry(key, value).is_err(), "{key:?}={value:?}");
        }
    }
}
