//! `donn run <name> [-- args]`：退出码透传 claude；错误必须含下一步动作提示。

use super::open_donn;

pub fn execute(name: &str, args: &[String]) -> i32 {
    let donn = match open_donn() {
        Ok(s) => s,
        Err(code) => return code,
    };
    // 启动前自愈手改漂移和版本升级后的渲染变化。失败不阻塞启动：
    // 已有落盘配置仍可能完全可用。
    match donn.sync(name) {
        Ok(report) if !report.overwritten.is_empty() => eprintln!(
            "note: restored donn-managed settings: {}",
            report.overwritten.join(", ")
        ),
        Ok(_) => {}
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
    for flag in plan.overridden_flags(args) {
        eprintln!(
            "note: your {flag} replaces donn's shared-mode overlay; the profile's model, permission mode and global settings are not applied this session"
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
