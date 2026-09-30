//! 启动链路：
//! 读 settings.json → 设 `CLAUDE_CONFIG_DIR` + 注入 env → exec claude。
//!
//! env 优先级（从左到右递增）：进程环境 ← settings.json env ← CLAUDE_CONFIG_DIR。
//! settings.json 必须在启动 claude 之前注入进程环境（onboarding 阶段读的是进程 env）。

use std::path::PathBuf;
use std::process::Command;

use crate::claude;
use crate::config::GlobalConfig;
use crate::error::{Error, Result, io_ctx};
use crate::home::DonnHome;
use crate::spec::{Isolation, ProfileSpec};

/// 已解析、可执行的启动计划。纯数据，便于测试与 TUI 复用。
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub program: PathBuf,
    /// 注入的 env（独立模式含最后强制的 CLAUDE_CONFIG_DIR）。
    pub env: Vec<(String, String)>,
    /// shared 模式：sync 生成的 overlay 文件，经 `--settings` 带进会话。
    pub settings_overlay: Option<PathBuf>,
}

impl LaunchPlan {
    /// 用户自己传了 `--settings`（`--settings v` 或 `--settings=v`）。Claude 的 `--settings`
    /// 只认一个值，以用户的为准：donn 的整份 overlay 本次不生效。
    pub fn overlay_overridden(&self, args: &[String]) -> bool {
        self.settings_overlay.is_some()
            && args
                .iter()
                .any(|a| a == "--settings" || a.starts_with("--settings="))
    }
}

/// 定位 claude 二进制：config.toml 显式路径 > which("claude")。
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

/// 组装启动计划。`name` 对应 profile 必须已存在（由调用方校验）。
pub fn prepare(home: &DonnHome, config: &GlobalConfig, name: &str) -> Result<LaunchPlan> {
    let spec = ProfileSpec::load(&home.spec_file(name))?;
    let settings = claude::read_json(&home.settings_file(name))?;

    let mut env = claude::SettingsView::new(&settings).env_entries();
    env.retain(|(k, _)| k != "CLAUDE_CONFIG_DIR");
    for (key, value) in &env {
        crate::keys::validate_env_entry(key, value)?;
    }

    // shared 模式：模型选择、settings 类旋钮、权限模式全部经 --settings overlay 文件
    // 压过 ~/.claude 的持久化配置（文件由 sync 生成），命令行只指向文件本身。
    let settings_overlay = (spec.isolation == Isolation::Shared)
        .then(|| home.shared_settings_file(name))
        .filter(|overlay| overlay.is_file());

    // 独立：CLAUDE_CONFIG_DIR 最后强制设置，即使 settings env 出现同名键也以 donn 为准。
    // 共享：不动 CLAUDE_CONFIG_DIR，session/全局配置走 ~/.claude，渠道身份只由注入的 env 决定。
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

/// 组装子进程命令：先从继承环境剥掉本次没注入的托管键，再注入 settings env 与
/// CLAUDE_CONFIG_DIR。
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

/// 执行启动。Unix: exec 进程替换（信号/退出码透传，成功则不返回）；
/// Windows: spawn + 等待 + 透传退出码。
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn setup(dir: &std::path::Path, env: &serde_json::Value) -> (DonnHome, GlobalConfig) {
        let home = DonnHome::for_test(dir);
        let settings_path = home.settings_file("zai");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(&settings_path, json!({ "env": env }).to_string()).unwrap();
        std::fs::write(
            home.spec_file("zai"),
            "schema_version = 2\nname = \"zai\"\npreset = \"zai\"\n",
        )
        .unwrap();
        // 伪 claude 二进制
        let fake_claude = dir.join("claude-bin");
        std::fs::write(&fake_claude, "#!/bin/sh\n").unwrap();
        let config = GlobalConfig {
            claude_bin: Some(fake_claude.display().to_string()),
            ..Default::default()
        };
        (home, config)
    }

    #[test]
    fn plan_injects_settings_env_and_forces_config_dir() {
        let dir = TempDir::new().unwrap();
        let (home, config) = setup(
            dir.path(),
            &json!({
                "ANTHROPIC_AUTH_TOKEN": "sk-x",
                "CLAUDE_CONFIG_DIR": "/evil/override"
            }),
        );
        let plan = prepare(&home, &config, "zai").unwrap();
        let get = |k: &str| {
            plan.env
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("ANTHROPIC_AUTH_TOKEN").as_deref(), Some("sk-x"));
        // CLAUDE_CONFIG_DIR 以 donn 计算值为准，且是最后一项
        assert!(
            get("CLAUDE_CONFIG_DIR")
                .unwrap()
                .ends_with("profiles/zai/claude")
        );
        assert_eq!(plan.env.last().unwrap().0, "CLAUDE_CONFIG_DIR");
        assert_eq!(
            plan.env
                .iter()
                .filter(|(k, _)| k == "CLAUDE_CONFIG_DIR")
                .count(),
            1
        );
    }

    #[test]
    fn missing_claude_bin_config_is_reported() {
        let dir = TempDir::new().unwrap();
        let (home, _) = setup(dir.path(), &json!({}));
        let config = GlobalConfig {
            claude_bin: Some("/nonexistent/claude".into()),
            ..Default::default()
        };
        let err = prepare(&home, &config, "zai").unwrap_err();
        assert!(err.to_string().contains("missing file"), "{err}");
    }

    #[test]
    fn plan_rejects_os_invalid_environment_without_leaking_value() {
        let dir = TempDir::new().unwrap();
        let (home, config) = setup(dir.path(), &json!({"VALID_NAME": "secret\0must-not-leak"}));
        let err = prepare(&home, &config, "zai").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
        assert!(!err.to_string().contains("must-not-leak"));
    }

    #[test]
    fn build_command_scrubs_managed_keys_absent_from_the_plan() {
        let dir = TempDir::new().unwrap();
        let (home, config) = setup(
            dir.path(),
            &json!({"ANTHROPIC_AUTH_TOKEN": "sk-x", "USER_FREEFORM": "v"}),
        );
        let plan = prepare(&home, &config, "zai").unwrap();
        let cmd = build_command(&plan, &[]);
        let env_of = |k: &str| {
            cmd.get_envs()
                .find(|(key, _)| *key == std::ffi::OsStr::new(k))
                .map(|(_, v)| v.map(|v| v.to_os_string()))
        };
        // 计划内键原样注入
        assert_eq!(
            env_of("ANTHROPIC_AUTH_TOKEN").flatten().unwrap(),
            std::ffi::OsString::from("sk-x")
        );
        assert_eq!(
            env_of("USER_FREEFORM").flatten().unwrap(),
            std::ffi::OsString::from("v")
        );
        // full 模式 CLAUDE_CONFIG_DIR 在计划内 → set；不在计划内的托管键 → env_remove
        assert!(env_of("CLAUDE_CONFIG_DIR").flatten().is_some());
        assert_eq!(
            env_of("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
            Some(None),
            "render 未写的托管键必须被剥除（值为 None = env_remove）"
        );
        assert_eq!(
            env_of("ANTHROPIC_API_KEY"),
            Some(None),
            "认证键同样不得从 shell 继承"
        );
        assert_eq!(
            env_of("CLAUDE_CODE_USE_BEDROCK"),
            Some(None),
            "会改道的 shell 变量同样剥除"
        );
        // 非托管键完全不出现在命令环境里（继承原状，由操作系统决定）
        assert_eq!(env_of("SOME_UNMANAGED_SHELL_VAR"), None);
    }

    #[test]
    fn shared_plan_points_at_settings_overlay_and_respects_user_flag() {
        let dir = TempDir::new().unwrap();
        let (home, config) = setup(dir.path(), &json!({}));
        std::fs::write(
            home.spec_file("zai"),
            "schema_version = 2\nname = \"zai\"\npreset = \"zai\"\nisolation = \"shared\"\n",
        )
        .unwrap();
        // overlay 由 sync 生成；launch 单元测试里直接写一个最小文件模拟
        let overlay = home.shared_settings_file("zai");
        std::fs::write(&overlay, "{}\n").unwrap();

        let plan = prepare(&home, &config, "zai").unwrap();
        assert_eq!(plan.settings_overlay.as_deref(), Some(overlay.as_path()));
        let args: Vec<_> = build_command(&plan, &["--model".into()])
            .get_args()
            .map(|a| a.to_os_string())
            .collect();
        assert_eq!(
            args[..2],
            ["--settings".into(), overlay.clone().into_os_string()]
        );
        // 用户显式 --settings 时以用户的为准，并能报告出来
        assert!(plan.overlay_overridden(&["--settings=/mine.json".into()]));
        assert!(!plan.overlay_overridden(&["--model".into()]));
        let cmd = build_command(&plan, &["--settings".into(), "/mine.json".into()]);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
        assert_eq!(
            args,
            vec![
                std::ffi::OsString::from("--settings"),
                std::ffi::OsString::from("/mine.json"),
            ]
        );

        // overlay 不存在 → 不追加任何参数
        std::fs::remove_file(&overlay).unwrap();
        let plan = prepare(&home, &config, "zai").unwrap();
        assert!(plan.settings_overlay.is_none());
    }
}
