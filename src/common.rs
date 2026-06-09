//! Shared helpers for the stryke-gui cdylib service modules.
//!
//! The persistent `Enigo` handle is the cdylib's main perf win over the
//! retired helper-binary model: in the fork-per-call shape, each invocation
//! constructed a new `Enigo` (and paid its handshake with the platform input
//! stack); the cdylib lives across the whole stryke process and reuses one
//! instance via [`enigo_lock`].

use std::sync::Mutex;

use anyhow::{anyhow, Result};
use enigo::{Button, Enigo, Settings};
use once_cell::sync::OnceCell;

/// Process-wide `Enigo` initialized on first `enigo_lock()` call. macOS only
/// prompts for Accessibility on the *first input event* (not on `Enigo::new`),
/// so the UX is identical to the old per-call construction — the user grants
/// the permission once and every subsequent GUI:: call reuses the handle.
fn enigo_global() -> Result<&'static Mutex<Enigo>> {
    static G: OnceCell<Mutex<Enigo>> = OnceCell::new();
    G.get_or_try_init(|| {
        Enigo::new(&Settings::default())
            .map(Mutex::new)
            .map_err(|e| anyhow!("GUI init failed: {e}"))
    })
}

/// Acquire a lock on the shared `Enigo`. All mouse/keyboard ops route through
/// this; the cdylib model lets stryke scripts hold the handle across calls
/// instead of paying the per-call init cost the old subprocess model had.
pub fn enigo_lock() -> Result<std::sync::MutexGuard<'static, Enigo>> {
    enigo_global()?
        .lock()
        .map_err(|e| anyhow!("Enigo mutex poisoned: {e}"))
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
