//! 全局配置写路径：改 config.toml + 自动重新同步全部 profile。
//! TUI 全局设置面板的唯一入口（UI 不直接碰文件）。

use crate::error::{Error, Result};

use super::Donn;

/// 对全局配置的单项变更。
#[derive(Debug, Clone)]
pub enum ConfigChange {
    /// UI 基于 `base` 改成 `value`；进锁后只应用两者有差异的字段，避免陈旧
    /// TUI 快照覆盖其它进程刚修改的不同旋钮。
    Knobs {
        base: crate::knobs::Knobs,
        value: crate::knobs::Knobs,
    },
    /// `[defaults.env]` 键值。
    SetDefaultEnv(String, String),
    RemoveDefaultEnv(String),
    /// `[defaults.settings]` 顶层字段（任意 JSON 值）。
    SetDefaultSetting(String, serde_json::Value),
    RemoveDefaultSetting(String),
}

/// 一次配置变更的结果：保存 + 全量同步的汇总。
#[derive(Debug, Clone, Default)]
pub struct ConfigReport {
    /// 成功同步的 profile 数。
    pub synced: usize,
    /// 被覆盖的手改，`profile:key` 形式。
    pub overwritten: Vec<String>,
    /// 同步失败的 profile 及原因。
    pub errors: Vec<String>,
}

impl Donn {
    /// 应用配置变更：校验 → 落盘 config.toml → 重新同步全部 profile。
    pub fn edit_config(&self, change: ConfigChange) -> Result<ConfigReport> {
        let _lock = self.write_lock()?;
        // 进锁后重读，避免长期运行的 TUI 覆盖其它进程刚写入的配置。
        let mut config = crate::config::GlobalConfig::load(&self.home)?;
        match change {
            ConfigChange::Knobs { base, value } => {
                value.validate()?;
                apply_knob_changes(&mut config.defaults.knobs, &base, value);
            }
            ConfigChange::SetDefaultEnv(key, value) => {
                let key = valid_key(&key)?;
                crate::keys::validate_env_entry(&key, &value)?;
                config.defaults.env.insert(key, value);
            }
            ConfigChange::RemoveDefaultEnv(key) => {
                config.defaults.env.remove(&key);
            }
            ConfigChange::SetDefaultSetting(key, value) => {
                let key = valid_key(&key)?;
                if crate::keys::RESERVED_TOP_KEYS.contains(&key.as_str()) {
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

        // 全量同步：让默认立即落到每个 profile
        let mut report = ConfigReport::default();
        for name in self.profile_names()? {
            match self.sync_unlocked(&name) {
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

fn apply_knob_changes(
    current: &mut crate::knobs::Knobs,
    base: &crate::knobs::Knobs,
    value: crate::knobs::Knobs,
) {
    if value.agent_teams != base.agent_teams {
        current.agent_teams = value.agent_teams;
    }
    if value.tool_search != base.tool_search {
        current.tool_search = value.tool_search;
    }
    if value.permission_mode != base.permission_mode {
        current.permission_mode = value.permission_mode;
    }
    if value.hide_attribution != base.hide_attribution {
        current.hide_attribution = value.hide_attribution;
    }
    if value.api_timeout_ms != base.api_timeout_ms {
        current.api_timeout_ms = value.api_timeout_ms;
    }
    if value.disable_nonessential_traffic != base.disable_nonessential_traffic {
        current.disable_nonessential_traffic = value.disable_nonessential_traffic;
    }
    let keys: std::collections::BTreeSet<&String> =
        base.extra.keys().chain(value.extra.keys()).collect();
    for key in keys {
        if value.extra.get(key) != base.extra.get(key) {
            current.set_extra(key, value.extra.get(key).cloned());
        }
    }
}

fn valid_key(key: &str) -> Result<String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(Error::InvalidInput("key must not be empty".into()));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::DonnHome;
    use tempfile::TempDir;

    #[test]
    fn reserved_settings_keys_rejected() {
        let dir = TempDir::new().unwrap();
        let donn = Donn::with_home(DonnHome::for_test(dir.path())).unwrap();
        for key in ["env", "permissions"] {
            let err = donn.edit_config(ConfigChange::SetDefaultSetting(
                key.into(),
                serde_json::Value::Bool(true),
            ));
            assert!(err.is_err(), "{key} 必须被拒绝");
        }
        let err = donn.edit_config(ConfigChange::SetDefaultEnv("  ".into(), "1".into()));
        assert!(err.is_err(), "空 key 必须被拒绝");
    }

    #[test]
    fn edit_config_persists_and_reloads() {
        let dir = TempDir::new().unwrap();
        let home = DonnHome::for_test(dir.path());
        std::fs::create_dir_all(home.root()).unwrap();
        let donn = Donn::with_home(DonnHome::for_test(dir.path())).unwrap();
        donn.edit_config(ConfigChange::SetDefaultSetting(
            "spinnerTipsEnabled".into(),
            serde_json::Value::Bool(false),
        ))
        .unwrap();

        let again = Donn::with_home(DonnHome::for_test(dir.path())).unwrap();
        assert_eq!(
            again
                .config()
                .unwrap()
                .defaults
                .settings
                .get("spinnerTipsEnabled"),
            Some(&serde_json::Value::Bool(false))
        );
    }
}
