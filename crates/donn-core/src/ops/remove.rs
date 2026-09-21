//! 删除 profile：别名 wrapper + profile 目录。
//! full 模式可选保留会话（只删身份，claude/ 会话数据原地不动）。
//! 运行中会话检出则拒绝（force 跳过）。

use crate::error::{Error, Result, io_ctx};
use crate::session::LiveCheck;
use crate::spec::Isolation;
use crate::wrapper;

use super::Donn;

impl Donn {
    /// `keep_sessions`：full 模式下只删身份（spec、含 key 的 settings.json/.claude.json、
    /// 别名），`claude/` 里的会话数据原地保留——同名重建 profile 自动接上。
    /// shared 模式会话本就在 ~/.claude，该参数无效果（目录整删）。
    pub fn remove(&self, name: &str, force: bool, keep_sessions: bool) -> Result<()> {
        let _lock = self.write_lock()?;
        let spec = match self.spec(name) {
            Ok(spec) => Some(spec),
            Err(Error::ProfileNotFound { .. }) => return Err(self.not_found(name)?),
            // profile.toml 坏了也必须能删，否则 TUI 会进入无法恢复的死胡同。
            Err(_) => None,
        };
        if !force && self.live_check(name) == LiveCheck::Running {
            return Err(Error::ProfileInUse(name.to_string()));
        }
        let bin_dir = self.bin_dir()?;
        let aliases = match &spec {
            Some(spec) => spec.wrapper.aliases.clone(),
            None => wrappers_targeting(&bin_dir, name),
        };
        for alias in &aliases {
            match wrapper::remove(&bin_dir, alias, name) {
                Ok(()) => {}
                // spec 可能被手改或 wrapper 被别的 profile 接管；一律保留目标文件。
                Err(
                    Error::NotOwnedByDonn(_) | Error::AliasInUse { .. } | Error::AliasConflict(..),
                ) => {}
                Err(e) => return Err(e),
            }
        }
        // spec 不可读时 isolation 未知：按 full 处理最保守，只清身份文件，
        // 留下会话目录不会破坏用户数据。
        if keep_sessions
            && spec
                .as_ref()
                .is_none_or(|spec| spec.isolation == Isolation::Full)
        {
            for file in self.home.identity_files(name) {
                if file.exists() {
                    std::fs::remove_file(&file)
                        .map_err(io_ctx(format!("failed to remove {}", file.display())))?;
                }
            }
            return Ok(());
        }
        let dir = self.home.profile_dir(name);
        std::fs::remove_dir_all(&dir).map_err(io_ctx(format!("failed to remove {}", dir.display())))
    }
}

/// spec 不可读时，以 bin 目录里 wrapper 的实际启动目标为准回收别名。
fn wrappers_targeting(bin_dir: &std::path::Path, name: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(bin_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            if !wrapper::is_donn_wrapper(&path).unwrap_or(false)
                || wrapper::wrapper_target(&path).ok().flatten().as_deref() != Some(name)
            {
                return None;
            }
            let file_name = entry.file_name().into_string().ok()?;
            #[cfg(windows)]
            let file_name = file_name
                .strip_suffix(".cmd")
                .or_else(|| file_name.strip_suffix(".CMD"))
                .unwrap_or(&file_name)
                .to_string();
            Some(file_name)
        })
        .collect()
}
