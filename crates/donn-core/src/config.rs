//! 全局 `~/.donn/config.toml`（全部字段可省略）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::home::DonnHome;
use crate::knobs::Knobs;

/// config.toml 的注释文档头；保存时恒重新附带，首次运行也用同一份模板。
pub const CONFIG_DOC: &str = r#"# ~/.donn/config.toml —— donn 全局配置。本注释即文档，保存与启动时自动刷新。
# 全部字段可省略，缺失 = 跟随默认。改完运行 donn（或任一 profile 命令）即生效。
# 优先级：donn 默认 < 渠道 preset < 本文件 < profile 单独设置。
#
# bin_dir = "~/.local/bin"        # 别名命令落盘目录；省略 = 平台默认
# claude_bin = "/path/to/claude"  # claude 二进制路径；省略 = PATH 查找
#
# [defaults.knobs]           # 类型化旋钮（类型写错加载即报错）
# env 前缀键写进 settings.json env（两种隔离都生效）；顶层键 full 模式直接写入，
# shared 模式由 sync 生成 shared-settings.json overlay、启动时经 --settings 带进会话
# （不写 ~/.claude）。
# agent_teams = true         # 实验特性 agent teams（默认开）
# tool_search = true         # MCP tool search。不写 = 交给 Claude Code：官方主机按需加载工具，第三方端点一次全加载；true = 强制按需加载（发 beta 头，代理得支持 tool_reference）；false = 一次全加载
# permission_mode = "bypassPermissions" # 默认权限模式 default/acceptEdits/plan/dontAsk/auto/bypassPermissions
#                            # （默认 bypassPermissions=权限直通；default = 交还 claude code 管理）
# hide_attribution = false   # 隐藏 Claude 署名：commit/PR 文本 + session 链接（默认关）
# effort = "medium"          # 默认思考强度 low/medium/high/xhigh；省略 = 跟随 claude code
# max_effort = "max"         # 思考强度上限 low/medium/high/xhigh/max；省略 = 不设上限
# disable_connectors = false # 停用 claude.ai connectors（默认关）
# thinking = true            # extended thinking（默认开）；false = 为所有会话关闭
# api_timeout_ms = 600000    # 请求超时毫秒，仅对配了端点的渠道注入（默认 600000；上限 2147483647，超出请求立即失败）
# disable_nonessential_traffic = false # 禁遥测/更新检查等非必要流量，仅配了端点的渠道（默认关）
# language = "中文"           # 回复语言，任意语言名；省略 = 跟随会话
# disable_experimental_betas = false # 去掉 anthropic-beta 头与 beta 字段：代理网关报 Unexpected anthropic-beta 时开
# disable_unknown_model_window_enforcement = false # 不认识的模型 id 不按猜测窗口提前压缩（默认关；套餐已给 max_context 时不需要）
# max_output_tokens = 64000  # 单次回复最大输出 token；省略 = 模型默认（未知 id 为 32000）
# auto_compact = true        # 上下文接近上限时自动压缩（默认开）
# model_fallback = true      # 模型不可用/被安全分类器拦截时自动切换模型（默认开）
# subagent_model_force = false # 强制 subagent/teammate 都用 CLAUDE_CODE_SUBAGENT_MODEL（默认关）
#
# [defaults.env]             # 自由 env：注入每个 profile 的 settings.json env
# MAX_THINKING_TOKENS = "31999"
#
# [defaults.settings]        # 自由 settings.json 顶层字段（支持嵌套表；env/permissions 保留键无效）
# spinnerTipsEnabled = false
"#;

/// 首次运行生成带注释的空模板；已有文件则把开头的模板注释头刷新到当前版本，
/// 正文字节原样不动。头已是最新时不落盘。
pub fn ensure_config_doc(home: &DonnHome) -> Result<()> {
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

/// 文件开头连续的注释/空行块是 donn 的模板头，替换为 [`CONFIG_DOC`]；其余原样保留。
fn refresh_doc_header(existing: &str) -> String {
    let body: String = existing
        .split_inclusive('\n')
        .skip_while(|line| {
            let line = line.trim_end_matches(['\n', '\r']);
            line.is_empty() || line.starts_with('#')
        })
        .collect();
    if body.is_empty() {
        CONFIG_DOC.to_string()
    } else {
        format!("{CONFIG_DOC}\n{body}")
    }
}

/// 用户全局默认：注入每个 profile 的个人偏好，一处配置全体生效。
/// 旋钮（类型化开关/选项）+ 自由 env/settings 补充。在渲染层序里的位置见 [`crate::render::render`]。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    /// 全局旋钮（None 字段 = 跟随默认）。
    #[serde(skip_serializing_if = "knobs_is_default")]
    pub knobs: Knobs,
    /// 注入 settings.json `env` 的键值（自由补充，覆盖旋钮同名键）。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// settings.json 顶层字段（任意 TOML 值，含嵌套表）。
    /// `env` / `permissions` 由 donn 另行管理，出现在这里会被忽略。
    /// shared 模式不写 profile settings.json（Claude 读的是 ~/.claude），而是经
    /// `--settings` overlay 文件把顶层字段带进会话。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, serde_json::Value>,
}

fn knobs_is_default(k: &Knobs) -> bool {
    *k == Knobs::default()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    /// wrapper 落盘目录；省略 → 平台默认（[`DonnHome::default_bin_dir`]）。支持 `~/` 前缀。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin_dir: Option<String>,
    /// claude 二进制显式路径；省略 → which("claude")。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_bin: Option<String>,
    /// 注入所有 profile 的用户全局默认。
    #[serde(skip_serializing_if = "defaults_is_empty")]
    pub defaults: Defaults,
}

fn defaults_is_empty(d: &Defaults) -> bool {
    *d == Defaults::default()
}

/// `GlobalConfig` 拥有的顶层键；save 只替换这些键。新增 struct 字段必须同步登记。
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
        // 非法取值在读取时就报错，而不是静默按默认渲染再把已落盘的键清掉
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

    /// 以现有文件为底只替换 donn 拥有的顶层键（`bin_dir` / `claude_bin` / `defaults`），
    /// 用户的其它键与正文注释保留；模板头刷新到当前版本；内容不变则不落盘。
    pub fn save(&self, home: &DonnHome) -> Result<()> {
        let path = home.config_file();
        let rendered = toml_edit::ser::to_string_pretty(self)
            .map_err(|e| Error::Internal(format!("failed to serialize config.toml: {e}")))?;
        let existing = fsx::read_if_exists(&path)?.unwrap_or_default();
        let merged = fsx::merge_toml_owned(&path, &existing, &rendered, &OWNED_TOP_KEYS)?;
        let text = refresh_doc_header(&merged);
        if text != existing {
            fsx::write_atomic(&path, text.as_bytes())?;
        }
        Ok(())
    }

    /// 解析 bin_dir（展开 `~`），省略时取平台默认。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knobs::{BOOL_KNOBS, VALUE_KNOBS};
    use tempfile::TempDir;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        let cfg = GlobalConfig::load(&home).unwrap();
        assert!(cfg.bin_dir.is_none());
        assert_eq!(cfg.resolve_bin_dir(&home), home.default_bin_dir());
    }

    #[test]
    fn tilde_expansion_and_roundtrip() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        let cfg = GlobalConfig {
            bin_dir: Some("~/mybin".into()),
            claude_bin: Some("/usr/local/bin/claude".into()),
            defaults: Defaults::default(),
        };
        cfg.save(&home).unwrap();
        let loaded = GlobalConfig::load(&home).unwrap();
        assert_eq!(loaded.resolve_bin_dir(&home), dir.path().join("mybin"));
        assert_eq!(
            loaded.resolve_claude_bin(&home),
            Some(PathBuf::from("/usr/local/bin/claude"))
        );
        assert_eq!(expand_tilde("~", &home), dir.path());
        assert_eq!(expand_tilde("~\\other", &home), dir.path().join("other"));
        assert_eq!(
            expand_tilde("~other/bin", &home),
            PathBuf::from("~other/bin")
        );
    }

    #[test]
    fn knob_defaults_are_locked() {
        // 默认值的唯一定义处：改这里 = 改产品行为，测试必须跟着改
        let f = Knobs::default();
        assert!(f.agent_teams_on());
        assert_eq!(f.permission_mode(), crate::keys::BYPASS_PERMISSIONS);
        assert!(!f.hide_attribution_on());
        assert_eq!(f.api_timeout(), 600_000);
        assert!(!f.disable_nonessential_traffic_on());
        for knob in BOOL_KNOBS {
            assert_eq!(f.flag(knob), knob.claude_default, "{}", knob.field);
            assert!(!f.is_explicit(knob.field));
        }
        for knob in VALUE_KNOBS {
            assert!(f.value(knob).is_none(), "{}", knob.field);
        }
        let fields: std::collections::BTreeSet<&str> = BOOL_KNOBS
            .iter()
            .map(|k| k.field)
            .chain(VALUE_KNOBS.iter().map(|k| k.field))
            .collect();
        assert_eq!(
            fields.len(),
            BOOL_KNOBS.len() + VALUE_KNOBS.len(),
            "duplicate knob field"
        );
    }

    #[test]
    fn config_document_mentions_every_knob() {
        for field in [
            "agent_teams",
            "tool_search",
            "permission_mode",
            "hide_attribution",
            "api_timeout_ms",
            "disable_nonessential_traffic",
        ]
        .into_iter()
        .chain(BOOL_KNOBS.iter().map(|k| k.field))
        .chain(VALUE_KNOBS.iter().map(|k| k.field))
        {
            assert!(CONFIG_DOC.contains(&format!("# {field} =")), "{field}");
        }
    }

    #[test]
    fn global_settings_doc_lists_every_knob() {
        let doc = include_str!("../../../docs/GLOBAL-SETTINGS.md");
        for field in [
            "agent_teams",
            "tool_search",
            "permission_mode",
            "hide_attribution",
            "api_timeout_ms",
            "disable_nonessential_traffic",
        ]
        .into_iter()
        .chain(BOOL_KNOBS.iter().map(|k| k.field))
        .chain(VALUE_KNOBS.iter().map(|k| k.field))
        {
            assert!(
                doc.contains(&format!("| `{field}` |")),
                "docs/GLOBAL-SETTINGS.md lacks {field}"
            );
        }
    }

    #[test]
    fn invalid_knob_values_fail_at_load_instead_of_silently_rendering_defaults() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        for body in [
            "permission_mode = \"bogus\"",
            "auto_compact = \"yes\"",
            "effort = \"ultra\"",
            "max_output_tokens = -1",
            "language = 3",
            "api_timeout_ms = 9999999999",
        ] {
            std::fs::write(home.config_file(), format!("[defaults.knobs]\n{body}\n")).unwrap();
            let err = GlobalConfig::load(&home).unwrap_err().to_string();
            assert!(err.contains("config.toml"), "{body}: {err}");
        }
        std::fs::write(
            home.config_file(),
            "[defaults.knobs]\npermission_mode = \"plan\"\nauto_compact = false\neffort = \"xhigh\"\nfuture_knob = \"anything\"\n",
        )
        .unwrap();
        GlobalConfig::load(&home).unwrap();
        // 上限边界值合法
        std::fs::write(
            home.config_file(),
            "[defaults.knobs]\napi_timeout_ms = 2147483647\n",
        )
        .unwrap();
        GlobalConfig::load(&home).unwrap();
    }

    #[test]
    fn save_keeps_user_keys_and_comments_and_is_a_noop_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.config_file(),
            "# stale header\n\nfuture_key = true # why I set this\n\n[future_table]\nx = 1\n\n[defaults.knobs]\nauto_compact = false\n",
        )
        .unwrap();
        let mut cfg = GlobalConfig::load(&home).unwrap();
        cfg.defaults
            .knobs
            .set_extra("language", Some(serde_json::json!("中文")));
        cfg.save(&home).unwrap();
        let text = std::fs::read_to_string(home.config_file()).unwrap();
        assert!(text.starts_with(CONFIG_DOC), "{text}");
        assert!(!text.contains("stale header"), "{text}");
        assert!(
            text.contains("future_key = true # why I set this"),
            "{text}"
        );
        assert!(text.contains("[future_table]"), "{text}");
        assert!(
            text.contains("auto_compact = false") && text.contains("language = \"中文\""),
            "{text}"
        );

        let before = std::fs::metadata(home.config_file())
            .unwrap()
            .modified()
            .unwrap();
        GlobalConfig::load(&home).unwrap().save(&home).unwrap();
        assert_eq!(
            std::fs::metadata(home.config_file())
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
    }

    #[test]
    fn startup_refreshes_only_the_template_header() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        let body = "bin_dir = \"~/bin\"\n\n[defaults.knobs]\n# my note\nauto_compact = false\n";
        std::fs::write(
            home.config_file(),
            format!("# old header line 1\n# always_thinking = false\n\n{body}"),
        )
        .unwrap();
        ensure_config_doc(&home).unwrap();
        let text = std::fs::read_to_string(home.config_file()).unwrap();
        assert_eq!(text, format!("{CONFIG_DOC}\n{body}"), "{text}");
        assert!(!text.contains("old header"));
        // 第二次是 no-op：字节不变
        let before = std::fs::metadata(home.config_file())
            .unwrap()
            .modified()
            .unwrap();
        ensure_config_doc(&home).unwrap();
        assert_eq!(std::fs::read_to_string(home.config_file()).unwrap(), text);
        assert_eq!(
            std::fs::metadata(home.config_file())
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        // 只有注释的文件 = 空模板
        std::fs::write(home.config_file(), "# stale\n").unwrap();
        ensure_config_doc(&home).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.config_file()).unwrap(),
            CONFIG_DOC
        );
    }

    #[test]
    fn simple_knobs_and_unknown_keys_roundtrip() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.config_file(),
            "[defaults.knobs]\nauto_compact = false\nlanguage = \"中文\"\nmax_output_tokens = 64000\nfuture_knob = 3\n",
        )
        .unwrap();
        let cfg = GlobalConfig::load(&home).unwrap();
        let knobs = &cfg.defaults.knobs;
        let auto_compact = BOOL_KNOBS
            .iter()
            .find(|k| k.field == "auto_compact")
            .unwrap();
        assert!(!knobs.flag(auto_compact));
        assert!(knobs.is_explicit("auto_compact"));
        assert_eq!(
            knobs.extra.get("language"),
            Some(&serde_json::Value::String("中文".into()))
        );
        assert_eq!(
            knobs.extra.get("max_output_tokens"),
            Some(&serde_json::json!(64000))
        );
        cfg.save(&home).unwrap();
        let text = std::fs::read_to_string(home.config_file()).unwrap();
        assert!(text.contains("auto_compact = false"), "{text}");
        assert!(
            text.contains("future_knob = 3"),
            "unknown knobs survive a save: {text}"
        );
        assert_eq!(GlobalConfig::load(&home).unwrap().defaults, cfg.defaults);
    }

    #[test]
    fn save_and_first_run_template_share_the_document_header() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        ensure_config_doc(&home).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.config_file()).unwrap(),
            CONFIG_DOC
        );
        assert_eq!(
            GlobalConfig::load(&home).unwrap().defaults,
            Defaults::default()
        );
        GlobalConfig::default().save(&home).unwrap();
        assert!(
            std::fs::read_to_string(home.config_file())
                .unwrap()
                .starts_with(CONFIG_DOC)
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_propagates_read_errors_instead_of_dropping_unreadable_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.config_file(),
            "# my note\nfuture_key = true\n\n[defaults.knobs]\nauto_compact = false\n",
        )
        .unwrap();
        std::fs::set_permissions(home.config_file(), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        if std::fs::read(home.config_file()).is_ok() {
            return; // root 读得了 0o000，测不出
        }
        let mut cfg = GlobalConfig::default();
        cfg.defaults
            .knobs
            .set_extra("language", Some(serde_json::json!("中文")));
        assert!(cfg.save(&home).is_err(), "读不了的文件不得当新建重写");
        std::fs::set_permissions(home.config_file(), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        let text = std::fs::read_to_string(home.config_file()).unwrap();
        assert!(text.contains("future_key = true"), "{text}");
        assert!(text.contains("# my note"), "{text}");
    }

    #[test]
    fn defaults_roundtrip_with_nested_settings() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.config_file(),
            r#"
[defaults.env]
CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1"

[defaults.settings]
spinnerTipsEnabled = false
effortLevel = "medium"

[defaults.settings.attribution]
commit = ""
"#,
        )
        .unwrap();
        let cfg = GlobalConfig::load(&home).unwrap();
        assert_eq!(
            cfg.defaults.env.get("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
            Some(&"1".to_string())
        );
        assert_eq!(
            cfg.defaults.settings.get("spinnerTipsEnabled"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            cfg.defaults.settings.get("attribution"),
            Some(&serde_json::json!({"commit": ""}))
        );
        // 保存往返不丢
        cfg.save(&home).unwrap();
        let again = GlobalConfig::load(&home).unwrap();
        assert_eq!(again.defaults, cfg.defaults);
    }
}
