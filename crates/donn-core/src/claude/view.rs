//! SettingsView：settings.json `Value` 的类型化只读视图。
//! 所有对该文件的读取都经这里 + [`crate::keys`] 的键名——不散落字符串字面量。

use serde_json::Value;

use crate::keys::{self, ModelSlot};
use crate::secret::{KeyState, Secret};

/// 只读视图。持有 `&Value`，零拷贝。
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
        match self.value.get("env").and_then(Value::as_object) {
            Some(map) => map
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string()),
                    )
                })
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn model(&self, slot: ModelSlot) -> Option<&'a str> {
        self.env(slot.env_key())
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
        assert_eq!(view.model(ModelSlot::Sonnet), Some("m-sonnet"));
        assert_eq!(view.model(ModelSlot::Opus), None);
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
