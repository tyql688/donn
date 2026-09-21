//! `donn list`：快查表（comfy-table 排版）。无 profile 时输出引导文案。

use comfy_table::{Table, presets};
use donn_core::ModelSlot;

use super::open_donn;

pub fn execute() -> i32 {
    let donn = match open_donn() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let cards = match donn.cards() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if cards.is_empty() {
        println!("no profiles yet — run `donn` to create one interactively");
        return 0;
    }

    let mut table = Table::new();
    table
        .load_preset(presets::NOTHING)
        .set_header(["NAME", "PRESET", "BASE_URL", "SONNET", "ALIASES"]);
    let mut broken = Vec::new();
    for card in &cards {
        if let Some(err) = &card.broken {
            broken.push((card.name.as_str(), err.as_str()));
        }
        table.add_row([
            card.name.as_str(),
            if card.broken.is_some() {
                "(broken)"
            } else {
                card.preset.as_str()
            },
            card.base_url.as_deref().unwrap_or("-"),
            card.models.get(ModelSlot::Sonnet).unwrap_or("-"),
            &card.aliases.join(", "),
        ]);
    }
    println!("{table}");
    // 坏 profile 的原因输出到 stderr 并以非零退出：脚本场景不静默
    for (name, err) in &broken {
        eprintln!("error: profile '{name}' failed to load: {err}");
    }
    if broken.is_empty() { 0 } else { 1 }
}
