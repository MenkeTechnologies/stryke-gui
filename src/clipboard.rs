//! Clipboard read/write via `arboard` (macOS NSPasteboard, X11/Wayland, Win32).
//!
//! A fresh `Clipboard` handle is opened per call — the package is otherwise
//! stateless and clipboard ops are infrequent relative to input synthesis, so
//! there's no handle to cache.

use anyhow::{anyhow, Result};

/// Current clipboard text (empty string when the clipboard holds no text).
pub fn get() -> Result<String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard open: {e}"))?;
    match cb.get_text() {
        Ok(s) => Ok(s),
        // A non-text or empty clipboard is not an error for callers — return "".
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(e) => Err(anyhow!("clipboard get: {e}")),
    }
}

/// Replace the clipboard text with `text`.
pub fn set(text: &str) -> Result<()> {
    let mut cb = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard open: {e}"))?;
    cb.set_text(text.to_string())
        .map_err(|e| anyhow!("clipboard set: {e}"))
}
