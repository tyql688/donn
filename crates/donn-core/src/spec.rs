//! ProfileSpec：`profile.toml`（schema v2），donn 意图的唯一来源。
//!
//! 一个 profile 的全部 donn 侧状态都在这里：用哪个 preset、覆盖了什么、
//! 上次生成了哪些 key（footprint）。settings.json 由 spec 渲染生成，意图从不反向
//! 从 settings.json 推断。这里绝不存 API key。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::keys::SlotMap;
use crate::preset::AuthMode;

pub const SCHEMA_VERSION: u32 = 2;

/// isolation。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// 独立 CLAUDE_CONFIG_DIR：session 和配置完全隔离（默认）。
    #[default]
    Full,
    /// 共享主配置：不设置 CLAUDE_CONFIG_DIR，session、全局 CLAUDE.md、agents 都用 `~/.claude` 的。
    /// provider env 在启动时注入，settings 类配置走 `--settings` overlay；donn 依然只写自己的目录。
    Shared,
}

/// 上次 render 实际写入的 key 名清单，用作清理 diff 的输入：donn 只增删这里声明过的 key，
/// 其余字段永不触碰。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Footprint {
    /// settings.json 顶层 `env` 对象下的 key。
    pub settings_env: Vec<String>,
    /// settings.json 顶层字段（settings 类 knob 和 `[defaults.settings]`）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings_top: Vec<String>,
    /// `.claude.json` 的顶层 key。
    pub claude_json: Vec<String>,
    /// `permissions` 对象内由 donn 拥有的子 key（例如 defaultMode）。
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
    /// 用户自己定义的 model 窗口：model id 对应上下文窗口（token）。给 preset 候选里没有的
    /// 自定义 model 用；Claude Code 不认识的 id 如果不给窗口，就按它猜的大小压缩。窗口是 model 的属性，
    /// 跟 id 走：同名条目也会压过 preset 候选自带的数字。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub model_windows: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// 认证方式记录：secret 存在 settings.json 的哪个 env key（绝不存 secret 本身）。
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

/// ProfileSpec 拥有的顶层 key。save 只替换这些 key，其余顶层 key（未来 schema 的字段）
/// 和文件里的注释原样保留。新增 struct 字段必须同步登记在这里。
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
    /// 按目录名加载，并拒绝 `name` 字段与目录名不一致的 spec。
    /// 写路径用 `spec.name` 决定落盘位置，这条校验防止手改的 `name` 把配置写到别的 profile。
    pub fn load_named(path: &Path, expected_name: &str) -> Result<Self> {
        let spec = Self::load(path)?;
        if spec.name != expected_name {
            return Err(Error::InvalidToml {
                path: path.to_path_buf(),
                message: format!(
                    "name '{}' does not match profile directory '{}'",
                    spec.name, expected_name
                ),
            });
        }
        Ok(spec)
    }

    fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(io_ctx(format!("failed to read {}", path.display())))?;
        let mut spec: Self = toml_edit::de::from_str(&text).map_err(|e| Error::InvalidToml {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        // 与 UI 写路径一致：空串表示「清除覆盖」，载入后也归一成未设置。
        spec.intent.base_url = spec.intent.base_url.filter(|url| !url.is_empty());
        // 版本更高的 schema 拒绝加载：静默按 v2 理解再回写会丢新版的语义。
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
        // 以现有文件为底，非 donn 拥有的顶层 key 和注释原样保留
        let text = match fsx::read_if_exists(path)? {
            Some(existing) => fsx::merge_toml_owned(path, &existing, &rendered, &OWNED_TOP_KEYS)?,
            None => rendered,
        };
        fsx::write_atomic(path, text.as_bytes())
    }
}
