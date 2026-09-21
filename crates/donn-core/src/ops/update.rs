//! 写路径：单字段编辑 / 换 key / 重新同步，全部收敛为「改 spec + regenerate」。
//! key 自动搬迁：sync 时从 settings.json 现有认证键读回 secret，换 auth 模式无需重输。

use serde_json::Value;

use crate::claude::{self, view::SettingsView};
use crate::error::Result;
use crate::keys::ModelSlot;
use crate::reconcile;
use crate::render::{self, Rendered};
use crate::secret::Secret;
use crate::spec::{Isolation, ProfileSpec};
use crate::timefmt;

use super::Donn;

/// 对 spec 意图的单项变更。
#[derive(Debug, Clone)]
pub enum SpecChange {
    /// `None` = 清除覆盖，回到跟随 preset。
    BaseUrl(Option<String>),
    /// `None` = 清除覆盖。
    Model(ModelSlot, Option<String>),
    /// 某个模型 id 的上下文窗口；`None` = 删掉这条（回到 preset 的数字，或交给 Claude Code）。
    ModelWindow(String, Option<u64>),
    SetEnv(String, String),
    RemoveEnv(String),
    /// 切换隔离模式（regenerate 会同步增删共享模式的 connectors 停用 env）。
    Isolation(Isolation),
}

/// 一次 regenerate 的结果报告。
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    /// donn 拥有的键上被覆盖的用户手改（漂移，以 donn 为准）。
    pub overwritten: Vec<String>,
}

/// settings.json 与 spec 渲染结果的偏差（doctor / $EDITOR 返回后检出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub key: String,
    pub kind: DriftKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftKind {
    /// donn 拥有的键被手工删除。
    Missing,
    /// donn 拥有的键被手工改值（下次 sync 覆盖）。
    Modified,
}

/// regenerate 的 secret 来源。
pub(super) enum SecretSource {
    /// 从现有 settings.json 的认证键读回（自动搬迁）。
    Recover,
    /// 显式提供（set_key / create）。
    Explicit(Option<Secret>),
}

impl Donn {
    /// 单字段编辑：改意图，重新生成。
    pub fn edit(&self, name: &str, change: SpecChange) -> Result<SyncReport> {
        self.edit_many(name, [change])
    }

    /// 多项编辑：一次 regenerate（如模型选择：写槽位 + 清残留 package 键）。
    pub fn edit_many(
        &self,
        name: &str,
        changes: impl IntoIterator<Item = SpecChange>,
    ) -> Result<SyncReport> {
        let _lock = self.write_lock()?;
        let old_spec = self.spec(name)?;
        let mut spec = old_spec.clone();
        for change in changes {
            apply_change(&mut spec, change)?;
        }
        self.regenerate(Some(&old_spec), &mut spec, SecretSource::Recover)
    }

    /// 设置/更换 API key。key 只写入 settings.json，绝不入 spec。
    pub fn set_key(&self, name: &str, key: Secret) -> Result<SyncReport> {
        let _lock = self.write_lock()?;
        let old_spec = self.spec(name)?;
        let mut spec = old_spec.clone();
        self.regenerate(
            Some(&old_spec),
            &mut spec,
            SecretSource::Explicit(Some(key)),
        )
    }

    /// 从 spec 重新生成配置（preset 数据更新、漂移修复都走这里）。
    pub fn sync(&self, name: &str) -> Result<SyncReport> {
        let _lock = self.write_lock()?;
        self.sync_unlocked(name)
    }

    pub(super) fn sync_unlocked(&self, name: &str) -> Result<SyncReport> {
        let old_spec = self.spec(name)?;
        let mut spec = old_spec.clone();
        // preset 数据可能更新了 auth 模式：跟随之，key 由 Recover 自动搬迁
        let catalog = self.presets();
        if let Ok(preset) = catalog.get(&spec.preset) {
            spec.auth.mode = preset.auth_mode;
        }
        self.regenerate(Some(&old_spec), &mut spec, SecretSource::Recover)
    }

    /// 漂移检出：settings.json 中 donn 拥有的键与 spec 渲染值的偏差。
    /// 不修改任何文件；键值不进入结果（可能含 secret）。
    pub fn audit_settings(&self, name: &str) -> Result<Vec<Drift>> {
        let spec = self.spec(name)?;
        let catalog = self.presets();
        let preset = catalog.get(&spec.preset)?;
        let settings = claude::read_json(&self.home.settings_file(name))?;
        let config = crate::config::GlobalConfig::load(&self.home)?;
        drift_for(&spec, preset, &settings, &config.defaults)
    }

    /// 写路径的公共主干。`old_spec` = 变更前意图，用于区分
    /// 「donn 自己上次写的值」（正常更新）与「用户手改」（覆盖并提示）；
    /// create 无前状态传 `None`，手改判定按「无旧渲染」处理。
    pub(super) fn regenerate(
        &self,
        old_spec: Option<&ProfileSpec>,
        spec: &mut ProfileSpec,
        secret: SecretSource,
    ) -> Result<SyncReport> {
        let catalog = self.presets();
        let preset = catalog.get(&spec.preset)?;
        let name = spec.name.clone();

        let existing_settings = claude::read_json(&self.home.settings_file(&name))?;
        let existing_claude = claude::read_json(&self.home.claude_json_file(&name))?;

        let recovered = SettingsView::new(&existing_settings).secret();
        // 旧渲染：现有文件里 donn 上次写的应然值（secret 用文件里现有的）。
        // 旧 preset 在本版 donn 里已不存在时按「无旧渲染」处理（手改判定退化为
        // 全部提示，footprint 驱动的清理不受影响）——硬报错会让 sync 永久卡死。
        let config = crate::config::GlobalConfig::load(&self.home)?;
        let defaults = &config.defaults;
        let old_rendered = match old_spec {
            Some(old) => catalog
                .get(&old.preset)
                .map(|old_preset| render::render(old, old_preset, recovered.as_ref(), defaults))
                .unwrap_or_default(),
            None => render::Rendered::default(),
        };
        let secret = match secret {
            SecretSource::Explicit(s) => s,
            SecretSource::Recover => recovered,
        };

        let rendered = render::render(spec, preset, secret.as_ref(), defaults);
        validate_rendered_env(&rendered)?;
        let settings = reconcile::settings(
            &existing_settings,
            &spec.footprint,
            &rendered,
            &old_rendered,
        )?;
        let claude_json = reconcile::claude_json(&existing_claude, &spec.footprint, &rendered)?;

        // 稳态 sync 是纯 no-op：wrapper 每次启动前都会走这里，不能因此触碰
        // settings/spec 的字节、mtime 或 updated_at。
        if existing_settings != settings.value {
            claude::write_json(&self.home.settings_file(&name), &settings.value)?;
        }
        if existing_claude != claude_json.value {
            claude::write_json(&self.home.claude_json_file(&name), &claude_json.value)?;
        }

        // shared 模式的 --settings overlay 整份归 donn：读不出就重写，full 模式清掉残留。
        let overlay_path = self.home.shared_settings_file(&name);
        let overlay = match spec.isolation {
            Isolation::Shared => render::shared_overlay(spec, preset, defaults),
            Isolation::Full => serde_json::Map::new(),
        };
        if overlay.is_empty() {
            if overlay_path.exists() {
                std::fs::remove_file(&overlay_path).map_err(crate::error::io_ctx(format!(
                    "failed to remove {}",
                    overlay_path.display()
                )))?;
            }
        } else {
            let value = Value::Object(overlay);
            if claude::read_json(&overlay_path).ok().as_ref() != Some(&value) {
                claude::write_json(&overlay_path, &value)?;
            }
        }

        spec.footprint = rendered.footprint();
        if old_spec.is_none() || old_spec.is_some_and(|old| !spec_equivalent(spec, old)) {
            spec.updated_at = timefmt::now_rfc3339();
            spec.save(&self.home.spec_file(&name))?;
        }

        Ok(SyncReport {
            overwritten: settings.overwritten,
        })
    }
}

/// `updated_at` 是落盘元数据，不参与「意图/footprint 是否变化」判定。
fn spec_equivalent(left: &ProfileSpec, right: &ProfileSpec) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.updated_at.clear();
    right.updated_at.clear();
    left == right
}

fn apply_change(spec: &mut ProfileSpec, change: SpecChange) -> Result<()> {
    match change {
        SpecChange::BaseUrl(url) => {
            spec.intent.base_url = url.filter(|u| !u.is_empty());
        }
        SpecChange::Model(slot, value) => {
            spec.intent.models.set(slot, value);
        }
        SpecChange::ModelWindow(id, tokens) => match tokens.filter(|n| *n > 0) {
            Some(tokens) => {
                spec.intent.model_windows.insert(id, tokens);
            }
            None => {
                spec.intent.model_windows.remove(&id);
            }
        },
        SpecChange::SetEnv(key, value) => {
            // donn 认识档位表的键在写入时校验：typo 会被 Claude Code 静默忽略，
            // 与其落盘后 UI 显示与实际行为脱节，不如当场报错
            if key == crate::keys::EFFORT && crate::keys::Effort::from_env(Some(&value)).is_none() {
                return Err(crate::error::Error::InvalidInput(format!(
                    "invalid effort level `{value}` (expected one of low/medium/high/xhigh/max)"
                )));
            }
            crate::keys::validate_env_entry(&key, &value)?;
            spec.intent.env.insert(key, value);
        }
        SpecChange::RemoveEnv(key) => {
            spec.intent.env.remove(&key);
        }
        SpecChange::Isolation(isolation) => {
            spec.isolation = isolation;
        }
    }
    Ok(())
}

pub(super) fn validate_rendered_env(rendered: &Rendered) -> Result<()> {
    for (key, value) in &rendered.env {
        crate::keys::validate_env_entry(key, value)?;
    }
    Ok(())
}

pub(super) fn drift_for(
    spec: &ProfileSpec,
    preset: &crate::preset::Preset,
    settings: &Value,
    defaults: &crate::config::Defaults,
) -> Result<Vec<Drift>> {
    let secret = SettingsView::new(settings).secret();
    let rendered = render::render(spec, preset, secret.as_ref(), defaults);
    validate_rendered_env(&rendered)?;
    Ok(diff_drift(settings, &rendered))
}

/// settings.json 与渲染结果逐键对比：env 键 + donn 拥有的顶层字段。
pub(super) fn diff_drift(settings: &Value, rendered: &Rendered) -> Vec<Drift> {
    let view = SettingsView::new(settings);
    let env_drift = rendered
        .env
        .iter()
        .filter_map(|(key, expected)| diff_one(key, view.env(key), &expected.as_str()));
    let top_drift = rendered
        .settings_top
        .iter()
        .filter_map(|(key, expected)| diff_one(key, settings.get(key), &expected));
    let mode_drift = rendered.permissions_default_mode.iter().filter_map(|mode| {
        let actual = settings
            .get("permissions")
            .and_then(|p| p.get(crate::keys::PERMISSIONS_DEFAULT_MODE))
            .and_then(Value::as_str);
        diff_one("permissions.defaultMode", actual, &mode.as_str())
    });
    env_drift.chain(top_drift).chain(mode_drift).collect()
}

/// 单键对比：缺失 / 值不同 / 一致。
fn diff_one<T: PartialEq>(key: &str, actual: Option<T>, expected: &T) -> Option<Drift> {
    let kind = match actual {
        None => DriftKind::Missing,
        Some(actual) if actual != *expected => DriftKind::Modified,
        Some(_) => return None,
    };
    Some(Drift {
        key: key.to_string(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drift_diff_table() {
        let rendered = Rendered {
            env: vec![
                ("A".into(), "1".into()),
                ("B".into(), "2".into()),
                ("C".into(), "3".into()),
            ],
            ..Default::default()
        };
        let settings = json!({"env": {"A": "1", "B": "changed", "USER": "x"}});
        let drift = diff_drift(&settings, &rendered);
        assert_eq!(
            drift,
            vec![
                Drift {
                    key: "B".into(),
                    kind: DriftKind::Modified
                },
                Drift {
                    key: "C".into(),
                    kind: DriftKind::Missing
                },
            ]
        );
    }

    #[test]
    fn drift_covers_donn_owned_top_level_keys() {
        let rendered = Rendered {
            settings_top: vec![
                ("effortLevel".into(), json!("high")),
                ("attribution".into(), json!({"commit": false})),
            ],
            ..Default::default()
        };
        // effortLevel 手改、attribution 手删，均须检出；用户自己的顶层键不参与
        let settings = json!({"effortLevel": "low", "theme": "dark"});
        let drift = diff_drift(&settings, &rendered);
        assert_eq!(
            drift,
            vec![
                Drift {
                    key: "effortLevel".into(),
                    kind: DriftKind::Modified
                },
                Drift {
                    key: "attribution".into(),
                    kind: DriftKind::Missing
                },
            ]
        );
    }

    #[test]
    fn spec_equivalence_ignores_only_updated_at() {
        let base = ProfileSpec {
            name: "p".into(),
            preset: "official".into(),
            updated_at: "before".into(),
            ..Default::default()
        };
        let mut changed_time = base.clone();
        changed_time.updated_at = "after".into();
        assert!(spec_equivalent(&base, &changed_time));

        let mut changed_intent = changed_time;
        changed_intent.intent.base_url = Some("https://example.invalid".into());
        assert!(!spec_equivalent(&base, &changed_intent));
    }
}
