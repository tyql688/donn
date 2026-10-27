//! 运行中 session 探测（best-effort）：删除 profile 前的安全检查。
//! Unix 用 `lsof +D <config_dir>`；不可用、超时或 Windows 一律返回 `Unavailable`，
//! 调用方据此改用更保守的二次确认文案，绝不因为探测失败而误删或卡死。

use std::path::Path;

/// 探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCheck {
    Running,
    NotRunning,
    Unavailable,
}

/// 检查 `config_dir` 下有没有进程持有打开的文件。
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
        // macOS lsof 可能已经输出 PID 却仍退出 1；以实际 PID 输出为准。
        Some((_, stdout)) if stdout.trim().is_empty() => LiveCheck::NotRunning,
        Some(_) => LiveCheck::Running,
    }
}
