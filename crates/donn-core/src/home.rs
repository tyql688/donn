//! `~/.donn` 磁盘布局与平台差异。所有路径计算集中于此。
//!
//! ```text
//! ~/.donn/
//! ├── config.toml            全局配置
//! ├── profiles/<name>/
//! │   ├── profile.toml       ProfileSpec —— donn 意图的唯一来源
//! │   ├── shared-settings.json  shared 隔离的 --settings overlay（仅该模式生成）
//! │   └── claude/            该 profile 的 CLAUDE_CONFIG_DIR
//! └── presets.d/             用户自定义 preset
//! ```

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// donn 的全部磁盘位置。测试可用任意根目录构造。
#[derive(Debug, Clone)]
pub struct DonnHome {
    root: PathBuf,
    user_home: PathBuf,
}

impl DonnHome {
    /// 生产构造：`$DONN_HOME` 覆盖（供测试/高级用户），默认 `~/.donn`。
    pub fn discover() -> Result<Self> {
        let user_home = dirs::home_dir()
            .ok_or_else(|| Error::Internal("cannot determine home directory".into()))?;
        let root = match std::env::var_os("DONN_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => user_home.join(".donn"),
        };
        Ok(Self { root, user_home })
    }

    /// 测试构造：donn 根与用户 home 都指到临时目录下。
    pub fn for_test(root: &Path) -> Self {
        Self {
            root: root.join(".donn"),
            user_home: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn user_home(&self) -> &Path {
        &self.user_home
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir().join(name)
    }

    pub fn spec_file(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("profile.toml")
    }

    /// `CLAUDE_CONFIG_DIR` 指向的目录。
    pub fn claude_config_dir(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("claude")
    }

    pub fn settings_file(&self, name: &str) -> PathBuf {
        self.claude_config_dir(name).join("settings.json")
    }

    pub fn claude_json_file(&self, name: &str) -> PathBuf {
        self.claude_config_dir(name).join(".claude.json")
    }

    /// shared 模式的 `--settings` overlay（内容见 [`crate::render::shared_overlay`]）。
    pub fn shared_settings_file(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("shared-settings.json")
    }

    /// 一个 profile 的身份文件：「保留会话」式删除只删这些，create 失败回滚也只动这些。
    pub fn identity_files(&self, name: &str) -> [PathBuf; 4] {
        [
            self.spec_file(name),
            self.settings_file(name),
            self.claude_json_file(name),
            self.shared_settings_file(name),
        ]
    }

    pub fn presets_dir(&self) -> PathBuf {
        self.root.join("presets.d")
    }

    /// 跨进程写事务锁。TUI/CLI 共用，避免读改写互相覆盖。
    pub fn lock_file(&self) -> PathBuf {
        self.root.join(".write.lock")
    }

    /// 展示用路径：用户 home 前缀缩成 `~`。
    pub fn tilde(&self, path: &Path) -> String {
        match path.strip_prefix(&self.user_home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }

    /// 用户主配置 `~/.claude/`（donn 只读，绝不写）。
    pub fn main_claude_dir(&self) -> PathBuf {
        self.user_home.join(".claude")
    }

    /// wrapper 落盘目录：macOS/Linux `~/.local/bin`，Windows `~/.donn/bin`。
    pub fn default_bin_dir(&self) -> PathBuf {
        if cfg!(windows) {
            self.root.join("bin")
        } else {
            self.user_home.join(".local").join("bin")
        }
    }
}

/// profile 名 / alias 名校验：`[a-z][a-z0-9-]*`。
pub fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_table() {
        let ok = ["zai", "z", "glm-4-air", "a1", "kimi-k2"];
        let bad = ["", "1zai", "-zai", "Zai", "z ai", "z_ai", "z.ai", "中文"];
        for name in ok {
            assert!(validate_name(name).is_ok(), "expected ok: {name:?}");
        }
        for name in bad {
            assert!(validate_name(name).is_err(), "expected err: {name:?}");
        }
    }

    #[test]
    fn layout() {
        let h = DonnHome::for_test(Path::new("/tmp/x"));
        assert_eq!(
            h.settings_file("zai"),
            PathBuf::from("/tmp/x/.donn/profiles/zai/claude/settings.json")
        );
        assert_eq!(
            h.claude_json_file("zai"),
            PathBuf::from("/tmp/x/.donn/profiles/zai/claude/.claude.json")
        );
        assert_eq!(
            h.spec_file("zai"),
            PathBuf::from("/tmp/x/.donn/profiles/zai/profile.toml")
        );
        assert_eq!(h.main_claude_dir(), PathBuf::from("/tmp/x/.claude"));
        assert_eq!(h.tilde(&h.main_claude_dir()), "~/.claude");
        assert_eq!(h.tilde(Path::new("/etc/hosts")), "/etc/hosts");
        assert_eq!(h.tilde(Path::new("/tmp/xbin/donn")), "/tmp/xbin/donn");
        assert_eq!(h.tilde(Path::new("/tmp/x")), "~");
    }
}
