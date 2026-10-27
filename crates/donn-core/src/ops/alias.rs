//! alias 管理：wrapper 文件和 spec 记录同步增删。
//! 两边保持一致：一侧落盘失败就回滚另一侧，绝不留下 spec 和文件系统对不上的状态。

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::timefmt;
use crate::wrapper;

use super::Donn;

impl Donn {
    pub fn add_alias(&self, name: &str, alias: &str) -> Result<PathBuf> {
        let _lock = self.write_lock()?;
        let mut spec = self.spec(name)?;
        let bin_dir = self.bin_dir()?;
        wrapper::check_alias_conflict(&bin_dir, alias, name)?;
        let path = wrapper::generate(&bin_dir, alias, name, self.cli_path())?;
        if !spec.wrapper.aliases.iter().any(|a| a == alias) {
            spec.wrapper.aliases.push(alias.to_string());
            spec.updated_at = timefmt::now_rfc3339();
            if let Err(e) = spec.save(&self.home.spec_file(name)) {
                // spec 落盘失败：撤掉刚生成的 wrapper，避免 spec 和文件系统脱节
                let _ = wrapper::remove(&bin_dir, alias, name);
                return Err(e);
            }
        }
        Ok(path)
    }

    pub fn remove_alias(&self, name: &str, alias: &str) -> Result<()> {
        let _lock = self.write_lock()?;
        let mut spec = self.spec(name)?;
        if !spec.wrapper.aliases.iter().any(|a| a == alias) {
            return Err(Error::InvalidInput(format!(
                "alias '{alias}' is not registered for profile '{name}'"
            )));
        }
        let bin_dir = self.bin_dir()?;
        wrapper::remove(&bin_dir, alias, name)?;
        spec.wrapper.aliases.retain(|a| a != alias);
        spec.updated_at = timefmt::now_rfc3339();
        if let Err(e) = spec.save(&self.home.spec_file(name)) {
            // spec 落盘失败：恢复 wrapper，保持 spec 与文件系统一致
            let _ = wrapper::generate(&bin_dir, alias, name, self.cli_path());
            return Err(e);
        }
        Ok(())
    }
}
