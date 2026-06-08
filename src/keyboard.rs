//! Keyboard synthesis — key-name parsing + press/type/hotkey ops.
//!
//! The key-name table covers PyAutoGUI's `KEYBOARD_KEYS` list plus the
//! platform-specific extras enigo 0.6 exposes (left/right modifier
//! variants, browser keys, media keys, language keys, num-pad keys). All
//! lookups are ASCII-lowercase; single-char names fall through to
//! `Key::Unicode(c)` so callers can do `key press a` without enumerating
//! every letter / digit / punctuation.

use std::thread::sleep as thread_sleep;
use std::time::Duration;

use anyhow::{anyhow, Result};
use enigo::{Direction, Key, Keyboard};

use crate::common::make_enigo;

pub const KEYBOARD_KEY_NAMES: &[&str] = &[
    // Whitespace / edit
    "tab",
    "enter",
    "return",
    "space",
    "backspace",
    "delete",
    "del",
    "escape",
    "esc",
    "linefeed",
    // Modifiers — generic + left/right variants
    "shift",
    "shiftleft",
    "shiftright",
    "ctrl",
    "control",
    "ctrlleft",
    "ctrlright",
    "alt",
    "altleft",
    "altright",
    "option",
    "optionleft",
    "optionright",
    "meta",
    "cmd",
    "command",
    "super",
    "win",
    "winleft",
    "winright",
    "rcommand",
    // Arrows + navigation
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "pgup",
    "pgdn",
    "insert",
    // Locks
    "capslock",
    "numlock",
    "scrolllock",
    "shiftlock",
    // System
    "pause",
    "printscreen",
    "prntscrn",
    "prtsc",
    "prtscr",
    "print",
    "snapshot",
    "sleep",
    "power",
    "eject",
    "help",
    "apps",
    "clear",
    "select",
    "execute",
    "cancel",
    "fn",
    "function",
    "accept",
    "convert",
    "nonconvert",
    "modechange",
    "final",
    "find",
    "redo",
    "undo",
    // Function keys F1..F24
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "f13",
    "f14",
    "f15",
    "f16",
    "f17",
    "f18",
    "f19",
    "f20",
    "f21",
    "f22",
    "f23",
    "f24",
    // Numpad
    "num0",
    "num1",
    "num2",
    "num3",
    "num4",
    "num5",
    "num6",
    "num7",
    "num8",
    "num9",
    "add",
    "subtract",
    "multiply",
    "divide",
    "decimal",
    "separator",
    "numpadenter",
    // Media
    "volumeup",
    "volumedown",
    "volumemute",
    "playpause",
    "nexttrack",
    "prevtrack",
    "stop",
    "mediarewind",
    "mediafast",
    "mediaplay",
    "micmute",
    // Launch
    "launchmail",
    "launchmediaselect",
    "launchapp1",
    "launchapp2",
    // Browser
    "browserback",
    "browserforward",
    "browserrefresh",
    "browserstop",
    "browsersearch",
    "browserfavorites",
    "browserhome",
    // CJK input method
    "hangul",
    "hanguel",
    "hanja",
    "junja",
    "kana",
    "kanji",
    // Yen — Unicode passthrough
    "yen",
];

pub fn parse_key(name: &str) -> Result<Key> {
    let lc = name.to_ascii_lowercase();
    if let Some(k) = parse_key_common(&lc) {
        return Ok(k);
    }
    if let Some(k) = parse_key_platform(&lc) {
        return Ok(k);
    }
    if name.chars().count() == 1 {
        return Ok(Key::Unicode(name.chars().next().unwrap()));
    }
    Err(anyhow!(
        "key press/down/up: unrecognized key name '{name}' on this platform"
    ))
}

/// Variants that enigo exposes on every supported OS (macOS, Linux,
/// Windows). Anything platform-conditional lives in `parse_key_platform`.
fn parse_key_common(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // ── modifiers ──
        "shift" => Shift,
        "shiftleft" => LShift,
        "shiftright" => RShift,
        "ctrl" | "control" => Control,
        "ctrlleft" => LControl,
        "ctrlright" => RControl,
        "alt" | "option" => Alt,
        "meta" | "cmd" | "command" | "super" => Meta,
        // ── whitespace + edit ──
        "return" | "enter" | "numpadenter" => Return,
        "tab" => Tab,
        "space" => Space,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "escape" | "esc" => Escape,
        // ── arrows + nav ──
        "up" => UpArrow,
        "down" => DownArrow,
        "left" => LeftArrow,
        "right" => RightArrow,
        "home" => Home,
        "end" => End,
        "pageup" | "pgup" => PageUp,
        "pagedown" | "pgdn" => PageDown,
        // ── locks ──
        "capslock" => CapsLock,
        // ── system ──
        "help" => Help,
        // ── function keys F1..F24 ──
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        "f13" => F13,
        "f14" => F14,
        "f15" => F15,
        "f16" => F16,
        "f17" => F17,
        "f18" => F18,
        "f19" => F19,
        "f20" => F20,
        // ── numpad ──
        "num0" => Numpad0,
        "num1" => Numpad1,
        "num2" => Numpad2,
        "num3" => Numpad3,
        "num4" => Numpad4,
        "num5" => Numpad5,
        "num6" => Numpad6,
        "num7" => Numpad7,
        "num8" => Numpad8,
        "num9" => Numpad9,
        "add" => Add,
        "subtract" => Subtract,
        "multiply" => Multiply,
        "divide" => Divide,
        "decimal" => Decimal,
        // ── media ──
        "volumeup" => VolumeUp,
        "volumedown" => VolumeDown,
        "volumemute" => VolumeMute,
        "playpause" | "mediaplay" => MediaPlayPause,
        "nexttrack" => MediaNextTrack,
        "prevtrack" => MediaPrevTrack,
        // ── yen as Unicode passthrough ──
        "yen" => Unicode('¥'),
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn parse_key_platform(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // macOS-specific modifiers and synonyms
        "fn" | "function" => Function,
        "rcommand" | "rcmd" => RCommand,
        "roption" | "optionright" | "altright" => ROption,
        // Mac power / hardware
        "eject" => Eject,
        "power" => Power,
        "brightnessup" => BrightnessUp,
        "brightnessdown" => BrightnessDown,
        "contrastup" => ContrastUp,
        "contrastdown" => ContrastDown,
        "illuminationup" => IlluminationUp,
        "illuminationdown" => IlluminationDown,
        "illuminationtoggle" => IlluminationToggle,
        "launchpanel" => LaunchPanel,
        "launchpad" => Launchpad,
        "missioncontrol" => MissionControl,
        "mediarewind" => MediaRewind,
        "mediafast" => MediaFast,
        "vidmirror" => VidMirror,
        // On macOS the left/right Alt and Win variants don't exist (the
        // platform uses Option / RCommand instead). Map the PyAutoGUI
        // spellings to the closest macOS equivalent so cross-platform
        // scripts keep working.
        "altleft" | "optionleft" => Alt,
        "winleft" | "win" => Meta,
        "winright" => RCommand,
        _ => return None,
    })
}

#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
fn parse_key_platform(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        // ── Available on BOTH Windows + Linux (enigo gates them under
        // cfg(any(windows, all(unix, not(macos))))) ──
        "f21" => F21,
        "f22" => F22,
        "f23" => F23,
        "f24" => F24,
        "printscreen" | "prntscrn" | "prtsc" | "prtscr" => PrintScr,
        "altleft" | "optionleft" => LMenu,
        "insert" => Insert,
        "numlock" => Numlock,
        "pause" => Pause,
        "modechange" => ModeChange,
        "select" => Select,
        "execute" => Execute,
        "cancel" => Cancel,
        "clear" => Clear,
        "stop" => MediaStop,
        "hangul" | "hanguel" => Hangul,
        "hanja" => Hanja,
        "kanji" => Kanji,
        // ── Windows-only variants ── enigo gates these under
        // cfg(target_os = "windows"); Linux build doesn't see them.
        #[cfg(target_os = "windows")]
        "altright" | "optionright" => RMenu,
        #[cfg(target_os = "windows")]
        "win" | "winleft" => LWin,
        #[cfg(target_os = "windows")]
        "winright" => RWin,
        #[cfg(target_os = "windows")]
        "apps" => Apps,
        #[cfg(target_os = "windows")]
        "sleep" => Sleep,
        #[cfg(target_os = "windows")]
        "accept" => Accept,
        #[cfg(target_os = "windows")]
        "convert" => Convert,
        #[cfg(target_os = "windows")]
        "nonconvert" => NonConvert,
        #[cfg(target_os = "windows")]
        "junja" => Junja,
        #[cfg(target_os = "windows")]
        "kana" => Kana,
        #[cfg(target_os = "windows")]
        "separator" => Separator,
        #[cfg(target_os = "windows")]
        "launchmail" => LaunchMail,
        #[cfg(target_os = "windows")]
        "launchmediaselect" => LaunchMediaSelect,
        #[cfg(target_os = "windows")]
        "launchapp1" => LaunchApp1,
        #[cfg(target_os = "windows")]
        "launchapp2" => LaunchApp2,
        #[cfg(target_os = "windows")]
        "browserback" => BrowserBack,
        #[cfg(target_os = "windows")]
        "browserforward" => BrowserForward,
        #[cfg(target_os = "windows")]
        "browserrefresh" => BrowserRefresh,
        #[cfg(target_os = "windows")]
        "browserstop" => BrowserStop,
        #[cfg(target_os = "windows")]
        "browsersearch" => BrowserSearch,
        #[cfg(target_os = "windows")]
        "browserfavorites" => BrowserFavorites,
        #[cfg(target_os = "windows")]
        "browserhome" => BrowserHome,
        _ => parse_key_unix(lc)?,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_key_unix(lc: &str) -> Option<Key> {
    use Key::*;
    Some(match lc {
        "shiftlock" => ShiftLock,
        "scrolllock" => ScrollLock,
        "linefeed" => Linefeed,
        "micmute" => MicMute,
        "find" => Find,
        "redo" => Redo,
        "undo" => Undo,
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
fn parse_key_unix(_lc: &str) -> Option<Key> {
    None
}

// ── ops ──────────────────────────────────────────────────────────────

pub fn press(name: &str, presses: i64, interval: f64) -> Result<()> {
    let presses = presses.max(1);
    let interval = interval.max(0.0);
    let key = parse_key(name)?;
    let mut e = make_enigo()?;
    for i in 0..presses {
        e.key(key, Direction::Click)?;
        if interval > 0.0 && i + 1 < presses {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    Ok(())
}

pub fn down(name: &str) -> Result<()> {
    let key = parse_key(name)?;
    let mut e = make_enigo()?;
    e.key(key, Direction::Press)?;
    Ok(())
}

pub fn up(name: &str) -> Result<()> {
    let key = parse_key(name)?;
    let mut e = make_enigo()?;
    e.key(key, Direction::Release)?;
    Ok(())
}

pub fn type_text(text: &str, interval: f64) -> Result<()> {
    let interval = interval.max(0.0);
    let mut e = make_enigo()?;
    if interval <= 0.0 {
        e.text(text)?;
    } else {
        // Per-char dispatch so the inter-char delay applies. `text()` on a
        // 1-char string takes the OS-keyboard-layout-correct fast path.
        let mut iter = text.chars().peekable();
        while let Some(c) = iter.next() {
            e.text(&c.to_string())?;
            if iter.peek().is_some() {
                thread_sleep(Duration::from_secs_f64(interval));
            }
        }
    }
    Ok(())
}

/// Chord: press every key in order, release in reverse.
pub fn hotkey(names: &[String], interval: f64) -> Result<()> {
    if names.is_empty() {
        return Err(anyhow!("key hotkey needs at least one key"));
    }
    let interval = interval.max(0.0);
    let keys: Result<Vec<Key>> = names.iter().map(|n| parse_key(n)).collect();
    let keys = keys?;
    let mut e = make_enigo()?;
    for (i, k) in keys.iter().enumerate() {
        e.key(*k, Direction::Press)?;
        if interval > 0.0 && i + 1 < keys.len() {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    for k in keys.iter().rev() {
        e.key(*k, Direction::Release)?;
    }
    Ok(())
}
