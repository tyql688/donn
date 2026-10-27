//! 启动链路：
//! 读 settings.json → 设置 `CLAUDE_CONFIG_DIR` 并注入 env → exec claude。
//!
//! env 优先级（从左到右递增）：进程 env ← settings.json env ← CLAUDE_CONFIG_DIR。
//! settings.json 里的 env 必须在启动 claude 之前注入进程（onboarding 阶段读的是进程 env）。

use std::path::PathBuf;
use std::process::Command;

use crate::claude;
use crate::config::GlobalConfig;
use crate::error::{Error, Result, io_ctx};
use crate::home::DonnHome;
use crate::spec::{Isolation, ProfileSpec};

/// 已经解析好、可以直接执行的启动计划。纯数据，方便 TUI 复用。
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub program: PathBuf,
    /// 要注入的 env（full 模式包含最后强制写入的 `CLAUDE_CONFIG_DIR`）。
    pub env: Vec<(String, String)>,
    /// shared 模式：sync 生成的 overlay 文件，通过 `--settings` 带进 session。
    pub settings_overlay: Option<PathBuf>,
}

impl LaunchPlan {
    /// 用户自己传了 `--settings`（`--settings v` 或 `--settings=v`）。Claude 的 `--settings`
    /// 只认一个值，以用户的为准；donn 的整份 overlay 这次不生效。
    pub fn overlay_overridden(&self, args: &[String]) -> bool {
        self.settings_overlay.is_some()
            && args
                .iter()
                .any(|a| a == "--settings" || a.starts_with("--settings="))
    }
}

/// 定位 claude 二进制：`config.toml` 里的显式路径 > `which("claude")`。
pub fn resolve_claude_bin(config: &GlobalConfig, home: &DonnHome) -> Result<PathBuf> {
    if let Some(explicit) = config.resolve_claude_bin(home) {
        if explicit.is_file() {
            return Ok(explicit);
        }
        return Err(Error::InvalidInput(format!(
            "claude_bin in config.toml points to a missing file: {}; fix it or remove the setting",
            explicit.display()
        )));
    }
    which::which("claude").map_err(|_| Error::ClaudeNotFound)
}

/// 组装启动计划。`name` 对应的 profile 必须已经存在（由调用方校验）。
pub fn prepare(home: &DonnHome, config: &GlobalConfig, name: &str) -> Result<LaunchPlan> {
    let spec = ProfileSpec::load_named(&home.spec_file(name), name)?;
    let settings = claude::read_json(&home.settings_file(name))?;

    let mut env = claude::SettingsView::new(&settings).env_entries();
    env.retain(|(k, _)| k != "CLAUDE_CONFIG_DIR");
    for (key, value) in &env {
        crate::keys::validate_env_entry(key, value)?;
    }

    // shared 模式：model 选择、settings 类 knob、permission mode 全部通过 --settings overlay 文件
    // 压过 ~/.claude 的持久化配置（文件由 sync 生成），命令行只指向文件本身。
    let settings_overlay = (spec.isolation == Isolation::Shared)
        .then(|| home.shared_settings_file(name))
        .filter(|overlay| overlay.is_file());

    // full 模式：最后强制设置 CLAUDE_CONFIG_DIR，即使 settings env 出现同名 key 也以 donn 为准。
    // shared 模式：不动 CLAUDE_CONFIG_DIR，session 和全局配置走 ~/.claude，provider 身份只由注入的 env 决定。
    if spec.isolation == Isolation::Full {
        let dir = std::path::absolute(home.claude_config_dir(name))
            .map_err(io_ctx("failed to resolve the profile config dir"))?;
        env.push(("CLAUDE_CONFIG_DIR".into(), dir.display().to_string()));
    }

    Ok(LaunchPlan {
        program: resolve_claude_bin(config, home)?,
        env,
        settings_overlay,
    })
}

/// 组装子进程命令：先从继承的 env 里剥掉本次没注入的托管 key，再注入 settings env 和
/// `CLAUDE_CONFIG_DIR`。
fn build_command(plan: &LaunchPlan, args: &[String]) -> Command {
    let mut cmd = Command::new(&plan.program);
    if let Some(overlay) = &plan.settings_overlay
        && !plan.overlay_overridden(args)
    {
        cmd.arg("--settings").arg(overlay);
    }
    cmd.args(args);
    let provided: std::collections::HashSet<&str> =
        plan.env.iter().map(|(k, _)| k.as_str()).collect();
    for key in crate::render::managed_env_keys() {
        if !provided.contains(key) {
            cmd.env_remove(key);
        }
    }
    for (key, value) in &plan.env {
        cmd.env(key, value);
    }
    cmd
}

/// 执行启动。Unix 用 exec 替换进程（信号和退出码透传，成功就不返回）；
/// Windows 先 spawn，等待后再透传退出码。
pub fn exec(plan: &LaunchPlan, args: &[String]) -> Result<i32> {
    let mut cmd = build_command(plan, args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec(); // 成功则不返回
        Err(Error::Io {
            context: format!("failed to exec {}", plan.program.display()),
            source: err,
        })
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(io_ctx(format!(
            "failed to spawn {}",
            plan.program.display()
        )))?;
        Ok(status.code().unwrap_or(1))
    }
}
