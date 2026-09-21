//! 创建 profile：校验 → render/reconcile 落盘 → wrapper。中途失败整体回滚。

use std::path::PathBuf;

use crate::error::{Error, Result, io_ctx};
use crate::home::validate_name;
use crate::keys::SlotMap;
use crate::secret::Secret;
use crate::spec::{AuthSpec, Intent, Isolation, ProfileSpec, WrapperSpec};
use crate::timefmt;
use crate::wrapper;

use super::Donn;
use super::update::SecretSource;

/// Add 表单收集的创建请求。
#[derive(Debug, Clone, Default)]
pub struct ProfileDraft {
    pub name: String,
    pub preset: String,
    pub key: Option<Secret>,
    /// 覆盖 preset base_url（custom preset 必填）。
    pub base_url: Option<String>,
    /// 模型槽位覆盖。
    pub models: SlotMap,
    /// 自定义模型的上下文窗口：模型 id → token。
    pub model_windows: std::collections::BTreeMap<String, u64>,
    /// 附加 env（覆盖 preset.env 同名键）。
    pub env: Vec<(String, String)>,
    pub isolation: Isolation,
    /// 命令别名；空则默认 `[name]`。
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateReceipt {
    pub wrapper_paths: Vec<PathBuf>,
    pub bin_dir: PathBuf,
    pub bin_dir_in_path: bool,
}

impl Donn {
    pub fn create(&self, draft: &ProfileDraft) -> Result<CreateReceipt> {
        let _lock = self.write_lock()?;
        validate_name(&draft.name)?;
        if self.exists(&draft.name) {
            return Err(Error::ProfileExists(draft.name.clone()));
        }
        let catalog = self.presets();
        let preset = catalog.get(&draft.preset)?;
        let bin_dir = self.bin_dir()?;
        for (key, value) in &draft.env {
            crate::keys::validate_env_entry(key, value)?;
        }

        let aliases: Vec<String> = if draft.aliases.is_empty() {
            vec![draft.name.clone()]
        } else {
            draft.aliases.clone()
        };
        for alias in &aliases {
            wrapper::check_alias_conflict(&bin_dir, alias, &draft.name)?;
        }

        let profile_dir = self.home.profile_dir(&draft.name);
        // 同名目录可能是上次「保留会话」删除留下的：回滚绝不整删已有目录，身份文件
        // 恢复到创建前的样子（原来有的写回原字节，原来没有的删掉）
        let dir_preexisted = profile_dir.exists();
        let mut before = Vec::new();
        for file in self.home.identity_files(&draft.name) {
            let original = match std::fs::read(&file) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                // 在但读不了：没法保证回滚得回去，干脆不开始
                Err(e) => return Err(io_ctx(format!("failed to read {}", file.display()))(e)),
            };
            before.push((original, file));
        }
        let result = self.create_inner(draft, preset.auth_mode, &aliases, &bin_dir);
        if result.is_err() {
            if dir_preexisted {
                for (original, file) in before {
                    match original {
                        Some(bytes) => drop(crate::fsx::write_atomic(&file, &bytes)),
                        None => drop(std::fs::remove_file(&file)),
                    }
                }
            } else {
                let _ = std::fs::remove_dir_all(&profile_dir);
            }
            for alias in &aliases {
                let _ = wrapper::remove(&bin_dir, alias, &draft.name);
            }
        }
        result
    }

    fn create_inner(
        &self,
        draft: &ProfileDraft,
        auth_mode: crate::preset::AuthMode,
        aliases: &[String],
        bin_dir: &std::path::Path,
    ) -> Result<CreateReceipt> {
        let claude_dir = self.home.claude_config_dir(&draft.name);
        std::fs::create_dir_all(&claude_dir)
            .map_err(io_ctx(format!("failed to create {}", claude_dir.display())))?;

        let now = timefmt::now_rfc3339();
        let mut spec = ProfileSpec {
            name: draft.name.clone(),
            preset: draft.preset.clone(),
            created_at: now.clone(),
            updated_at: now,
            auth: AuthSpec { mode: auth_mode },
            isolation: draft.isolation,
            intent: Intent {
                base_url: draft.base_url.clone().filter(|u| !u.is_empty()),
                models: draft.models.clone(),
                model_windows: draft.model_windows.clone(),
                env: draft.env.iter().cloned().collect(),
            },
            wrapper: WrapperSpec {
                aliases: aliases.to_vec(),
            },
            ..Default::default()
        };

        self.regenerate(None, &mut spec, SecretSource::Explicit(draft.key.clone()))?;

        // wrapper 在创建成功时自动生成
        let mut wrapper_paths = Vec::new();
        for alias in aliases {
            wrapper_paths.push(wrapper::generate(
                bin_dir,
                alias,
                &draft.name,
                self.cli_path(),
            )?);
        }

        Ok(CreateReceipt {
            wrapper_paths,
            bin_dir: bin_dir.to_path_buf(),
            bin_dir_in_path: wrapper::bin_dir_in_path(bin_dir),
        })
    }
}
