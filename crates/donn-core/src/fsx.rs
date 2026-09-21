//! 文件落盘纪律：temp + fsync + rename 原子写。写用户文件的唯一通道。
//! donn 写的内容都能从 spec 再生，不做备份。

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result, io_ctx};

/// 持有期间阻止其它 donn 进程进入写事务；关闭文件即自动解锁。
pub(crate) struct WriteLock {
    _file: File,
}

pub(crate) fn lock_writes(path: &Path) -> Result<WriteLock> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Internal(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent).map_err(io_ctx(format!(
        "failed to create directory {}",
        parent.display()
    )))?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(io_ctx(format!("failed to open {}", path.display())))?;
    // 锁竞争最多等约 5 秒，之后给调用方一个可操作错误，
    // 不能让启动前 sync 或 TUI 写操作无限挂住。
    let mut locked = false;
    for attempt in 0..100 {
        match file.try_lock() {
            Ok(()) => {
                locked = true;
                break;
            }
            Err(std::fs::TryLockError::WouldBlock) if attempt < 99 => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(std::fs::TryLockError::WouldBlock) => break,
            Err(std::fs::TryLockError::Error(source)) => {
                return Err(Error::Io {
                    context: format!("failed to lock {}", path.display()),
                    source,
                });
            }
        }
    }
    if !locked {
        return Err(Error::Io {
            context: format!("failed to lock {} after 5 seconds", path.display()),
            source: std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "another donn process is still writing",
            ),
        });
    }
    Ok(WriteLock { _file: file })
}

/// 原子写。tmp 文件与目标同目录（同文件系统才有原子 rename）。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_mode(path, bytes, None)
}

/// 原子写，新建文件时设定 Unix 权限（含 secret 的文件传 `Some(0o600)`）。
/// 目标已存在则保留其现有权限（尊重用户手动 chmod）。
pub fn write_atomic_mode(path: &Path, bytes: &[u8], new_file_mode: Option<u32>) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Internal(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent).map_err(io_ctx(format!(
        "failed to create directory {}",
        parent.display()
    )))?;

    let file_name = file_name_of(path)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}.donn-tmp-"))
        .tempfile_in(parent)
        .map_err(io_ctx(format!(
            "failed to create temp file in {}",
            parent.display()
        )))?;
    tmp.write_all(bytes).map_err(io_ctx(format!(
        "failed to write temp file {}",
        tmp.path().display()
    )))?;
    tmp.as_file().sync_all().map_err(io_ctx(format!(
        "failed to fsync temp file {}",
        tmp.path().display()
    )))?;

    // 保留原文件权限（用户 chmod 过的不退回默认 umask）；新建 secret 文件收紧到指定模式。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match fs::metadata(path) {
            Ok(meta) => Some(meta.permissions().mode()),
            Err(_) => new_file_mode,
        };
        // 失败即报错：secret 文件权限收不紧（只读 fs、FAT32 等）宁可整个写入失败
        if let Some(mode) = mode {
            fs::set_permissions(tmp.path(), fs::Permissions::from_mode(mode)).map_err(io_ctx(
                format!("failed to set permissions on {}", path.display()),
            ))?;
        }
    }
    // Windows 无 POSIX mode：new_file_mode 被忽略（NTFS 是 ACL 语义，mode 位不可靠），
    // 秘密文件的保护依赖 ~/.donn 自身的目录权限。
    #[cfg(not(unix))]
    let _ = new_file_mode;

    // persist = 原子替换目标（Windows 上也是）；失败（含中途 Drop）时 tempfile 自动清理
    tmp.persist(path).map_err(|e| Error::Io {
        context: format!("failed to move temp file into place at {}", path.display()),
        source: e.error,
    })?;
    // 目录项 fsync：rename 只进了目录缓存，断电窗口内元数据可能丢。部分文件系统
    // 不支持目录 sync——尽力而为，不因此否定已成功的写入。
    #[cfg(unix)]
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// 读「以它为底再合并」的现有文件。不存在 = `None`；文件在但读不了（权限等）必须报错——
/// 当成不存在会整盘重写，用户的键和注释就丢了。
pub fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_ctx(format!("failed to read {}", path.display()))(e)),
    }
}

/// 把渲染结果里 donn 拥有的顶层键并进现有 TOML 文本：以现有文件为底，只替换/删除
/// `owned` 里的键，其余键、注释与顺序原样保留（对 TOML 的「保留未知字段」）。
pub fn merge_toml_owned(
    path: &Path,
    existing: &str,
    rendered: &str,
    owned: &[&str],
) -> Result<String> {
    let invalid = |e: toml_edit::TomlError| Error::InvalidToml {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    let mut doc: toml_edit::DocumentMut = existing.parse().map_err(invalid)?;
    let new: toml_edit::DocumentMut = rendered.parse().map_err(invalid)?;
    for key in owned {
        let Some(item) = new.get(key) else {
            doc.remove(key);
            continue;
        };
        // 替换值但留住用户写在这个键/表头上方的注释
        let key_decor = doc.key(key).map(|k| k.leaf_decor().clone());
        let table_decor = doc
            .get(key)
            .and_then(toml_edit::Item::as_table)
            .map(|t| t.decor().clone());
        doc.insert(key, item.clone());
        if let Some(decor) = key_decor
            && let Some(mut k) = doc.key_mut(key)
        {
            *k.leaf_decor_mut() = decor;
        }
        if let Some(decor) = table_decor
            && let Some(t) = doc.get_mut(key).and_then(toml_edit::Item::as_table_mut)
        {
            *t.decor_mut() = decor;
        }
    }
    Ok(doc.to_string())
}

fn file_name_of(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::Internal(format!("path has no valid file name: {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_creates_parents_and_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a/b/settings.json");
        write_atomic(&path, b"{}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
    }

    #[test]
    fn write_replaces_existing_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        // 无残留 tmp 文件
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("donn-tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_existing_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        write_atomic(&path, b"v1").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"v2").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "rewrite must not reset user-tightened permissions"
        );
    }

    #[test]
    fn windows_style_paths_in_names() {
        // 表驱动：文件名提取对各种路径成立
        let dir = TempDir::new().unwrap();
        for name in [".claude.json", "settings.json", "profile.toml"] {
            let path = dir.path().join(name);
            write_atomic(&path, b"x").unwrap();
            assert!(path.exists(), "{name}");
        }
    }

    #[test]
    fn merge_keeps_unknown_keys_and_comments_and_replaces_owned() {
        let existing =
            "# my note\nname = \"old\"\nfuture = 1\n\n[extra]\nx = 1\n\n[owned_table]\na = 1\n";
        let rendered = "name = \"new\"\n\n[owned_table]\nb = 2\n";
        let merged = merge_toml_owned(
            Path::new("t.toml"),
            existing,
            rendered,
            &["name", "owned_table", "gone"],
        )
        .unwrap();
        assert!(merged.contains("# my note"), "{merged}");
        assert!(merged.contains("name = \"new\""), "{merged}");
        assert!(merged.contains("future = 1"), "{merged}");
        assert!(merged.contains("[extra]"), "{merged}");
        assert!(
            merged.contains("b = 2") && !merged.contains("a = 1"),
            "{merged}"
        );
        assert!(merge_toml_owned(Path::new("t.toml"), "not [valid", rendered, &[]).is_err());
    }

    #[test]
    fn write_lock_releases_on_drop_and_can_be_reacquired() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".write.lock");
        let lock = lock_writes(&path).unwrap();
        drop(lock);
        let _again = lock_writes(&path).unwrap();
    }
}
