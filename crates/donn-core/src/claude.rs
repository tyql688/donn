//! Claude Code 配置文件的读写：JSON 原样读、原子写，加一个 settings.json 的只读视图
//! [`SettingsView`]——对该文件的读取都经它 + [`crate::keys`] 的键名，不散落字符串字面量。
//!
//! donn 依赖的上游行为：
//! - `CLAUDE_CONFIG_DIR` 使全部状态落到指定目录（full 隔离的支点）；
//! - `<config_dir>/settings.json` 的 `env` 对象在启动时注入；
//! - `<config_dir>/.claude.json` 的 onboarding / key 确认屏字段；
//! - `--settings <file>` 压过 `~/.claude` 的同名配置（shared 隔离的支点）。
//!
//! 键名与取值的含义都在 [`crate::keys`]。

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::keys;
use crate::secret::{KeyState, Secret};

/// 读 JSON 文件为 `Value`。文件不存在 → `{}`；非法 JSON → 报错终止（绝不覆盖）。
/// 绝不定义完整 struct 反序列化 —— 未知字段必须原样穿透。
pub fn read_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let bytes =
        std::fs::read(path).map_err(io_ctx(format!("failed to read {}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::InvalidJson {
        path: path.to_path_buf(),
        source,
    })
}

/// 原子写 JSON（pretty + 末尾换行）。
/// settings.json / .claude.json 可能含 secret：新建时权限收紧到 0600。
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::InvalidJson {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    fsx::write_atomic_mode(path, &bytes, Some(0o600))
}

/// settings.json `Value` 的类型化只读视图。持有 `&Value`，零拷贝。
#[derive(Debug, Clone, Copy)]
pub struct SettingsView<'a> {
    value: &'a Value,
}

impl<'a> SettingsView<'a> {
    pub fn new(value: &'a Value) -> Self {
        Self { value }
    }

    /// `env` 对象中某键的字符串值。
    pub fn env(&self, key: &str) -> Option<&'a str> {
        self.value.get("env")?.get(key)?.as_str()
    }

    /// `env` 对象的全部键值对（顺序保留；非字符串值序列化为字符串）。
    pub fn env_entries(&self) -> Vec<(String, String)> {
        let env = self.value.get("env").and_then(Value::as_object);
        env.into_iter()
            .flatten()
            .map(|(k, v)| {
                let text = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                (k.clone(), text)
            })
            .collect()
    }

    /// 从认证 env 键中取回 secret（auth_token 优先，其次非空 api_key）。
    /// 这是 key 的唯一存身之处——sync/换模式时据此自动迁移，无需重输。
    pub fn secret(&self) -> Option<Secret> {
        self.env(keys::AUTH_TOKEN)
            .and_then(Secret::new)
            .or_else(|| self.env(keys::API_KEY).and_then(Secret::new))
    }

    /// key 状态（UI 展示用；raw key 不出视图）。
    pub fn key_state(&self, needs_key: bool) -> KeyState {
        if !needs_key {
            return KeyState::NotNeeded;
        }
        match self.secret() {
            Some(secret) => KeyState::Present {
                tail4: secret.tail4(),
            },
            None => KeyState::Absent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn missing_file_reads_as_empty_object() {
        let dir = TempDir::new().unwrap();
        let v = read_json(&dir.path().join("nope.json")).unwrap();
        assert_eq!(v, serde_json::json!({}));
    }

    #[test]
    fn invalid_json_is_an_error_not_overwritten() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("broken.json");
        std::fs::write(&path, "{not json").unwrap();
        let err = read_json(&path).unwrap_err();
        assert!(err.to_string().contains("never overwrites"), "{err}");
        // 文件原样保留
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[test]
    fn roundtrip_preserves_key_order() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"zebra":1,"alpha":2,"mid":{"z":1,"a":2}}"#).unwrap();
        let v = read_json(&path).unwrap();
        write_json(&path, &v).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let zebra = text.find("zebra").unwrap();
        let alpha = text.find("alpha").unwrap();
        assert!(
            zebra < alpha,
            "preserve_order must keep insertion order: {text}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn new_secret_bearing_json_files_are_mode_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        write_json(&path, &serde_json::json!({"env": {"TOKEN": "secret"}})).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn env_reads() {
        let v = json!({"env": {
            "ANTHROPIC_BASE_URL": "https://x",
            "ANTHROPIC_AUTH_TOKEN": "sk-12345",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "m-sonnet",
            "N": 5
        }});
        let view = SettingsView::new(&v);
        assert_eq!(view.env("ANTHROPIC_BASE_URL"), Some("https://x"));
        let entries = view.env_entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[3], ("N".into(), "5".into()));
        let empty = json!({});
        assert!(SettingsView::new(&empty).env_entries().is_empty());
    }

    #[test]
    fn secret_recovery_table() {
        // (settings, expect_secret)
        let cases = [
            (
                json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-token"}}),
                Some("sk-token"),
            ),
            (
                json!({"env": {"ANTHROPIC_API_KEY": "sk-key"}}),
                Some("sk-key"),
            ),
            // 两个都有 → auth_token 优先
            (
                json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-t", "ANTHROPIC_API_KEY": "sk-k"}}),
                Some("sk-t"),
            ),
            // 空 api_key（auth_token 模式写的屏蔽值）不算 key
            (json!({"env": {"ANTHROPIC_API_KEY": ""}}), None),
            (json!({}), None),
        ];
        for (settings, expect) in cases {
            let got = SettingsView::new(&settings).secret();
            assert_eq!(got.as_ref().map(Secret::reveal), expect, "{settings}");
        }
    }

    #[test]
    fn key_state() {
        let v = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-abcd1234"}});
        assert_eq!(
            SettingsView::new(&v).key_state(true),
            KeyState::Present {
                tail4: "1234".into()
            }
        );
        let empty = json!({});
        assert_eq!(SettingsView::new(&empty).key_state(true), KeyState::Absent);
        assert_eq!(
            SettingsView::new(&empty).key_state(false),
            KeyState::NotNeeded
        );
    }
}
