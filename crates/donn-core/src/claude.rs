//! Claude Code 配置文件的读写：JSON 原样读、原子写，再加一个 settings.json 的只读视图
//! [`SettingsView`]。对这个文件的读取都经过它和 [`crate::keys`] 里的 key 名，不散落字符串字面量。
//!
//! donn 依赖的上游行为：
//! - `CLAUDE_CONFIG_DIR` 让全部状态落到指定目录（full isolation 的支点）；
//! - `<config_dir>/settings.json` 的 `env` 对象在启动时注入；
//! - `<config_dir>/.claude.json` 的 onboarding 和 key 确认屏字段；
//! - `--settings <file>` 压过 `~/.claude` 的同名配置（shared isolation 的支点）。
//!
//! key 名和取值的含义都在 [`crate::keys`]。

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;
use crate::keys;
use crate::secret::{KeyState, Secret};

/// 把 JSON 文件读成 `Value`。文件不存在返回 `{}`；非法 JSON 直接报错终止，绝不覆盖。
/// 这里绝不定义完整 struct 反序列化：未知字段必须原样穿透。
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

/// 原子写 JSON（pretty，末尾换行）。
/// settings.json 和 .claude.json 可能含 secret，新建时权限收紧到 0600。
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

    /// `env` 对象里某个 key 的字符串值。
    pub fn env(&self, key: &str) -> Option<&'a str> {
        self.value.get("env")?.get(key)?.as_str()
    }

    /// `env` 对象的全部 key-value（保留顺序；非字符串值序列化成字符串）。
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

    /// 从认证 env key 里取回 secret（auth_token 优先，其次非空 api_key）。
    /// 这是 key 唯一的存放处；sync 或换模式时据此自动迁移，不用重输。
    pub fn secret(&self) -> Option<Secret> {
        self.env(keys::AUTH_TOKEN)
            .and_then(Secret::new)
            .or_else(|| self.env(keys::API_KEY).and_then(Secret::new))
    }

    /// key 状态（UI 展示用；raw key 不出这个视图）。
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
