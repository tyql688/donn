//! Donn facade：UI 唯一的操作入口。
//!
//! 每个写操作都走同一条路：改 spec、render、reconcile、原子落盘、更新 footprint。
//! 读操作从 spec 和 preset 推导，意图从不反向从 settings.json 解析。

mod alias;
mod config;
mod create;
mod list;
mod remove;
mod update;

pub use config::{ConfigChange, ConfigReport};
pub use create::{CreateReceipt, ProfileDraft};
pub use list::{ProfileCard, ProfileView};
pub use update::{Drift, DriftKind, SpecChange, SyncReport};

use std::path::{Path, PathBuf};

use crate::config::GlobalConfig;
use crate::error::{Error, Result, io_ctx};
use crate::home::DonnHome;
use crate::launch::{self, LaunchPlan};
use crate::preset::PresetCatalog;
use crate::session::{self, LiveCheck};
use crate::spec::{Isolation, ProfileSpec};

pub struct Donn {
    home: DonnHome,
    cli_path: PathBuf,
}

impl Donn {
    pub fn open() -> Result<Self> {
        Self::with_home(DonnHome::discover()?)
    }

    /// 自定义根目录构造。wrapper 里记的 donn 路径取当前可执行文件。
    pub fn with_home(home: DonnHome) -> Result<Self> {
        let cli_path = std::env::current_exe()
            .and_then(std::path::absolute)
            .map_err(io_ctx("failed to resolve the donn executable path"))?;
        Ok(Self { home, cli_path })
    }

    pub fn home(&self) -> &DonnHome {
        &self.home
    }

    /// 现场读盘：手改 config.toml 对下一次调用立即可见。
    pub fn config(&self) -> Result<GlobalConfig> {
        GlobalConfig::load(&self.home)
    }

    pub fn cli_path(&self) -> &Path {
        &self.cli_path
    }

    pub fn bin_dir(&self) -> Result<PathBuf> {
        Ok(self.config()?.resolve_bin_dir(&self.home))
    }

    /// 首次运行尽量生成带注释的 config.toml 模板；失败不阻塞只读操作。
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

    /// 加载 spec；不存在时列出可用的 profile。
    pub fn spec(&self, name: &str) -> Result<ProfileSpec> {
        if !self.exists(name) {
            return Err(self.not_found(name)?);
        }
        ProfileSpec::load_named(&self.home.spec_file(name), name)
    }

    pub fn launch_plan(&self, name: &str) -> Result<LaunchPlan> {
        if !self.exists(name) {
            return Err(self.not_found(name)?);
        }
        launch::prepare(&self.home, &self.config()?, name)
    }

    /// 构造 ProfileNotFound；枚举 profile 目录失败时把 IO 错误传出去，
    /// 不用空列表冒充「还没有任何 profile」。
    pub(super) fn not_found(&self, name: &str) -> Result<Error> {
        Ok(Error::ProfileNotFound {
            name: name.to_string(),
            available: self.profile_names()?,
        })
    }

    /// 运行中 session 探测（删除前的安全检查）。只探测 profile 自己的 `claude/` 目录；
    /// shared 模式的 session 跑在 `~/.claude` 上，不探测这个目录。该模式下删除不会破坏
    /// 运行中的 session（env 已经注入进程），拦截反而会误伤与本 profile 无关的主配置 session。
    /// spec 读不出来时仍按保守策略探测。
    pub fn live_check(&self, name: &str) -> LiveCheck {
        let spec = self.spec(name).ok();
        if !needs_live_probe(spec.as_ref()) {
            return LiveCheck::NotRunning;
        }
        session::probe(&self.home.claude_config_dir(name))
    }

    pub(super) fn write_lock(&self) -> Result<crate::fsx::WriteLock> {
        crate::fsx::lock_writes(&self.home.lock_file())
    }
}

fn needs_live_probe(spec: Option<&ProfileSpec>) -> bool {
    !matches!(spec, Some(spec) if spec.isolation == Isolation::Shared)
}
