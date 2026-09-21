pub mod doctor;
pub mod list;
pub mod run;

/// 打开 Donn，失败时打印错误并返回退出码 1。
pub fn open_donn() -> Result<donn_core::Donn, i32> {
    donn_core::Donn::open().map_err(|e| {
        eprintln!("error: {e}");
        1
    })
}
