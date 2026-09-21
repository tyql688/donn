//! 健康检查。输出：✓/✗ + 一行原因 + 修复建议。
//! 逐项检查永不因单项失败中断；漂移检查与 `Donn::audit_settings` 共用同一实现。

use crate::claude::{self, view::SettingsView};
use crate::launch;
use crate::ops::{Donn, DriftKind};
use crate::preset::AuthMode;
use crate::wrapper;

#[derive(Debug, Clone)]
pub struct Check {
    pub ok: bool,
    pub title: String,
    pub detail: String,
    pub fix: Option<String>,
}

impl Check {
    fn pass(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            ok: true,
            title: title.into(),
            detail: detail.into(),
            fix: None,
        }
    }

    fn fail(title: impl Into<String>, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            ok: false,
            title: title.into(),
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
}

/// 全量检查。
pub fn run(donn: &Donn) -> Vec<Check> {
    let mut checks = Vec::new();

    checks.push(check_claude_bin(donn));
    checks.push(check_bin_dir_on_path(donn));
    checks.extend(check_unknown_knobs(donn));
    checks.extend(check_preset_dir(donn));

    match donn.profile_names() {
        Ok(names) if names.is_empty() => {
            checks.push(Check::pass(
                "profiles",
                "no profiles yet (run `donn` to create one)",
            ));
        }
        Ok(names) => {
            for name in names {
                checks.extend(check_profile(donn, &name));
            }
        }
        Err(e) => checks.push(Check::fail(
            "profiles",
            e.to_string(),
            "check permissions on ~/.donn/profiles",
        )),
    }

    checks
}

fn check_claude_bin(donn: &Donn) -> Check {
    let config = match donn.config() {
        Ok(config) => config,
        Err(e) => {
            return Check::fail("claude binary", e.to_string(), "fix ~/.donn/config.toml");
        }
    };
    match launch::resolve_claude_bin(&config, donn.home()) {
        Ok(path) => {
            let version = claude_version(&path, std::time::Duration::from_secs(10));
            match version {
                Some(v) => Check::pass("claude binary", format!("{} ({v})", path.display())),
                None => Check::fail(
                    "claude binary",
                    format!("{} found but `--version` failed", path.display()),
                    "reinstall Claude Code or fix claude_bin in ~/.donn/config.toml",
                ),
            }
        }
        Err(e) => Check::fail(
            "claude binary",
            e.to_string(),
            "install Claude Code, or set claude_bin in ~/.donn/config.toml",
        ),
    }
}

fn claude_version(path: &std::path::Path, timeout: std::time::Duration) -> Option<String> {
    let (status, stdout) = crate::proc::run_with_timeout(
        path.as_os_str(),
        &[std::ffi::OsStr::new("--version")],
        timeout,
    )?;
    status.success().then(|| stdout.trim().to_string())
}

fn check_bin_dir_on_path(donn: &Donn) -> Check {
    let bin_dir = match donn.bin_dir() {
        Ok(bin_dir) => bin_dir,
        Err(e) => return Check::fail("bin dir on PATH", e.to_string(), "fix ~/.donn/config.toml"),
    };
    if wrapper::bin_dir_in_path(&bin_dir) {
        Check::pass("bin dir on PATH", bin_dir.display().to_string())
    } else {
        Check::fail(
            "bin dir on PATH",
            format!(
                "{} is not on PATH; profile commands won't resolve",
                bin_dir.display()
            ),
            format!(
                "add to your shell rc: export PATH=\"{}:$PATH\"",
                bin_dir.display()
            ),
        )
    }
}

/// `[defaults.knobs]` 里 donn 不认识的键：原样保留但不生效，必须让用户看见
/// （旧版本字段名、拼写错误都落在这里）。
fn check_unknown_knobs(donn: &Donn) -> Vec<Check> {
    use crate::knobs::{BOOL_KNOBS, VALUE_KNOBS};
    let Ok(config) = donn.config() else {
        return Vec::new(); // 配置本身坏了由 claude binary 那项报告
    };
    let unknown: Vec<&str> = config
        .defaults
        .knobs
        .extra
        .keys()
        .map(String::as_str)
        .filter(|key| {
            !BOOL_KNOBS.iter().any(|k| k.field == *key)
                && !VALUE_KNOBS.iter().any(|k| k.field == *key)
        })
        .collect();
    if unknown.is_empty() {
        return Vec::new();
    }
    vec![Check::fail(
        "config.toml knobs",
        format!("unknown keys have no effect: {}", unknown.join(", ")),
        "rename or remove them under [defaults.knobs] (see docs/GLOBAL-SETTINGS.md)",
    )]
}

fn check_preset_dir(donn: &Donn) -> Vec<Check> {
    donn.presets()
        .load_errors
        .iter()
        .map(|e| {
            Check::fail(
                "preset file",
                format!("{}: {}", e.file.display(), e.message),
                "fix the TOML or delete the file from ~/.donn/presets.d/",
            )
        })
        .collect()
}

fn check_profile(donn: &Donn, name: &str) -> Vec<Check> {
    let mut checks = Vec::new();
    let title = format!("profile '{name}'");

    // spec 可解析 + 目录结构完整
    let spec = match donn.spec(name) {
        Ok(spec) => spec,
        Err(e) => {
            checks.push(Check::fail(
                &title,
                e.to_string(),
                "fix profile.toml by hand or recreate the profile",
            ));
            return checks;
        }
    };
    if !donn.home().claude_config_dir(name).is_dir() {
        checks.push(Check::fail(
            &title,
            "claude/ config dir missing",
            "recreate the profile (delete + add in `donn`)",
        ));
        return checks;
    }

    // .claude.json 可解析（footprint 托管的第二个文件；坏了 sync 会整体失败）
    match claude::read_json(&donn.home().claude_json_file(name)) {
        Ok(_) => checks.push(Check::pass(format!("{title}: .claude.json"), "parses")),
        Err(e) => checks.push(Check::fail(
            format!("{title}: .claude.json"),
            e.to_string(),
            "fix the JSON by hand, then sync the profile in `donn`",
        )),
    }

    // settings.json 可解析 + 漂移 + 认证
    match claude::read_json(&donn.home().settings_file(name)) {
        Ok(settings) => {
            // 漂移：settings.json 与 spec 渲染值的偏差（手改以 donn 为准）
            match donn.audit_settings(name) {
                Ok(drift) if drift.is_empty() => {
                    checks.push(Check::pass(
                        format!("{title}: settings.json"),
                        "parses; in sync with profile.toml",
                    ));
                }
                Ok(drift) => {
                    let described: Vec<String> = drift
                        .iter()
                        .map(|d| match d.kind {
                            DriftKind::Missing => format!("{} (removed)", d.key),
                            DriftKind::Modified => format!("{} (edited)", d.key),
                        })
                        .collect();
                    checks.push(Check::fail(
                        format!("{title}: drift"),
                        format!("hand-edited donn-owned keys: {}", described.join(", ")),
                        "sync the profile in `donn` to restore them (edits to these keys belong in donn)",
                    ));
                }
                Err(e) => checks.push(Check::fail(
                    format!("{title}: drift"),
                    e.to_string(),
                    "fix the preset/profile.toml, then re-run doctor",
                )),
            }

            // 认证检查（按 spec 记录的 auth 模式）
            let view = SettingsView::new(&settings);
            let auth_ok = match spec.auth.mode {
                AuthMode::None => true,
                AuthMode::ApiKey | AuthMode::AuthToken => view.secret().is_some(),
            };
            if auth_ok {
                checks.push(Check::pass(
                    format!("{title}: auth"),
                    spec.auth.mode.label(),
                ));
            } else {
                checks.push(Check::fail(
                    format!("{title}: auth"),
                    format!(
                        "auth env for mode {} is missing or empty",
                        spec.auth.mode.label()
                    ),
                    "set the API key in the profile detail pane",
                ));
            }

            // preset 存在性
            let catalog = donn.presets();
            if catalog.get(&spec.preset).is_err() {
                checks.push(Check::fail(
                    format!("{title}: preset"),
                    format!("preset '{}' not found", spec.preset),
                    "add a matching toml under ~/.donn/presets.d/ or recreate the profile",
                ));
            }
        }
        Err(e) => checks.push(Check::fail(
            format!("{title}: settings.json"),
            e.to_string(),
            "fix the JSON by hand, then run sync to regenerate donn-owned keys".to_string(),
        )),
    }

    // wrapper 存在且指向有效
    let bin_dir = match donn.bin_dir() {
        Ok(bin_dir) => bin_dir,
        Err(e) => {
            checks.push(Check::fail(
                format!("{title}: wrappers"),
                e.to_string(),
                "fix ~/.donn/config.toml",
            ));
            return checks;
        }
    };
    for alias in &spec.wrapper.aliases {
        let path = wrapper::wrapper_path(&bin_dir, alias);
        let subtitle = format!("{title}: wrapper '{alias}'");
        if !path.exists() {
            checks.push(Check::fail(
                &subtitle,
                format!("{} missing", path.display()),
                "re-add the alias in the profile detail pane",
            ));
            continue;
        }
        match wrapper::is_donn_wrapper(&path) {
            Ok(true) => match wrapper::wrapper_target(&path) {
                Ok(Some(target)) if target == *name => {
                    checks.push(Check::pass(&subtitle, path.display().to_string()));
                }
                Ok(target) => checks.push(Check::fail(
                    &subtitle,
                    format!(
                        "points to '{}' instead of '{name}'",
                        target.unwrap_or_default()
                    ),
                    "remove and re-add the alias in the profile detail pane",
                )),
                Err(e) => checks.push(Check::fail(&subtitle, e.to_string(), "re-add the alias")),
            },
            Ok(false) => checks.push(Check::fail(
                &subtitle,
                format!("{} exists but was not generated by donn", path.display()),
                "pick a different alias for this profile",
            )),
            Err(e) => checks.push(Check::fail(
                &subtitle,
                e.to_string(),
                "check file permissions",
            )),
        }
    }

    checks
}

/// CI/脚本用：有任意 ✗ 项返回 false。
pub fn all_ok(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.ok)
}

/// 文本渲染（CLI 输出；TUI 有自己的渲染）。
pub fn render_text(checks: &[Check]) -> String {
    let mut out = String::new();
    for check in checks {
        let mark = if check.ok { "✓" } else { "✗" };
        out.push_str(&format!("{mark} {}: {}\n", check.title, check.detail));
        if let Some(fix) = &check.fix {
            out.push_str(&format!("    fix: {fix}\n"));
        }
    }
    out
}
