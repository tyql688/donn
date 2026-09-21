//! donn-core：零 UI 依赖的纯业务逻辑库。不能破的边界在 docs/RULES.md。
//!
//! 领域模型：**Spec → Render → Reconcile**。
//! [`spec::ProfileSpec`]（profile.toml）是 donn 意图的唯一来源；
//! [`render`] 由它生成 donn 拥有的配置内容；[`reconcile`] 把内容合并进
//! settings.json / .claude.json，未知字段原样穿透。UI 只调 [`ops::Donn`]。

pub mod claude;
pub mod config;
pub mod doctor;
pub mod error;
pub mod fsx;
pub mod home;
pub mod keys;
pub mod knobs;
pub mod launch;
pub mod ops;
pub mod preset;
pub mod proc;
pub mod reconcile;
pub mod render;
pub mod secret;
pub mod session;
pub mod spec;
pub mod timefmt;
pub mod wrapper;

pub use config::{Defaults, GlobalConfig};
pub use error::{Error, Result};
pub use home::DonnHome;
pub use keys::{EFFORT_SETTING_LEVELS, Effort, ModelSlot, SlotMap};
pub use knobs::Knobs;
pub use ops::{
    ConfigChange, ConfigReport, CreateReceipt, Donn, Drift, DriftKind, ProfileCard, ProfileDraft,
    ProfileView, SpecChange, SyncReport,
};
pub use preset::{AuthMode, ModelChoice, Preset, PresetCatalog};
pub use secret::{KeyState, Secret};
pub use session::LiveCheck;
pub use spec::{Isolation, ProfileSpec};
