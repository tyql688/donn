//! Claude Code 配置文件的读写：JSON 原样读、原子写，加一个 settings.json 的只读视图。
//!
//! donn 依赖的上游行为：
//! - `CLAUDE_CONFIG_DIR` 使全部状态落到指定目录（full 隔离的支点）；
//! - `<config_dir>/settings.json` 的 `env` 对象在启动时注入；
//! - `<config_dir>/.claude.json` 的 onboarding / key 确认屏字段；
//! - `--settings <file>` 压过 `~/.claude` 的同名配置（shared 隔离的支点）。
//!
//! 键名与取值的含义都在 [`crate::keys`]。

pub mod view;

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{Error, Result, io_ctx};
use crate::fsx;

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
