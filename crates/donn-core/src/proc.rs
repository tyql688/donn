//! 带超时的子进程执行：超时即杀掉，绝不挂住调用方。
//! stdout 在进程退出后才读，只适合输出小于管道缓冲（约 64K）的命令——现有调用方是
//! `claude --version` 和 `lsof -t`。

use std::ffi::OsStr;
use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// 运行并等待；返回退出状态与 stdout。spawn 失败、超时或读取失败都返回 `None`。
pub fn run_with_timeout(
    program: &OsStr,
    args: &[&OsStr],
    timeout: Duration,
) -> Option<(ExitStatus, String)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    Some((status, stdout))
}
