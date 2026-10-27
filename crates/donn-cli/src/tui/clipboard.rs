//! 跨平台文本剪贴板，在 TUI session 期间保持存活。

#[derive(Default)]
pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn set_text(&mut self, text: String) -> Result<(), String> {
        let clipboard = match &mut self.inner {
            Some(clipboard) => clipboard,
            None => self.inner.insert(
                arboard::Clipboard::new()
                    .map_err(|error| format!("failed to open the system clipboard: {error}"))?,
            ),
        };
        clipboard
            .set_text(text)
            .map_err(|error| format!("failed to copy text: {error}"))
    }
}
