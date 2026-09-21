//! 别名管理：wrapper 文件 + spec 记录同步增删。
//! 两侧都保证一致性：一侧落盘失败即回滚另一侧，绝不留下 spec 与文件系统不符的状态。

use std::path::PathBuf;

use crate::error::Result;
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
                // spec 落盘失败：撤掉刚生成的 wrapper，避免 spec 与文件系统脱节
                let _ = wrapper::remove(&bin_dir, alias, name);
                return Err(e);
            }
        }
        Ok(path)
    }

    pub fn remove_alias(&self, name: &str, alias: &str) -> Result<()> {
        let _lock = self.write_lock()?;
        let mut spec = self.spec(name)?;
        if !spec
            .wrapper
            .aliases
            .iter()
            .any(|registered| registered == alias)
        {
            return Err(crate::error::Error::InvalidInput(format!(
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

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::home::DonnHome;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn donn_with_profile(root: &TempDir) -> Donn {
        let donn = Donn::with_home(DonnHome::for_test(root.path())).unwrap();
        donn.create(&crate::ops::ProfileDraft {
            name: "zai".into(),
            preset: "official".into(),
            ..Default::default()
        })
        .unwrap();
        donn
    }

    /// 把 profile 目录切成只读：spec.save 的临时文件创建失败，wrapper 所在 bin 目录不受影响。
    /// root 照样写得进去，测不出——返回 None 让调用方跳过。
    fn make_spec_dir_readonly(root: &TempDir) -> Option<std::path::PathBuf> {
        let dir = DonnHome::for_test(root.path()).profile_dir("zai");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".probe");
        if std::fs::write(&probe, "").is_ok() {
            let _ = std::fs::remove_file(probe);
            return None;
        }
        Some(dir)
    }

    #[test]
    fn add_alias_rolls_back_wrapper_when_spec_save_fails() {
        let dir = TempDir::new().unwrap();
        let donn = donn_with_profile(&dir);
        let bin_dir = donn.bin_dir().unwrap();
        let Some(profile_dir) = make_spec_dir_readonly(&dir) else {
            return;
        };

        let result = donn.add_alias("zai", "zz");
        std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(
            !crate::wrapper::wrapper_path(&bin_dir, "zz").exists(),
            "spec 落盘失败时 wrapper 必须被回滚"
        );
        assert_eq!(donn.spec("zai").unwrap().wrapper.aliases, vec!["zai"]);
    }

    #[test]
    fn remove_alias_restores_wrapper_when_spec_save_fails() {
        let dir = TempDir::new().unwrap();
        let donn = donn_with_profile(&dir);
        donn.add_alias("zai", "z").unwrap();
        let bin_dir = donn.bin_dir().unwrap();
        let Some(profile_dir) = make_spec_dir_readonly(&dir) else {
            return;
        };

        let result = donn.remove_alias("zai", "z");
        std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(
            crate::wrapper::wrapper_path(&bin_dir, "z").exists(),
            "spec 落盘失败时 wrapper 必须被恢复"
        );
        assert_eq!(
            donn.spec("zai").unwrap().wrapper.aliases,
            vec!["zai", "z"],
            "盘上 spec 保持原样"
        );
    }
}
