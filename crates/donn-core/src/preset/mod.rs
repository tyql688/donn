//! Provider preset：纯 TOML 数据，加 provider 不改代码。
//! 加载顺序：内嵌 → `~/.donn/presets.d/` 覆盖（同 key 覆盖）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::home::DonnHome;
use crate::keys::SlotMap;

/// 认证模式：secret 落到 settings.json 的哪个 env 键。
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

/// Claude Code 自己认识的模型 id：去掉网关前缀（`anthropic/`、`us.anthropic.`）后以
/// `claude-` 开头。这类 id 不需要、也不吃 `max_context`。
pub fn is_claude_model(id: &str) -> bool {
    let name = id.rsplit('/').next().unwrap_or(id);
    name.split('.').any(|part| part.starts_with("claude-"))
}

/// 兼容性怪癖 flags：preset 数据里表达，逻辑不特判。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetFlags {
    pub auth_token_also_sets_api_key: bool,
}

/// 模型候选项（数据驱动）。
///
/// UI 候选 = 本列表 ∪ 槽位默认 id；手填任意 id。
/// **套餐不进 intent**：`render()` 按 sonnet 生效 id 匹配本列表后注入
/// [`ModelChoice::package_env`]；UI 只改模型槽。用户在 `intent.env` 里自己写的
/// 同名键优先于套餐。
///
/// ```toml
/// [[preset.model_choices]]
/// id = "k3[1m]"
/// label = "K3 · 1M"
/// max_context = 1048576
/// auto_compact = 1000000   # 省略 = max_context（写入时封顶于 Claude Code 接受的 1000000）
/// pin_subagent = true      # 省略且有 max → true
/// env = { FOO = "1" }      # 可选；后写覆盖结构化键
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

    /// render 注入的配套 env（结构化字段 + freeform）。不写 intent。
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
        // freeform env last so authors can override structured defaults
        for (k, v) in &self.env {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

impl Preset {
    /// 按模型 id 查找套餐（精确匹配优先，再 ASCII 大小写不敏感）。
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
    /// 额外模型候选（`[[preset.model_choices]]`）：带 label / 配套 env 时用。
    /// 即使为空，UI 仍会把 `[preset.models]` 里出现过的 id 收进下拉。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_choices: Vec<ModelChoice>,
    /// 加载来源：`builtin` 或 `user`（presets.d）。运行时填充，不入 TOML。
    #[serde(skip)]
    pub source: PresetSource,
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

/// presets.d 中解析失败的文件（跳过并在 doctor 报告）。
#[derive(Debug, Clone)]
pub struct PresetLoadError {
    pub file: std::path::PathBuf,
    pub message: String,
}

#[derive(Debug)]
pub struct PresetCatalog {
    presets: Vec<Preset>,
    pub load_errors: Vec<PresetLoadError>,
}

/// 内置 preset 清单（加载后统一按 key 排序，此处顺序不影响展示）。
const BUILTIN_PRESETS: &[(&str, &str)] = &[
    ("ant-ling.toml", include_str!("presets/ant-ling.toml")),
    ("official.toml", include_str!("presets/official.toml")),
    ("zai.toml", include_str!("presets/zai.toml")),
    ("kimi-plan.toml", include_str!("presets/kimi-plan.toml")),
    ("minimax.toml", include_str!("presets/minimax.toml")),
    ("deepseek.toml", include_str!("presets/deepseek.toml")),
    ("zai-cn.toml", include_str!("presets/zai-cn.toml")),
    ("kimi-cn.toml", include_str!("presets/kimi-cn.toml")),
    ("minimax-cn.toml", include_str!("presets/minimax-cn.toml")),
    ("mimo.toml", include_str!("presets/mimo.toml")),
    ("mimo-plan.toml", include_str!("presets/mimo-plan.toml")),
    ("lmstudio.toml", include_str!("presets/lmstudio.toml")),
    ("openrouter.toml", include_str!("presets/openrouter.toml")),
    ("siliconflow.toml", include_str!("presets/siliconflow.toml")),
    ("fireworks.toml", include_str!("presets/fireworks.toml")),
    ("huggingface.toml", include_str!("presets/huggingface.toml")),
    ("vercel.toml", include_str!("presets/vercel.toml")),
    ("opencode.toml", include_str!("presets/opencode.toml")),
    ("opencode-go.toml", include_str!("presets/opencode-go.toml")),
    ("qwen-plan.toml", include_str!("presets/qwen-plan.toml")),
    ("xai.toml", include_str!("presets/xai.toml")),
    ("ollama.toml", include_str!("presets/ollama.toml")),
    ("custom.toml", include_str!("presets/custom.toml")),
];

impl PresetCatalog {
    /// 内嵌 → 用户目录覆盖。内嵌解析失败属打包错误，直接 panic（有测试兜底）。
    pub fn load(home: &DonnHome) -> Self {
        #[allow(clippy::expect_used)] // 打包不变量：内嵌 TOML 由测试保证可解析
        let mut presets: Vec<Preset> = BUILTIN_PRESETS
            .iter()
            .map(|(file, text)| {
                parse_preset(text, std::path::Path::new(file)).expect("builtin preset must parse")
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

        // 统一按 key 字典序：渠道多时可预期、同家国内外站相邻，用户 preset 也插入正确位置
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

fn parse_preset(text: &str, path: &std::path::Path) -> Result<Preset> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builtin_presets_parse_and_cover_launch_list() {
        let dir = TempDir::new().unwrap();
        let catalog = PresetCatalog::load(&DonnHome::for_test(dir.path()));
        let keys: Vec<&str> = catalog.all().iter().map(|p| p.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "ant-ling",
                "custom",
                "deepseek",
                "fireworks",
                "huggingface",
                "kimi-cn",
                "kimi-plan",
                "lmstudio",
                "mimo",
                "mimo-plan",
                "minimax",
                "minimax-cn",
                "official",
                "ollama",
                "opencode",
                "opencode-go",
                "openrouter",
                "qwen-plan",
                "siliconflow",
                "vercel",
                "xai",
                "zai",
                "zai-cn"
            ]
        );
        assert!(catalog.load_errors.is_empty());
        for preset in catalog.all() {
            assert!(!preset.key.is_empty());
            assert!(!preset.label.is_empty(), "{}", preset.key);
            assert!(!preset.description.is_empty(), "{}", preset.key);
            assert!(preset.base_url.as_deref() != Some(""), "{}", preset.key);
            assert!(preset.key_url.as_deref() != Some(""), "{}", preset.key);
            assert!(
                preset
                    .model_choices
                    .iter()
                    .all(|choice| !choice.id.is_empty())
            );
        }

        // 表驱动：关键字段抽查
        let zai = catalog.get("zai").unwrap();
        assert_eq!(zai.auth_mode, AuthMode::AuthToken);
        assert_eq!(
            zai.base_url.as_deref(),
            Some("https://api.z.ai/api/anthropic")
        );
        let official = catalog.get("official").unwrap();
        assert_eq!(official.auth_mode, AuthMode::None);
        assert!(official.base_url.is_none());

        let ollama = catalog.get("ollama").unwrap();
        assert!(ollama.flags.auth_token_also_sets_api_key);

        let deepseek = catalog.get("deepseek").unwrap();
        assert_eq!(
            deepseek.base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic")
        );

        let zai_cn = catalog.get("zai-cn").unwrap();
        assert_eq!(
            zai_cn.base_url.as_deref(),
            Some("https://open.bigmodel.cn/api/anthropic")
        );

        // Spot-check structured model_choices + package_env on a long-context preset
        let plan = catalog.get("kimi-plan").unwrap();
        assert_eq!(
            plan.models.get(crate::keys::ModelSlot::Sonnet),
            Some("k3[1m]")
        );
        assert!(plan.model_choices.iter().any(|c| c.id == "k3-256k"));
        let pkg = plan.choice_for_model("k3[1m]").unwrap().package_env();
        assert_eq!(
            pkg.get(crate::keys::MAX_CONTEXT).map(String::as_str),
            Some("1048576")
        );
        assert_eq!(
            pkg.get(crate::keys::SUBAGENT_MODEL).map(String::as_str),
            Some("k3[1m]")
        );
        let zai = catalog.get("zai").unwrap();
        assert_eq!(
            zai.choice_for_model("glm-5.3[1m]")
                .and_then(|c| c.max_context),
            Some(1_000_000)
        );
        let mm = catalog.get("minimax").unwrap();
        assert_eq!(
            mm.choice_for_model("MiniMax-M3[1m]")
                .and_then(|c| c.max_context),
            Some(1_000_000)
        );
    }

    #[test]
    fn claude_models_are_recognized_by_name_not_by_substring() {
        for id in [
            "claude-sonnet-5",
            "claude-sonnet-4.6",
            "anthropic/claude-fable-5.1",
            "us.anthropic.claude-opus-4-8-v1:0",
        ] {
            assert!(is_claude_model(id), "{id}");
        }
        for id in ["glm-5.3", "my-claude-proxy/glm", "gw/claudette", ""] {
            assert!(!is_claude_model(id), "{id}");
        }
    }

    #[test]
    fn every_non_claude_choice_declares_its_context_window() {
        // Claude Code 不认识第三方 id，不给 max_context 就按它猜的窗口压缩；
        // 能解析成 Claude 模型的 id 该变量不生效，所以豁免。
        let dir = TempDir::new().unwrap();
        let catalog = PresetCatalog::load(&DonnHome::for_test(dir.path()));
        for preset in catalog.all() {
            for choice in &preset.model_choices {
                assert!(
                    is_claude_model(&choice.id) || choice.max_context.is_some(),
                    "{}: {} needs max_context",
                    preset.key,
                    choice.id
                );
            }
        }
    }

    #[test]
    fn model_choice_package_env() {
        let e = ModelChoice {
            id: "x".into(),
            max_context: Some(100),
            auto_compact: Some(80),
            env: BTreeMap::from([("FOO".into(), "1".into())]),
            ..Default::default()
        }
        .package_env();
        assert_eq!(
            e.get(crate::keys::MAX_CONTEXT).map(String::as_str),
            Some("100")
        );
        assert_eq!(
            e.get(crate::keys::AUTO_COMPACT).map(String::as_str),
            Some("80")
        );
        assert_eq!(
            e.get(crate::keys::SUBAGENT_MODEL).map(String::as_str),
            Some("x")
        );
        assert_eq!(e.get("FOO").map(String::as_str), Some("1"));

        // 1M 窗口用 1048576 表达时，压缩窗口封顶于 Claude Code 接受的上限
        let capped = ModelChoice {
            id: "y".into(),
            max_context: Some(1_048_576),
            ..Default::default()
        }
        .package_env();
        assert_eq!(
            capped.get(crate::keys::MAX_CONTEXT).map(String::as_str),
            Some("1048576")
        );
        assert_eq!(
            capped.get(crate::keys::AUTO_COMPACT).map(String::as_str),
            Some("1000000")
        );

        let compact_only = ModelChoice {
            id: "solo".into(),
            auto_compact: Some(70),
            pin_subagent: Some(false),
            ..Default::default()
        }
        .package_env();
        assert_eq!(
            compact_only
                .get(crate::keys::AUTO_COMPACT)
                .map(String::as_str),
            Some("70")
        );
        assert!(!compact_only.contains_key(crate::keys::MAX_CONTEXT));
        assert!(!compact_only.contains_key(crate::keys::SUBAGENT_MODEL));

        let preset = Preset {
            model_choices: vec![ModelChoice {
                id: "Exact-ID".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            preset
                .choice_for_model("exact-id")
                .map(|choice| choice.id.as_str()),
            Some("Exact-ID")
        );
    }

    #[test]
    fn user_preset_overrides_builtin_by_key() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.presets_dir()).unwrap();
        std::fs::write(
            home.presets_dir().join("zai.toml"),
            r#"[preset]
key = "zai"
label = "My zai"
description = "override"
base_url = "https://my-relay.example/api"
auth_mode = "auth_token"
"#,
        )
        .unwrap();
        let catalog = PresetCatalog::load(&home);
        let zai = catalog.get("zai").unwrap();
        assert_eq!(zai.label, "My zai");
        assert_eq!(zai.source, PresetSource::User);
        // 覆盖不追加
        assert_eq!(catalog.all().iter().filter(|p| p.key == "zai").count(), 1);
        // 排序不变式：任何来源合并后仍是 key 字典序
        let keys: Vec<&str> = catalog.all().iter().map(|p| p.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn broken_user_preset_is_skipped_and_reported() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.presets_dir()).unwrap();
        std::fs::write(home.presets_dir().join("broken.toml"), "not [valid toml").unwrap();
        let catalog = PresetCatalog::load(&home);
        assert_eq!(catalog.load_errors.len(), 1);
        assert_eq!(catalog.all().len(), BUILTIN_PRESETS.len()); // 内置仍完整
        assert!(
            catalog.load_errors[0]
                .message
                .contains(&home.presets_dir().join("broken.toml").display().to_string())
        );
    }

    #[test]
    fn unknown_preset_key_errors() {
        let dir = TempDir::new().unwrap();
        let catalog = PresetCatalog::load(&DonnHome::for_test(dir.path()));
        assert!(catalog.get("nope").is_err());
    }
}
