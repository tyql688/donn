//! Donn facade：UI 唯一的操作入口。
//!
//! 每个写操作 = 改 spec → render → reconcile → 原子落盘 → 更新 footprint。
//! 读操作从 spec + preset 推导，意图从不反向从 settings.json 解析。

mod alias;
mod config_ops;
mod create;
mod list;
mod remove;
mod update;

pub use config_ops::{ConfigChange, ConfigReport};
pub use create::{CreateReceipt, ProfileDraft};
pub use list::{ProfileCard, ProfileView};
pub use update::{Drift, DriftKind, SpecChange, SyncReport};

use std::path::PathBuf;

use crate::config::GlobalConfig;
use crate::error::{Error, Result, io_ctx};
use crate::home::DonnHome;
use crate::launch::{self, LaunchPlan};
use crate::preset::PresetCatalog;
use crate::session::{self, LiveCheck};
use crate::spec::ProfileSpec;

pub struct Donn {
    home: DonnHome,
    cli_path: PathBuf,
}

impl Donn {
    pub fn open() -> Result<Self> {
        let home = DonnHome::discover()?;
        let cli_path =
            std::env::current_exe().map_err(io_ctx("failed to resolve donn executable path"))?;
        Self::with_home_and_cli(home, &cli_path)
    }

    /// 测试/自定义根目录构造。
    pub fn with_home(home: DonnHome) -> Result<Self> {
        let cli_path =
            std::env::current_exe().map_err(io_ctx("failed to resolve test executable path"))?;
        Self::with_home_and_cli(home, &cli_path)
    }

    fn with_home_and_cli(home: DonnHome, cli_path: &std::path::Path) -> Result<Self> {
        let cli_path = std::path::absolute(cli_path)
            .map_err(io_ctx("failed to resolve the donn executable path"))?;
        if !cli_path.is_file() {
            return Err(Error::InvalidInput(format!(
                "donn CLI is not a file: {}",
                cli_path.display()
            )));
        }
        Ok(Self { home, cli_path })
    }

    pub fn home(&self) -> &DonnHome {
        &self.home
    }

    /// 现场读盘：手改 config.toml 对下一次调用立即可见。
    pub fn config(&self) -> Result<GlobalConfig> {
        GlobalConfig::load(&self.home)
    }

    pub fn cli_path(&self) -> &std::path::Path {
        &self.cli_path
    }

    pub fn bin_dir(&self) -> Result<PathBuf> {
        Ok(GlobalConfig::load(&self.home)?.resolve_bin_dir(&self.home))
    }

    /// 首次运行尽力生成带注释的 config.toml 模板；失败不阻塞只读操作。
    pub fn ensure_config_template(&self) {
        if let Ok(_lock) = self.write_lock() {
            let _ = crate::config::ensure_config_doc(&self.home);
        }
    }

    pub fn presets(&self) -> PresetCatalog {
        PresetCatalog::load(&self.home)
    }

    pub fn profile_names(&self) -> Result<Vec<String>> {
        let dir = self.home.profiles_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(io_ctx(format!("failed to read {}", dir.display())))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| self.home.spec_file(name).is_file())
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.home.spec_file(name).is_file()
    }

    /// 加载 spec；不存在时报错并列出可用 profile。
    pub fn spec(&self, name: &str) -> Result<ProfileSpec> {
        if !self.exists(name) {
            return Err(self.not_found(name)?);
        }
        ProfileSpec::load(&self.home.spec_file(name))
    }

    pub fn launch_plan(&self, name: &str) -> Result<LaunchPlan> {
        if !self.exists(name) {
            return Err(self.not_found(name)?);
        }
        let config = GlobalConfig::load(&self.home)?;
        launch::prepare(&self.home, &config, name)
    }

    /// 构造 ProfileNotFound；枚举 profile 目录失败时传播 IO 错误，
    /// 不用空列表冒充「还没有任何 profile」。
    pub(super) fn not_found(&self, name: &str) -> Result<Error> {
        Ok(Error::ProfileNotFound {
            name: name.to_string(),
            available: self.profile_names()?,
        })
    }

    /// 运行中会话探测（删除前安全检查）。只探测 profile 自己的 claude/ 目录：
    /// shared 模式会话跑在 ~/.claude 上，恒报 NotRunning——设计使然，
    /// 该模式下删除对运行中会话无破坏（env 已注入进程），拦截反而会误伤
    /// 与本 profile 无关的主配置会话。
    pub fn live_check(&self, name: &str) -> LiveCheck {
        session::probe(&self.home.claude_config_dir(name))
    }

    pub(super) fn write_lock(&self) -> Result<crate::fsx::WriteLock> {
        crate::fsx::lock_writes(&self.home.lock_file())
    }
}
