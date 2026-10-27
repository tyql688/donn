//! 从 `src/preset/presets/*.toml` 生成内置 preset 清单。
//! 文件名就是 key；构建时同时核对 TOML 里的 `key` 字段，避免文件名和 key 对不上。
#![allow(clippy::disallowed_macros)] // 只向 Cargo 输出构建指令，不是运行期终端输出

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = generate() {
        eprintln!("failed to generate builtin preset list: {error}");
        std::process::exit(1);
    }
}

fn generate() -> Result<(), Box<dyn Error>> {
    let manifest = env::var("CARGO_MANIFEST_DIR")?;
    let dir = Path::new(&manifest).join("src/preset/presets");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();

    let mut generated = String::from("const BUILTIN_PRESETS: &[(&str, &str)] = &[\n");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| format!("preset file has no UTF-8 stem: {}", path.display()))?;
        let text = fs::read_to_string(&path)?;
        let expected = format!("key = \"{stem}\"");
        if !text.lines().any(|line| line.trim() == expected) {
            return Err(format!(
                "{} must contain `{expected}` so the file name and preset key agree",
                path.display()
            )
            .into());
        }
        generated.push_str(&format!(
            "    ({stem:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/preset/presets/{stem}.toml\"))),\n"
        ));
    }
    generated.push_str("];\n");

    fs::write(
        Path::new(&env::var("OUT_DIR")?).join("builtin_presets.rs"),
        generated,
    )?;
    Ok(())
}
