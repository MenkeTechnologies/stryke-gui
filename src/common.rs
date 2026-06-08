//! Shared helpers for the stryke-gui-helper service modules: enigo /
//! display init, button + arg parsing, JSON output writers.

use std::io::{self, Write};

use anyhow::{anyhow, Result};
use enigo::{Button, Enigo, Settings};

/// Construct a fresh `Enigo` input handle. On macOS the first call prompts
/// for Accessibility permission for the launching terminal app.
pub fn make_enigo() -> Result<Enigo> {
    Enigo::new(&Settings::default()).map_err(|e| anyhow!("GUI init failed: {e}"))
}

/// Parse a pyautogui-style button name into an enigo `Button`. Unknown /
/// empty names fall back to the left button.
pub fn parse_button(s: &str) -> Button {
    match s.to_ascii_lowercase().as_str() {
        "right" | "r" | "secondary" => Button::Right,
        "middle" | "m" | "wheel" => Button::Middle,
        _ => Button::Left,
    }
}

/// Write a single JSON value to stdout, followed by a newline. Queries
/// (mouse pos, screen size, pixel, …) use this; the `.stk` wrappers parse
/// the line with `from_json`.
pub fn emit_json<T: serde::Serialize>(v: &T) -> Result<()> {
    let stdout = io::stdout();
    let mut w = stdout.lock();
    serde_json::to_writer(&mut w, v)?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Button {
        parse_button(s)
    }

    #[test]
    fn parse_button_left_is_default_for_unknown() {
        // Spec: unknown / empty falls back to Left so a typo in a .stk
        // script doesn't error — pyautogui parity.
        assert!(matches!(b(""), Button::Left));
        assert!(matches!(b("nope"), Button::Left));
        assert!(matches!(b("left"), Button::Left));
        assert!(matches!(b("LEFT"), Button::Left));
    }

    #[test]
    fn parse_button_right_aliases() {
        assert!(matches!(b("right"), Button::Right));
        assert!(matches!(b("R"), Button::Right));
        assert!(matches!(b("Secondary"), Button::Right));
    }

    #[test]
    fn parse_button_middle_aliases() {
        assert!(matches!(b("middle"), Button::Middle));
        assert!(matches!(b("M"), Button::Middle));
        assert!(matches!(b("Wheel"), Button::Middle));
    }
}
