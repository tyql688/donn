//! Cross-platform text clipboard kept alive for the TUI session.

#[derive(Default)]
pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn set_text(&mut self, text: String) -> Result<(), String> {
        if self.inner.is_none() {
            self.inner = Some(
                arboard::Clipboard::new()
                    .map_err(|error| format!("failed to open the system clipboard: {error}"))?,
            );
        }
        self.inner
            .as_mut()
            .ok_or_else(|| "failed to open the system clipboard".to_string())?
            .set_text(text)
            .map_err(|error| format!("failed to copy text: {error}"))
    }
}
