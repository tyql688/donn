//! `donn doctor`：CI/脚本可用，非 0 退出码表示有 ✗ 项。

use super::open_donn;

pub fn execute() -> i32 {
    let donn = match open_donn() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let checks = donn_core::doctor::run(&donn);
    print!("{}", donn_core::doctor::render_text(&checks));
    if donn_core::doctor::all_ok(&checks) {
        0
    } else {
        1
    }
}
