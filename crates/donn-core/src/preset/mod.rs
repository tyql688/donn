//! Provider preset：纯 TOML 数据，新增 provider 不用改代码。
//! 加载顺序：内嵌 preset → `~/.donn/presets.d/` 覆盖（同 key 覆盖）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::home::DonnHome;
use crate::keys::SlotMap;

/// 认证模式：secret 写进 settings.json 的哪个 env key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    #[default]
    AuthToken,
    None,
}

impl AuthMode {
    pub fn label(self) -> &'static str {
        match self {
            AuthMode::ApiKey => "api_key (ANTHROPIC_API_KEY)",
            AuthMode::AuthToken => "auth_token (ANTHROPIC_AUTH_TOKEN)",
            AuthMode::None => "none (OAuth login)",
        }
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, AuthMode::None)
    }
}

/// Claude Code 自己认识的 model id：去掉 gateway 前缀（`anthropic/`、`us.anthropic.`）后以
/// `claude-` 开头。这类 id 不需要，也不吃 `max_context`。
pub fn is_claude_model(id: &str) -> bool {
    let name = id.rsplit('/').next().unwrap_or(id);
    name.split('.').any(|part| part.starts_with("claude-"))
}

/// 兼容性 flags：在 preset 数据里表达，逻辑不特判 provider。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetFlags {
    pub auth_token_also_sets_api_key: bool,
}

/// model 候选项（数据驱动）。
///
/// UI 候选 = 这个列表 ∪ slot 默认 id；也可以手填任意 id。
/// package 不进 intent：`render()` 按生效的 sonnet id 匹配这个列表，再注入
/// [`ModelChoice::package_env`]；UI 只改 model slot。用户自己在 `intent.env` 里写的
/// 同名 key 优先于 package。
///
/// ```toml
/// [[preset.model_choices]]
/// id = "k3[1m]"
/// label = "K3 · 1M"
/// max_context = 1048576
/// auto_compact = 1000000   # 省略 = max_context（写入时封顶到 Claude Code 接受的 1000000）
/// pin_subagent = true      # 省略且有 max 时为 true
/// env = { FOO = "1" }      # 可选；后写覆盖结构化 key
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelChoice {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compact: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin_subagent: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl ModelChoice {
    pub fn display(&self) -> &str {
        self.label.as_deref().unwrap_or(self.id.as_str())
    }

    /// render 注入的配套 env（结构化字段加 freeform）。不写 intent。
    pub fn package_env(&self) -> BTreeMap<String, String> {
        use crate::keys::{AUTO_COMPACT, AUTO_COMPACT_MAX, MAX_CONTEXT, SUBAGENT_MODEL};
        let mut out = BTreeMap::new();
        if let Some(max) = self.max_context {
            out.insert(MAX_CONTEXT.to_string(), max.to_string());
        }
        if let Some(compact) = self.auto_compact.or(self.max_context) {
            let compact = compact.min(AUTO_COMPACT_MAX);
            out.insert(AUTO_COMPACT.to_string(), compact.to_string());
        }
        let pin = self.pin_subagent.unwrap_or(self.max_context.is_some());
        if pin && !self.id.is_empty() {
            out.insert(SUBAGENT_MODEL.to_string(), self.id.clone());
        }
        // freeform env 放最后，作者可以覆盖结构化默认值
        for (k, v) in &self.env {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub key: String,
    pub label: String,
    pub description: String,
    pub base_url: Option<String>,
    pub auth_mode: AuthMode,
    pub key_url: Option<String>,
    pub models: SlotMap,
    pub env: BTreeMap<String, String>,
    pub flags: PresetFlags,
    /// 额外 model 候选（`[[preset.model_choices]]`）：带 label 或配套 env 时用。
    /// 即使为空，UI 仍会把 `[preset.models]` 里出现过的 id 收进下拉。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_choices: Vec<ModelChoice>,
    /// 加载来源：`builtin` 或 `user`（presets.d）。运行时填充，不写进 TOML。
    #[serde(skip)]
    pub source: PresetSource,
}

impl Preset {
    /// 按 model id 查找 package：先精确匹配，再按 ASCII 大小写不敏感匹配。
    pub fn choice_for_model(&self, id: &str) -> Option<&ModelChoice> {
        if id.is_empty() {
            return None;
        }
        self.model_choices.iter().find(|c| c.id == id).or_else(|| {
            self.model_choices
                .iter()
                .find(|c| c.id.eq_ignore_ascii_case(id))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresetSource {
    #[default]
    Builtin,
    User,
}

impl PresetSource {
    pub fn label(self) -> &'static str {
        match self {
            PresetSource::Builtin => "builtin",
            PresetSource::User => "user",
        }
    }
}

#[derive(Debug, Deserialize)]
struct PresetFile {
    preset: Preset,
}

/// presets.d 里解析失败的文件，会被跳过并由 doctor 报告。
#[derive(Debug, Clone)]
pub struct PresetLoadError {
    pub file: PathBuf,
    pub message: String,
}

#[derive(Debug)]
pub struct PresetCatalog {
    presets: Vec<Preset>,
    pub load_errors: Vec<PresetLoadError>,
}

// 内置 preset：`presets/<key>.toml`，文件名就是 key。清单由 `build.rs` 从目录生成。
include!(concat!(env!("OUT_DIR"), "/builtin_presets.rs"));

impl PresetCatalog {
    /// 内嵌 preset → 用户目录覆盖。内嵌解析失败属于打包错误，直接 panic。
    pub fn load(home: &DonnHome) -> Self {
        #[allow(clippy::expect_used)] // 打包不变量：内嵌 TOML 必须可解析
        let mut presets: Vec<Preset> = BUILTIN_PRESETS
            .iter()
            .map(|(key, text)| {
                parse_preset(text, Path::new(key)).expect("builtin preset must parse")
            })
            .collect();
        let mut load_errors = Vec::new();

        let dir = home.presets_dir();
        if dir.is_dir() {
            let mut files: Vec<_> = match std::fs::read_dir(&dir) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|name| name != ".toml")
                            && p.extension().is_some_and(|ext| ext == "toml")
                    })
                    .collect(),
                Err(error) => {
                    load_errors.push(PresetLoadError {
                        file: dir,
                        message: format!("failed to read preset directory: {error}"),
                    });
                    Vec::new()
                }
            };
            files.sort();
            for file in files {
                match std::fs::read_to_string(&file)
                    .map_err(|e| e.to_string())
                    .and_then(|text| parse_preset(&text, &file).map_err(|e| e.to_string()))
                {
                    Ok(mut preset) => {
                        preset.source = PresetSource::User;
                        match presets.iter_mut().find(|p| p.key == preset.key) {
                            Some(existing) => *existing = preset,
                            None => presets.push(preset),
                        }
                    }
                    Err(message) => load_errors.push(PresetLoadError { file, message }),
                }
            }
        }

        // 统一按 key 字典序：provider 多时可预期，同家国内外站相邻，用户 preset 也会插入正确位置
        presets.sort_by(|a, b| a.key.cmp(&b.key));
        Self {
            presets,
            load_errors,
        }
    }

    pub fn all(&self) -> &[Preset] {
        &self.presets
    }

    pub fn get(&self, key: &str) -> Result<&Preset> {
        self.presets
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| Error::PresetNotFound(key.to_string()))
    }
}

fn parse_preset(text: &str, path: &Path) -> Result<Preset> {
    let file: PresetFile = toml_edit::de::from_str(text).map_err(|e| Error::InvalidToml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if file.preset.key.is_empty() {
        return Err(Error::InvalidInput("preset key must not be empty".into()));
    }
    if file
        .preset
        .model_choices
        .iter()
        .any(|choice| choice.id.is_empty())
    {
        return Err(Error::InvalidInput(
            "preset model choice id must not be empty".into(),
        ));
    }
    let mut preset = file.preset;
    preset.base_url = preset.base_url.filter(|url| !url.is_empty());
    preset.key_url = preset.key_url.filter(|url| !url.is_empty());
    Ok(preset)
}
