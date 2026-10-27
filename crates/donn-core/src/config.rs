//! 全局 `~/.donn/config.toml`（所有字段都可省略）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::home::DonnHome;
use crate::knobs::Knobs;

/// config.toml 的注释文档头。保存时总是重新附上，首次运行也用同一份模板。
pub const CONFIG_DOC: &str = r#"# ~/.donn/config.toml：donn 全局配置。本注释就是文档，保存和启动时会自动刷新。
# 所有字段都可省略，缺失表示跟随默认。改完运行 donn（或任一 profile 命令）即生效。
# 优先级：donn 默认 < provider preset < 本文件 < profile 单独设置。
#
# bin_dir = "~/.local/bin"        # alias 命令的落盘目录；省略 = 平台默认
# claude_bin = "/path/to/claude"  # claude 二进制路径；省略 = 在 PATH 里查找
#
# [defaults.knobs]           # 类型化 knob（类型写错时加载即报错）
# env 前缀的 key 写进 settings.json 的 env（两种 isolation 都生效）；顶层 key 在 full 模式直接写入，
# shared 模式由 sync 生成 shared-settings.json overlay，启动时经 --settings 带进 session
# （不写 ~/.claude）。
# agent_teams = true         # 实验特性 agent teams（默认开）
# tool_search = true         # MCP tool search。不写 = 交给 Claude Code 决定（gateway 后 v2.1.304+ 同样按需加载）；true = 强制按需加载（发 beta 头，proxy 要支持 tool_reference）；false = 一次全加载
# permission_mode = "bypassPermissions" # 默认 permission mode：default/acceptEdits/plan/dontAsk/auto/bypassPermissions
#                            # （默认 bypassPermissions = 权限直通；default = 交还 Claude Code 管理）
# hide_attribution = false   # 隐藏 Claude 署名：commit/PR 文本和 session 链接（默认关）
# effort = "medium"          # 思考强度 low/medium/high/xhigh；省略 = 跟随 Claude Code
# max_effort = "max"         # 思考强度上限 low/medium/high/xhigh/max；省略 = 不设上限
# disable_connectors = false # 停用 claude.ai connectors（默认关）
# thinking = true            # extended thinking（默认开）；false = 为所有 session 关闭
# api_timeout_ms = 600000    # 请求超时毫秒，只对配置了 endpoint 的 provider 注入（默认 600000；上限 2147483647，超出后请求立即失败）
# disable_nonessential_traffic = false # 禁 telemetry、更新检查等非必要流量，只对配置了 endpoint 的 provider（默认关）
# language = "中文"           # 回复语言，任意语言名；省略 = 跟随 session
# disable_experimental_betas = false # 去掉 anthropic-beta 头和 beta 字段：proxy gateway 报 Unexpected anthropic-beta 时开
# disable_unknown_model_window_enforcement = false # 不认识的 model id 不按猜测窗口提前压缩（默认关；套餐已给 max_context 时不需要）
# max_output_tokens = 64000  # 单次回复最大输出 token；省略 = model 默认（未知 id 为 32000）
# auto_compact = true        # 上下文接近上限时自动压缩（默认开）
# model_fallback = true      # model 不可用或被安全分类器拦截时自动切换 model（默认开）
# subagent_model_force = false # 强制 subagent/teammate 都用 CLAUDE_CODE_SUBAGENT_MODEL（默认关）
#
# [defaults.env]             # 自由 env：注入每个 profile 的 settings.json env
# MAX_THINKING_TOKENS = "31999"
#
# [defaults.settings]        # 自由 settings.json 顶层字段（支持嵌套表；env/permissions 是保留 key，无效）
# spinnerTipsEnabled = false
"#;

#[cfg(debug_assertions)]
fn assert_knob_docs_cover_all_fields() {
    let reference = include_str!("../../../docs/GLOBAL-SETTINGS.md");
    for field in crate::knobs::knob_fields() {
        debug_assert!(
            CONFIG_DOC.contains(&format!("# {field} =")),
            "CONFIG_DOC lacks knob `{field}`"
        );
        debug_assert!(
            reference.contains(&format!("| `{field}` |")),
            "GLOBAL-SETTINGS.md lacks knob `{field}`"
        );
    }
}

/// 首次运行生成带注释的空模板。文件已经存在时，只把开头的模板注释头刷新到当前版本，
/// 正文原样不动。头已经是最新版本时不落盘。
pub fn ensure_config_doc(home: &DonnHome) -> Result<()> {
    #[cfg(debug_assertions)]
    assert_knob_docs_cover_all_fields();
    let path = home.config_file();
    let Some(existing) = fsx::read_if_exists(&path)? else {
        return fsx::write_atomic(&path, CONFIG_DOC.as_bytes());
    };
    let refreshed = refresh_doc_header(&existing);
    if refreshed != existing {
        fsx::write_atomic(&path, refreshed.as_bytes())?;
    }
    Ok(())
}

/// 文件开头连续的注释或空行是 donn 的模板头，替换成 [`CONFIG_DOC`]；其余内容原样保留。
fn refresh_doc_header(existing: &str) -> String {
    let body = body_without_doc_header(existing);
    if body.is_empty() {
        CONFIG_DOC.to_string()
    } else {
        format!("{CONFIG_DOC}\n{body}")
    }
}

/// 去掉文件开头连续的注释或空行（模板头），返回正文。
fn body_without_doc_header(existing: &str) -> String {
    existing
        .split_inclusive('\n')
        .skip_while(|line| {
            let line = line.trim_end_matches(['\n', '\r']);
            line.is_empty() || line.starts_with('#')
        })
        .collect()
}

/// 用户全局默认：注入每个 profile 的个人偏好，一处配置对所有 profile 生效。
/// knob（类型化开关或选项）加自由 env/settings 补充。它在渲染层序里的位置见 [`crate::render::render`]。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    /// 全局 knob（None 字段 = 跟随默认）。
    #[serde(skip_serializing_if = "knobs_is_default")]
    pub knobs: Knobs,
    /// 注入 settings.json `env` 的 key-value（自由补充，会覆盖 knob 的同名 key）。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// settings.json 顶层字段（任意 TOML 值，含嵌套表）。
    /// `env` / `permissions` 由 donn 另行管理，出现在这里会被忽略。
    /// shared 模式不写 profile settings.json（Claude 读的是 `~/.claude`），而是通过
    /// `--settings` overlay 文件把顶层字段带进 session。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, serde_json::Value>,
}

fn knobs_is_default(k: &Knobs) -> bool {
    *k == Knobs::default()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    /// wrapper 的落盘目录；省略时用平台默认（[`DonnHome::default_bin_dir`]）。支持 `~/` 前缀。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin_dir: Option<String>,
    /// claude 二进制的显式路径；省略时用 `which("claude")`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_bin: Option<String>,
    /// 注入所有 profile 的用户全局默认。
    #[serde(skip_serializing_if = "defaults_is_empty")]
    pub defaults: Defaults,
}

fn defaults_is_empty(d: &Defaults) -> bool {
    *d == Defaults::default()
}

/// `GlobalConfig` 拥有的顶层 key；save 只替换这些 key。新增 struct 字段必须同步登记。
const OWNED_TOP_KEYS: [&str; 3] = ["bin_dir", "claude_bin", "defaults"];

impl GlobalConfig {
    pub fn load(home: &DonnHome) -> Result<Self> {
        let path = home.config_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(io_ctx(format!("failed to read {}", path.display())))?;
        let config: Self = toml_edit::de::from_str(&text).map_err(|e| Error::InvalidToml {
            path: path.clone(),
            message: e.to_string(),
        })?;
        // 非法取值在读取时就报错，不会静默按默认渲染，再把已经落盘的 key 清掉
        config
            .defaults
            .knobs
            .validate()
            .map_err(|e| Error::InvalidToml {
                path,
                message: e.to_string(),
            })?;
        Ok(config)
    }

    /// 以现有文件为底，只替换 donn 拥有的顶层 key（`bin_dir` / `claude_bin` / `defaults`），
    /// 用户的其它 key 和正文注释保留；模板头刷新到当前版本；内容没变化就不落盘。
    pub fn save(&self, home: &DonnHome) -> Result<()> {
        let path = home.config_file();
        let rendered = toml_edit::ser::to_string_pretty(self)
            .map_err(|e| Error::Internal(format!("failed to serialize config.toml: {e}")))?;
        let existing = fsx::read_if_exists(&path)?.unwrap_or_default();
        // 先摘掉模板头再合并：toml_edit 会把新表插到文件末尾或头部注释之外，
        // 合并后再摘就会漏掉这层头，写出第二份。
        let body = body_without_doc_header(&existing);
        let merged = fsx::merge_toml_owned(&path, &body, &rendered, &OWNED_TOP_KEYS)?;
        let text = format!("{CONFIG_DOC}\n{merged}");
        if text != existing {
            fsx::write_atomic(&path, text.as_bytes())?;
        }
        Ok(())
    }

    /// 解析 `bin_dir`（展开 `~`），省略时取平台默认。
    pub fn resolve_bin_dir(&self, home: &DonnHome) -> PathBuf {
        match &self.bin_dir {
            Some(dir) => expand_tilde(dir, home),
            None => home.default_bin_dir(),
        }
    }

    pub fn resolve_claude_bin(&self, home: &DonnHome) -> Option<PathBuf> {
        self.claude_bin.as_ref().map(|p| expand_tilde(p, home))
    }
}

fn expand_tilde(raw: &str, home: &DonnHome) -> PathBuf {
    if raw == "~" {
        return home.user_home().to_path_buf();
    }
    raw.strip_prefix("~/")
        .or_else(|| raw.strip_prefix("~\\"))
        .map_or_else(|| PathBuf::from(raw), |rest| home.user_home().join(rest))
}
