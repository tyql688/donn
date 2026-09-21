//! 运行中会话探测（best-effort）：删除 profile 前的安全检查。
//! Unix 用 `lsof +D <config_dir>`；不可用/超时/Windows 一律 `Unavailable`，
//! 调用方据此改用二次确认文案，绝不因探测失败而误删或卡死。

use std::path::Path;

/// 探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCheck {
    Running,
    NotRunning,
    Unavailable,
}

/// 检查 `config_dir` 下是否有进程持有打开的文件。
pub fn probe(config_dir: &Path) -> LiveCheck {
    #[cfg(unix)]
    {
        probe_with(std::ffi::OsStr::new("lsof"), config_dir)
    }
    #[cfg(not(unix))]
    {
        let _ = config_dir;
        LiveCheck::Unavailable
    }
}

#[cfg(unix)]
fn probe_with(program: &std::ffi::OsStr, config_dir: &Path) -> LiveCheck {
    use std::ffi::OsStr;
    // lsof +D 会遍历整个目录树，大目录可能很慢：超时杀掉，按 Unavailable 处理。
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
    let args = [OsStr::new("-t"), OsStr::new("+D"), config_dir.as_os_str()];
    match crate::proc::run_with_timeout(program, &args, TIMEOUT) {
        None => LiveCheck::Unavailable,
        // macOS lsof 可能已经输出 PID 却仍退出 1；实际 PID 输出才是事实来源。
        Some((_, stdout)) if stdout.trim().is_empty() => LiveCheck::NotRunning,
        Some(_) => LiveCheck::Running,
    }
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn script(dir: &TempDir, body: &str) -> std::path::PathBuf {
        let path = dir.path().join("fake-lsof");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn empty_dir_is_not_running() {
        let dir = TempDir::new().unwrap();
        // lsof 存在时应报 NotRunning；无 lsof 的环境报 Unavailable，两者都可接受
        let result = probe(dir.path());
        assert_ne!(result, LiveCheck::Running);
    }

    #[test]
    fn pid_output_wins_over_exit_status() {
        let dir = TempDir::new().unwrap();
        let command = script(&dir, "printf '12345\\n'; exit 1");
        assert_eq!(
            probe_with(command.as_os_str(), dir.path()),
            LiveCheck::Running
        );
    }

    #[test]
    fn empty_or_whitespace_output_is_not_running() {
        let dir = TempDir::new().unwrap();
        for body in ["exit 0", "printf '  \\n'; exit 0"] {
            let command = script(&dir, body);
            assert_eq!(
                probe_with(command.as_os_str(), dir.path()),
                LiveCheck::NotRunning
            );
        }
    }
}
