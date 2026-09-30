//! `donn run <name> [-- args]`：退出码透传 claude；错误必须含下一步动作提示。

use donn_core::Donn;

use super::open_donn;

pub fn execute(name: &str, args: &[String]) -> i32 {
    match open_donn() {
        Ok(donn) => launch(&donn, name, args),
        Err(code) => code,
    }
}

/// sync → 启动计划 → exec claude。TUI 的 Enter 启动也走这里。
pub fn launch(donn: &Donn, name: &str, args: &[String]) -> i32 {
    // 启动前自愈手改漂移和版本升级后的渲染变化。失败不阻塞启动：
    // 已有落盘配置仍可能完全可用。
    match donn.sync(name) {
        Ok(report) if !report.overwritten.is_empty() => eprintln!(
            "note: restored donn-managed settings: {}",
            report.overwritten.join(", ")
        ),
        Ok(_) => {}
        // 没有这个 profile：下面取启动计划时会报同一个错，这里不重复
        Err(donn_core::Error::ProfileNotFound { .. }) => {}
        Err(e) => {
            eprintln!("warn: config refresh failed, launching with existing files: {e}");
        }
    }
    let plan = match donn.launch_plan(name) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if plan.overlay_overridden(args) {
        eprintln!(
            "note: your --settings replaces donn's; this session skips the profile's model, permission mode and global settings"
        );
    }
    match donn_core::launch::exec(&plan, args) {
        Ok(code) => code, // Windows: 透传退出码；Unix 的 exec 成功不返回
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
