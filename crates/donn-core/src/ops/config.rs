//! 全局配置写路径：改 config.toml，然后自动重新 sync 所有 profile。
//! TUI 全局设置面板的唯一入口，UI 不直接碰文件。

use crate::error::{Error, Result};
use crate::keys;
use crate::knobs::Knobs;

use super::Donn;

/// 对全局配置的单向变更。
#[derive(Debug, Clone)]
pub enum ConfigChange {
    /// UI 基于 `base` 改成 `value`；进 lock 后只应用两者有差异的字段，避免过期的
    /// TUI 快照覆盖其它进程刚改的不同 knob。
    Knobs {
        base: Knobs,
        value: Knobs,
    },
    /// `[defaults.env]` 里的 key-value。
    SetDefaultEnv(String, String),
    RemoveDefaultEnv(String),
    /// `[defaults.settings]` 顶层字段（任意 JSON 值）。
    SetDefaultSetting(String, serde_json::Value),
    RemoveDefaultSetting(String),
}

/// 一次配置变更的结果：保存 config.toml 加上全量 sync 的汇总。
#[derive(Debug, Clone, Default)]
pub struct ConfigReport {
    /// 成功 sync 的 profile 数。
    pub synced: usize,
    /// 被覆盖的手改，格式是 `profile:key`。
    pub overwritten: Vec<String>,
    /// sync 失败的 profile 和原因。
    pub errors: Vec<String>,
}

impl Donn {
    /// 应用配置变更：校验、落盘 config.toml、重新 sync 所有 profile。
    pub fn edit_config(&self, change: ConfigChange) -> Result<ConfigReport> {
        let _lock = self.write_lock()?;
        // 进 lock 后重读，避免长期运行的 TUI 覆盖其它进程刚写入的配置。
        let mut config = self.config()?;
        // 旧渲染必须用变更前的 defaults 计算，否则全局默认值本身的变化会被误判成手改。
        let old_defaults = config.defaults.clone();
        match change {
            ConfigChange::Knobs { base, value } => {
                value.validate()?;
                config.defaults.knobs.apply_changes_from(&base, &value);
            }
            ConfigChange::SetDefaultEnv(key, value) => {
                let key = valid_key(&key)?;
                keys::validate_env_entry(&key, &value)?;
                config.defaults.env.insert(key, value);
            }
            ConfigChange::RemoveDefaultEnv(key) => {
                config.defaults.env.remove(&key);
            }
            ConfigChange::SetDefaultSetting(key, value) => {
                let key = valid_key(&key)?;
                if keys::RESERVED_TOP_KEYS.contains(&key.as_str()) {
                    return Err(Error::InvalidInput(format!(
                        "`{key}` is managed by donn and cannot be a default"
                    )));
                }
                config.defaults.settings.insert(key, value);
            }
            ConfigChange::RemoveDefaultSetting(key) => {
                config.defaults.settings.remove(&key);
            }
        }
        config.save(&self.home)?;

        // 全量 sync：让默认值立即落到每个 profile
        let mut report = ConfigReport::default();
        for name in self.profile_names()? {
            match self.sync_unlocked(&name, Some(&old_defaults)) {
                Ok(sync) => {
                    report.synced += 1;
                    report
                        .overwritten
                        .extend(sync.overwritten.iter().map(|k| format!("{name}:{k}")));
                }
                Err(e) => report.errors.push(format!("{name}: {e}")),
            }
        }
        Ok(report)
    }
}

fn valid_key(key: &str) -> Result<String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(Error::InvalidInput("key must not be empty".into()));
    }
    Ok(key)
}
