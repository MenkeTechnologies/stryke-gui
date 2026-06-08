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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn key_name_table_has_no_duplicates() {
        let mut seen = HashSet::new();
        for n in KEYBOARD_KEY_NAMES {
            assert!(seen.insert(*n), "duplicate key name in table: {n}");
        }
    }

    #[test]
    fn key_name_table_is_ascii_lowercase() {
        // parse_key lowercases its input — any uppercase entry in the
        // table is dead code (will never match) and almost certainly a typo.
        for n in KEYBOARD_KEY_NAMES {
            assert!(
                n.chars().all(|c| !c.is_ascii_uppercase()),
                "non-lowercase entry: {n}"
            );
            assert!(!n.is_empty(), "empty entry in key name table");
        }
    }

    #[test]
    fn parse_key_is_case_insensitive() {
        assert!(matches!(parse_key("ENTER").unwrap(), Key::Return));
        assert!(matches!(parse_key("Enter").unwrap(), Key::Return));
        assert!(matches!(parse_key("enter").unwrap(), Key::Return));
    }

    #[test]
    fn parse_key_meta_aliases_collapse_to_meta() {
        // pyautogui parity: cmd / command / meta / super all == Meta.
        for n in ["meta", "cmd", "command", "super"] {
            assert!(matches!(parse_key(n).unwrap(), Key::Meta), "{n}");
        }
    }

    #[test]
    fn parse_key_enter_aliases_collapse_to_return() {
        for n in ["return", "enter", "numpadenter"] {
            assert!(matches!(parse_key(n).unwrap(), Key::Return), "{n}");
        }
    }

    #[test]
    fn parse_key_single_char_falls_through_to_unicode() {
        assert!(matches!(parse_key("a").unwrap(), Key::Unicode('a')));
        assert!(matches!(parse_key("Z").unwrap(), Key::Unicode('Z')));
        assert!(matches!(parse_key("7").unwrap(), Key::Unicode('7')));
        assert!(matches!(parse_key("$").unwrap(), Key::Unicode('$')));
        // Non-ASCII single char also passes through.
        assert!(matches!(parse_key("é").unwrap(), Key::Unicode('é')));
    }

    #[test]
    fn parse_key_unknown_multi_char_errors() {
        // A 1-char unknown is Unicode; a multi-char unknown must error so
        // typos in `.stk` scripts surface instead of silently mapping to
        // garbage.
        assert!(parse_key("notakey").is_err());
        assert!(parse_key("ctrl-c").is_err());
    }

    #[test]
    fn parse_key_function_row_all_parse() {
        // F1..F20 are common across every platform enigo supports.
        for i in 1..=20 {
            let name = format!("f{i}");
            parse_key(&name).unwrap_or_else(|_| panic!("F{i} should parse"));
        }
    }

    #[test]
    fn parse_key_numpad_digits_all_parse() {
        for i in 0..=9 {
            let name = format!("num{i}");
            parse_key(&name).unwrap_or_else(|_| panic!("num{i} should parse"));
        }
    }

    #[test]
    fn parse_key_arrows_distinct() {
        assert!(matches!(parse_key("up").unwrap(), Key::UpArrow));
        assert!(matches!(parse_key("down").unwrap(), Key::DownArrow));
        assert!(matches!(parse_key("left").unwrap(), Key::LeftArrow));
        assert!(matches!(parse_key("right").unwrap(), Key::RightArrow));
    }

    #[test]
    fn parse_key_page_nav_aliases() {
        assert!(matches!(parse_key("pageup").unwrap(), Key::PageUp));
        assert!(matches!(parse_key("pgup").unwrap(), Key::PageUp));
        assert!(matches!(parse_key("pagedown").unwrap(), Key::PageDown));
        assert!(matches!(parse_key("pgdn").unwrap(), Key::PageDown));
    }

    #[test]
    fn parse_key_yen_is_unicode_passthrough() {
        assert!(matches!(parse_key("yen").unwrap(), Key::Unicode('¥')));
    }

    #[test]
    fn parse_key_empty_string_errors() {
        // 0-char input is neither a known name nor a single-char unicode
        // fallback. Must error so a `gui key press ""` typo surfaces.
        assert!(parse_key("").is_err());
    }

    #[test]
    fn parse_key_modifier_left_right_distinct() {
        // pyautogui semantics: shift / shiftleft / shiftright all reach
        // enigo's distinct variants, not collapsed to one.
        assert!(matches!(parse_key("shift").unwrap(), Key::Shift));
        assert!(matches!(parse_key("shiftleft").unwrap(), Key::LShift));
        assert!(matches!(parse_key("shiftright").unwrap(), Key::RShift));
        assert!(matches!(parse_key("ctrl").unwrap(), Key::Control));
        assert!(matches!(parse_key("ctrlleft").unwrap(), Key::LControl));
        assert!(matches!(parse_key("ctrlright").unwrap(), Key::RControl));
    }

    #[test]
    fn parse_key_alt_option_aliases_collapse() {
        // pyautogui parity: alt == option on every platform.
        assert!(matches!(parse_key("alt").unwrap(), Key::Alt));
        assert!(matches!(parse_key("option").unwrap(), Key::Alt));
    }

    #[test]
    fn parse_key_escape_and_delete_aliases() {
        assert!(matches!(parse_key("escape").unwrap(), Key::Escape));
        assert!(matches!(parse_key("esc").unwrap(), Key::Escape));
        assert!(matches!(parse_key("delete").unwrap(), Key::Delete));
        assert!(matches!(parse_key("del").unwrap(), Key::Delete));
    }

    #[test]
    fn parse_key_numpad_arith_keys() {
        assert!(matches!(parse_key("add").unwrap(), Key::Add));
        assert!(matches!(parse_key("subtract").unwrap(), Key::Subtract));
        assert!(matches!(parse_key("multiply").unwrap(), Key::Multiply));
        assert!(matches!(parse_key("divide").unwrap(), Key::Divide));
        assert!(matches!(parse_key("decimal").unwrap(), Key::Decimal));
    }

    #[test]
    fn parse_key_media_keys() {
        assert!(matches!(parse_key("volumeup").unwrap(), Key::VolumeUp));
        assert!(matches!(parse_key("volumedown").unwrap(), Key::VolumeDown));
        assert!(matches!(parse_key("volumemute").unwrap(), Key::VolumeMute));
        // playpause and mediaplay both collapse to MediaPlayPause.
        assert!(matches!(
            parse_key("playpause").unwrap(),
            Key::MediaPlayPause
        ));
        assert!(matches!(
            parse_key("mediaplay").unwrap(),
            Key::MediaPlayPause
        ));
        assert!(matches!(
            parse_key("nexttrack").unwrap(),
            Key::MediaNextTrack
        ));
        assert!(matches!(
            parse_key("prevtrack").unwrap(),
            Key::MediaPrevTrack
        ));
    }

    /// Every name guaranteed to parse on every supported platform via
    /// `parse_key_common`. New additions to that arm should grow this list
    /// too — the test is the contract.
    const CROSS_PLATFORM_NAMES: &[&str] = &[
        // whitespace + edit
        "tab",
        "enter",
        "return",
        "space",
        "backspace",
        "delete",
        "del",
        "escape",
        "esc",
        "numpadenter",
        // modifiers
        "shift",
        "shiftleft",
        "shiftright",
        "ctrl",
        "control",
        "ctrlleft",
        "ctrlright",
        "alt",
        "option",
        "meta",
        "cmd",
        "command",
        "super",
        // arrows + nav
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
        // locks + system
        "capslock",
        "help",
        // function keys F1..F20
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
        // numpad
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
        // media
        "volumeup",
        "volumedown",
        "volumemute",
        "playpause",
        "mediaplay",
        "nexttrack",
        "prevtrack",
        // unicode passthrough
        "yen",
    ];

    #[test]
    fn parse_key_cross_platform_names_all_parse() {
        for n in CROSS_PLATFORM_NAMES {
            parse_key(n).unwrap_or_else(|e| panic!("cross-platform name {n:?} failed: {e}"));
        }
    }

    #[test]
    fn cross_platform_names_are_subset_of_public_table() {
        // KEYBOARD_KEY_NAMES is what `gui key keys` prints; the
        // cross-platform set used in the test above must be a subset of it
        // so we never test names that aren't documented.
        let table: HashSet<&str> = KEYBOARD_KEY_NAMES.iter().copied().collect();
        for n in CROSS_PLATFORM_NAMES {
            assert!(table.contains(*n), "{n} missing from KEYBOARD_KEY_NAMES");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_key_macos_specific_names_parse() {
        // Names enigo only exposes on macOS. If any of these stop parsing,
        // the platform arm regressed.
        for n in [
            "fn",
            "function",
            "rcommand",
            "rcmd",
            "eject",
            "power",
            "brightnessup",
            "brightnessdown",
            "launchpad",
            "missioncontrol",
        ] {
            parse_key(n).unwrap_or_else(|e| panic!("macOS name {n:?} failed: {e}"));
        }
    }

    #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
    #[test]
    fn parse_key_non_macos_specific_names_parse() {
        // Names enigo gates behind cfg(any(windows, all(unix, not(macos)))).
        for n in [
            "f21",
            "f22",
            "f23",
            "f24",
            "printscreen",
            "prntscrn",
            "prtsc",
            "prtscr",
            "insert",
            "numlock",
            "pause",
            "stop",
        ] {
            parse_key(n).unwrap_or_else(|e| panic!("non-macOS name {n:?} failed: {e}"));
        }
    }

    #[test]
    fn hotkey_empty_names_errors_before_gui_init() {
        // Empty check runs before make_enigo(), so this is safe on a
        // headless CI runner with no display.
        let err = hotkey(&[], 0.0).unwrap_err();
        assert!(err.to_string().contains("at least one key"));
    }

    #[test]
    fn hotkey_unknown_key_errors_before_gui_init() {
        // parse_key fails inside the `.collect()` before make_enigo runs.
        let err = hotkey(&["bogus".to_string()], 0.0).unwrap_err();
        assert!(err.to_string().contains("unrecognized key name"));
    }

    #[test]
    fn press_unknown_key_errors_before_gui_init() {
        let err = press("notakey", 1, 0.0).unwrap_err();
        assert!(err.to_string().contains("unrecognized key name"));
    }

    #[test]
    fn down_unknown_key_errors_before_gui_init() {
        assert!(down("notakey").is_err());
    }

    #[test]
    fn up_unknown_key_errors_before_gui_init() {
        assert!(up("notakey").is_err());
    }
}
