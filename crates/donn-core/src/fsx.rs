//! 文件落盘纪律：temp + fsync + rename 的原子写，是写用户文件的唯一通道。
//! donn 写的内容都能从 spec 再生，不做备份。

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result, io_ctx};

/// 持有期间阻止其它 donn 进程进入写事务；关闭文件就自动解锁。
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
    // lock 竞争最多等约 5 秒，之后给调用方一个可操作的错误，
    // 不能让启动前 sync 或 TUI 写操作无限挂住。
    for _ in 0..100 {
        match file.try_lock() {
            Ok(()) => return Ok(WriteLock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(std::fs::TryLockError::Error(source)) => {
                return Err(Error::Io {
                    context: format!("failed to lock {}", path.display()),
                    source,
                });
            }
        }
    }
    Err(Error::Io {
        context: format!("failed to lock {} after 5 seconds", path.display()),
        source: std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "another donn process is still writing",
        ),
    })
}

/// 原子写。tmp 文件和目标在同一目录（同一文件系统才有原子 rename）。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_mode(path, bytes, None)
}

/// 原子写；新建文件时设置 Unix 权限（含 secret 的文件传 `Some(0o600)`）。
/// 目标已存在时保留现有权限，尊重用户手动 chmod。
pub fn write_atomic_mode(path: &Path, bytes: &[u8], new_file_mode: Option<u32>) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Internal(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent).map_err(io_ctx(format!(
        "failed to create directory {}",
        parent.display()
    )))?;

    let file_name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        Error::Internal(format!("path has no valid file name: {}", path.display()))
    })?;
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

    // 保留原文件权限（用户 chmod 过的不退回默认 umask）；新建的 secret 文件收紧到指定模式。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match fs::metadata(path) {
            Ok(meta) => Some(meta.permissions().mode()),
            Err(_) => new_file_mode,
        };
        // 失败就报错：secret 文件权限收不紧（只读 fs、FAT32 等）时，宁可整个写入失败
        if let Some(mode) = mode {
            fs::set_permissions(tmp.path(), fs::Permissions::from_mode(mode)).map_err(io_ctx(
                format!("failed to set permissions on {}", path.display()),
            ))?;
        }
    }
    // Windows 没有 POSIX mode，new_file_mode 会被忽略（NTFS 是 ACL 语义，mode 位不可靠），
    // secret 文件的保护依赖 ~/.donn 目录本身的权限。
    #[cfg(not(unix))]
    let _ = new_file_mode;

    // persist 是原子替换目标（Windows 上也是）；失败（包括中途 Drop）时 tempfile 会自动清理
    tmp.persist(path).map_err(|e| Error::Io {
        context: format!("failed to move temp file into place at {}", path.display()),
        source: e.error,
    })?;
    // 目录项 fsync：rename 只进了目录缓存，断电窗口内元数据可能丢。部分文件系统
    // 不支持目录 sync，尽力而为，不因此否定已经成功的写入。
    #[cfg(unix)]
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// 读取「以它为底再合并」的现有文件。不存在返回 `None`；文件存在但读不了（权限等）必须报错，
/// 当成不存在会整盘重写，用户的 key 和注释就丢了。
pub fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_ctx(format!("failed to read {}", path.display()))(e)),
    }
}

/// 把渲染结果里 donn 拥有的顶层 key 并进现有 TOML 文本：以现有文件为底，只替换或删除
/// `owned` 里的 key，其余 key、注释和顺序原样保留（TOML 版的「保留未知字段」）。
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
        // 替换值，但留住用户写在这个 key 或表头上方的注释
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
