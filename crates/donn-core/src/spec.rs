//! ProfileSpec：`profile.toml`（schema v2）—— donn 意图的唯一来源。
//!
//! 一个 profile 的全部 donn 侧状态都在这里：用哪个 preset、覆盖了什么、
//! 上次生成了哪些键（footprint）。settings.json 由 spec 渲染生成，意图从不反向
//! 从 settings.json 推断。**绝不在此文件存 API key**。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::keys::SlotMap;
use crate::preset::AuthMode;

pub const SCHEMA_VERSION: u32 = 2;

/// 隔离模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// 独立 CLAUDE_CONFIG_DIR：session/配置完全隔离（默认）。
    #[default]
    Full,
    /// 共享主配置：不设 CLAUDE_CONFIG_DIR，会话、全局 CLAUDE.md、agents 都用 ~/.claude 的。
    /// 渠道 env 启动时注入，settings 类配置走 `--settings` overlay；donn 依然只写自己目录。
    Shared,
}

/// 上次 render 实际写入的键名清单——清理 diff 的输入：donn 只增删这里声明过的键，
/// 其余字段永不触碰。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Footprint {
    /// settings.json 顶层 `env` 对象下的键。
    pub settings_env: Vec<String>,
    /// settings.json 顶层字段（settings 类旋钮与 `[defaults.settings]`）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings_top: Vec<String>,
    /// `.claude.json` 的顶层键。
    pub claude_json: Vec<String>,
    /// `permissions` 对象内由 donn 拥有的子键（如 defaultMode）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub permissions_top: Vec<String>,
}

/// 用户对 preset 的覆盖意图。只存差异：空 = 完全跟随 preset。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Intent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "SlotMap::is_empty")]
    pub models: SlotMap,
    /// 用户自己定义的模型窗口：模型 id → 上下文窗口（token）。给 preset 候选里没有的
    /// 自定义模型用——Claude Code 不认识的 id 不给窗口就按它猜的大小压缩。窗口是模型的属性，
    /// 跟 id 走：同名条目也压过 preset 候选自带的数字。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub model_windows: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// 认证方式记录：secret 存在 settings.json 的哪个 env 键（绝不存 secret 本身）。
/// 每次 sync 跟随 preset 的 `auth_mode`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthSpec {
    pub mode: AuthMode,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WrapperSpec {
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSpec {
    pub schema_version: u32,
    pub name: String,
    /// preset key；custom 时为 "custom"。
    pub preset: String,
    pub created_at: String,
    pub updated_at: String,
    pub auth: AuthSpec,
    pub isolation: Isolation,
    pub intent: Intent,
    pub wrapper: WrapperSpec,
    pub footprint: Footprint,
}

impl Default for ProfileSpec {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: String::new(),
            preset: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            auth: AuthSpec::default(),
            isolation: Isolation::default(),
            intent: Intent::default(),
            wrapper: WrapperSpec::default(),
            footprint: Footprint::default(),
        }
    }
}

/// ProfileSpec 拥有的顶层键。save 只替换这些键，其余顶层键（未来 schema 的字段）
/// 与文件里的注释原样保留。新增 struct 字段必须同步登记在此。
const OWNED_TOP_KEYS: [&str; 10] = [
    "schema_version",
    "name",
    "preset",
    "created_at",
    "updated_at",
    "auth",
    "isolation",
    "intent",
    "wrapper",
    "footprint",
];

impl ProfileSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(io_ctx(format!("failed to read {}", path.display())))?;
        let mut spec: Self = toml_edit::de::from_str(&text).map_err(|e| Error::InvalidToml {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        // 与 UI 写路径一致：空串表示“清除覆盖”，载入后也归一成未设置。
        spec.intent.base_url = spec.intent.base_url.filter(|url| !url.is_empty());
        // 更新的 schema 拒绝加载：静默按 v2 理解再回写会丢新版语义
        if spec.schema_version > SCHEMA_VERSION {
            return Err(Error::InvalidToml {
                path: path.to_path_buf(),
                message: format!(
                    "schema_version {} is newer than supported {SCHEMA_VERSION}; upgrade donn",
                    spec.schema_version
                ),
            });
        }
        Ok(spec)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let rendered = toml_edit::ser::to_string_pretty(self)
            .map_err(|e| Error::Internal(format!("failed to serialize profile.toml: {e}")))?;
        // 现有文件为底：非 donn 拥有的顶层键与注释原样保留
        let text = match fsx::read_if_exists(path)? {
            Some(existing) => fsx::merge_toml_owned(path, &existing, &rendered, &OWNED_TOP_KEYS)?,
            None => rendered,
        };
        fsx::write_atomic(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::ModelSlot;
    use tempfile::TempDir;

    fn sample() -> ProfileSpec {
        let mut models = SlotMap::default();
        models.set(ModelSlot::Sonnet, Some("glm-5.1".into()));
        ProfileSpec {
            name: "zai".into(),
            preset: "zai".into(),
            created_at: "2026-07-16T10:00:00Z".into(),
            updated_at: "2026-07-16T10:00:00Z".into(),
            auth: AuthSpec {
                mode: AuthMode::AuthToken,
            },
            intent: Intent {
                base_url: Some("https://my-relay.example/api".into()),
                models,
                model_windows: BTreeMap::new(),
                env: BTreeMap::from([("API_TIMEOUT_MS".into(), "600000".into())]),
            },
            wrapper: WrapperSpec {
                aliases: vec!["zai".into(), "z".into()],
            },
            footprint: Footprint {
                settings_env: vec!["ANTHROPIC_BASE_URL".into(), "ANTHROPIC_AUTH_TOKEN".into()],
                settings_top: vec!["spinnerTipsEnabled".into()],
                claude_json: vec!["hasCompletedOnboarding".into()],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn roundtrip_and_never_contains_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        let spec = sample();
        spec.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.to_lowercase().contains("sk-"),
            "profile.toml must never contain keys: {text}"
        );
        let loaded = ProfileSpec::load(&path).unwrap();
        assert_eq!(loaded.name, "zai");
        assert_eq!(loaded.auth.mode, AuthMode::AuthToken);
        assert_eq!(loaded.intent, spec.intent);
        assert_eq!(loaded.footprint, spec.footprint);
        assert_eq!(loaded.wrapper.aliases, vec!["zai", "z"]);
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn minimal_and_unknown_fields_tolerated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(
            &path,
            "schema_version = 2\nname = \"a\"\npreset = \"zai\"\nfuture_field = true\n",
        )
        .unwrap();
        let loaded = ProfileSpec::load(&path).unwrap();
        assert_eq!(loaded.name, "a");
        assert!(loaded.intent.base_url.is_none());
        assert!(loaded.footprint.settings_env.is_empty());

        std::fs::write(
            &path,
            "schema_version = 2\nname = \"a\"\npreset = \"zai\"\n[footprint]\npermissions_deny = [\"legacy\"]\n",
        )
        .unwrap();
        let loaded = ProfileSpec::load(&path).unwrap();
        assert!(loaded.footprint.permissions_top.is_empty());
    }

    #[test]
    fn empty_base_url_is_normalized_to_unset() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(
            &path,
            "schema_version = 2\nname = \"a\"\npreset = \"custom\"\n[intent]\nbase_url = \"\"\n",
        )
        .unwrap();
        let mut spec = ProfileSpec::load(&path).unwrap();
        assert!(spec.intent.base_url.is_none());
        spec.updated_at = "now".into();
        spec.save(&path).unwrap();
        assert!(!std::fs::read_to_string(path).unwrap().contains("base_url"));
    }

    #[test]
    fn owned_top_keys_cover_all_struct_fields() {
        // 新增 ProfileSpec 字段而忘登记 OWNED_TOP_KEYS 时此测试失败：
        // 否则该字段被 save 当「未知键」从旧文件复活，删除意图失效
        let text = toml_edit::ser::to_string_pretty(&sample()).unwrap();
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        for (key, _) in doc.iter() {
            assert!(OWNED_TOP_KEYS.contains(&key), "unregistered field: {key}");
        }
    }

    #[test]
    fn newer_schema_version_is_rejected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(
            &path,
            "schema_version = 99\nname = \"a\"\npreset = \"zai\"\n",
        )
        .unwrap();
        let err = ProfileSpec::load(&path).unwrap_err();
        assert!(err.to_string().contains("upgrade donn"), "{err}");
    }

    #[test]
    fn save_preserves_unknown_top_level_keys() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(
            &path,
            "schema_version = 2\nname = \"a\"\npreset = \"zai\"\nfuture_field = true\n\n[future_table]\nx = 1\n",
        )
        .unwrap();
        let mut spec = ProfileSpec::load(&path).unwrap();
        spec.updated_at = "2026-07-17T00:00:00Z".into();
        spec.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("future_field = true"), "{text}");
        assert!(text.contains("[future_table]"), "{text}");
        // donn 拥有的键以本次渲染为准，不从旧文件复活
        let reloaded = ProfileSpec::load(&path).unwrap();
        assert_eq!(reloaded.updated_at, "2026-07-17T00:00:00Z");
    }

    #[test]
    fn save_refuses_to_replace_a_syntactically_broken_existing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(&path, "not [valid toml").unwrap();
        let spec = sample();
        assert!(spec.save(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "not [valid toml");
    }

    #[cfg(unix)]
    #[test]
    fn save_propagates_read_errors_instead_of_dropping_unreadable_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profile.toml");
        std::fs::write(
            &path,
            "schema_version = 2\nname = \"a\"\npreset = \"zai\"\nfuture_key = true\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            return; // root 读得了 0o000，测不出
        }
        let spec = sample();
        assert!(spec.save(&path).is_err(), "读不了的文件不得当新建重写");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("future_key = true"),
            "原文件内容必须原样保留"
        );
    }

    #[test]
    fn empty_intent_sections_are_omitted() {
        let spec = ProfileSpec {
            name: "min".into(),
            preset: "official".into(),
            ..Default::default()
        };
        let text = toml_edit::ser::to_string_pretty(&spec).unwrap();
        assert!(!text.contains("base_url"), "{text}");
        assert!(!text.contains("[intent.models]"), "{text}");
    }
}
