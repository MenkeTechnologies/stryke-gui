//! stryke-gui — GUI automation cdylib loaded in-process by stryke via dlopen.
//!
//! Each `#[no_mangle] extern "C" fn gui__*` is a JSON-string-in / JSON-string-out
//! wrapper around the `mouse` / `keyboard` / `capture` modules. stryke's FFI
//! bridge (`rust_ffi.rs::load_cdylib`) resolves these symbols at first
//! `use GUI`, registers each one as a stryke-callable function, and on each
//! call passes a JSON-encoded args dict and copies the returned JSON into a
//! stryke string. The cdylib's `stryke_free_cstring` export plugs the
//! returned-allocation leak the inline-FFI v1 had.
//!
//! Why this exists: the predecessor `stryke-gui-helper` binary required one
//! `fork(2) + exec(2) + Enigo::new()` per `GUI::*` call. The cdylib model
//! drops that to a single dlopen at first `use GUI` plus a function call
//! per op (the `Enigo` instance persists in a process-wide `OnceCell`,
//! see `common.rs::enigo_lock`).

mod capture;
mod clipboard;
mod common;
mod keyboard;
mod mouse;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;

use anyhow::Result;
use serde_json::{json, Value};

use crate::common::parse_button;

/// Run a handler that takes a parsed JSON `Value` and returns a JSON `Value`,
/// converting any error or panic into a `{"error": "<msg>"}` JSON object so
/// the stryke side can `die` on it. Always returns a freshly allocated
/// `CString` — the caller (stryke's FFI bridge) must free it via
/// [`stryke_free_cstring`].
fn ffi_call<F>(args: *const c_char, handler: F) -> *const c_char
where
    F: FnOnce(Value) -> Result<Value>,
{
    let input = if args.is_null() {
        Value::Null
    } else {
        // SAFETY: args is a `*const c_char` from stryke's FFI bridge; the
        // bridge only passes pointers into NUL-terminated `CString`s it
        // allocated for this call (see `rust_ffi.rs::invoke` StrToStr arm).
        let cs = unsafe { CStr::from_ptr(args) };
        serde_json::from_slice::<Value>(cs.to_bytes()).unwrap_or(Value::Null)
    };
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| handler(input)));
    let out = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({ "error": e.to_string() }),
        Err(_) => json!({ "error": "stryke-gui handler panicked" }),
    };
    let s =
        serde_json::to_string(&out).unwrap_or_else(|_| String::from(r#"{"error":"serialize"}"#));
    match CString::new(s) {
        Ok(c) => c.into_raw() as *const c_char,
        Err(_) => std::ptr::null(),
    }
}

/// Free a C string allocated by any of this cdylib's exports. stryke's FFI
/// bridge calls this immediately after copying the returned bytes into a
/// stryke string. Without this hook, every call would leak its return
/// allocation — that's the v1 inline-FFI bug this export plugs.
///
/// # Safety
///
/// `p` must be a pointer previously returned by an export from this cdylib
/// (i.e. a `CString::into_raw` output), or a null pointer. Calling this with
/// any other pointer is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn stryke_free_cstring(p: *mut c_char) {
    if p.is_null() {
        return;
    }
    drop(CString::from_raw(p));
}

// ── mouse: position / size ───────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__mouse_pos(args: *const c_char) -> *const c_char {
    ffi_call(args, |_| Ok(serde_json::to_value(mouse::pos()?)?))
}

#[no_mangle]
pub extern "C" fn gui__screen_size(args: *const c_char) -> *const c_char {
    ffi_call(args, |_| Ok(serde_json::to_value(mouse::screen_size()?)?))
}

#[no_mangle]
pub extern "C" fn gui__displays(args: *const c_char) -> *const c_char {
    ffi_call(args, |_| {
        Ok(json!({ "displays": serde_json::to_value(capture::displays()?)? }))
    })
}

#[no_mangle]
pub extern "C" fn gui__display_screenshot(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let id = v["display"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("missing display (a monitor id from displays())"))?
            as u32;
        if let Some(path) = v["output"].as_str() {
            Ok(json!({ "path": capture::display_screenshot_to_file(id, path)? }))
        } else {
            Ok(serde_json::to_value(capture::display_screenshot_raw(id)?)?)
        }
    })
}

#[no_mangle]
pub extern "C" fn gui__screen_on_screen(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_i64().unwrap_or(0) as i32;
        let y = v["y"].as_i64().unwrap_or(0) as i32;
        Ok(json!({ "on_screen": mouse::on_screen(x, y)? }))
    })
}

// ── mouse: motion ────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__mouse_move(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_i64().unwrap_or(0) as i32;
        let y = v["y"].as_i64().unwrap_or(0) as i32;
        let dur = v["duration"].as_f64().unwrap_or(0.0);
        mouse::mv(x, y, dur)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__mouse_move_rel(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let dx = v["dx"].as_i64().unwrap_or(0) as i32;
        let dy = v["dy"].as_i64().unwrap_or(0) as i32;
        let dur = v["duration"].as_f64().unwrap_or(0.0);
        mouse::mv_rel(dx, dy, dur)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__mouse_drag(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_i64().unwrap_or(0) as i32;
        let y = v["y"].as_i64().unwrap_or(0) as i32;
        let dur = v["duration"].as_f64().unwrap_or(0.0);
        let btn = parse_button(v["button"].as_str().unwrap_or("left"));
        let rel = v["relative"].as_bool().unwrap_or(false);
        mouse::drag(x, y, dur, btn, rel)?;
        Ok(json!({}))
    })
}

// ── mouse: buttons ───────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__mouse_click(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_i64().map(|n| n as i32);
        let y = v["y"].as_i64().map(|n| n as i32);
        let clicks = v["clicks"].as_i64().unwrap_or(1);
        let interval = v["interval"].as_f64().unwrap_or(0.0);
        let btn = parse_button(v["button"].as_str().unwrap_or("left"));
        mouse::click(x, y, clicks, interval, btn)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__mouse_button_hold(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let btn = parse_button(v["button"].as_str().unwrap_or("left"));
        let press = v["press"].as_bool().unwrap_or(true);
        mouse::button_hold(btn, press)?;
        Ok(json!({}))
    })
}

// ── mouse: wheel ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__mouse_scroll(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let clicks = v["clicks"].as_i64().unwrap_or(0) as i32;
        let x = v["x"].as_i64().map(|n| n as i32);
        let y = v["y"].as_i64().map(|n| n as i32);
        let horizontal = v["horizontal"].as_bool().unwrap_or(false);
        mouse::scroll(clicks, x, y, horizontal)?;
        Ok(json!({}))
    })
}

// ── keyboard ─────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__key_press(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let name = v["name"].as_str().unwrap_or("").to_string();
        let presses = v["presses"].as_i64().unwrap_or(1);
        let interval = v["interval"].as_f64().unwrap_or(0.0);
        keyboard::press(&name, presses, interval)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__key_down(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let name = v["name"].as_str().unwrap_or("").to_string();
        keyboard::down(&name)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__key_up(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let name = v["name"].as_str().unwrap_or("").to_string();
        keyboard::up(&name)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__key_type(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let text = v["text"].as_str().unwrap_or("").to_string();
        let interval = v["interval"].as_f64().unwrap_or(0.0);
        keyboard::type_text(&text, interval)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__clipboard_get(args: *const c_char) -> *const c_char {
    ffi_call(args, |_| Ok(json!({ "text": clipboard::get()? })))
}

#[no_mangle]
pub extern "C" fn gui__clipboard_set(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let text = v["text"].as_str().unwrap_or("");
        clipboard::set(text)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__key_hotkey(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let names: Vec<String> = v["names"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let interval = v["interval"].as_f64().unwrap_or(0.0);
        keyboard::hotkey(&names, interval)?;
        Ok(json!({}))
    })
}

#[no_mangle]
pub extern "C" fn gui__key_keys(args: *const c_char) -> *const c_char {
    ffi_call(args, |_| {
        Ok(serde_json::to_value(keyboard::KEYBOARD_KEY_NAMES)?)
    })
}

// ── pixel + screenshot ───────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn gui__pixel(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_u64().unwrap_or(0) as u32;
        let y = v["y"].as_u64().unwrap_or(0) as u32;
        Ok(serde_json::to_value(capture::pixel(x, y)?)?)
    })
}

#[no_mangle]
pub extern "C" fn gui__pixel_match(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let x = v["x"].as_u64().unwrap_or(0) as u32;
        let y = v["y"].as_u64().unwrap_or(0) as u32;
        let r = v["r"].as_u64().unwrap_or(0) as u8;
        let g = v["g"].as_u64().unwrap_or(0) as u8;
        let b = v["b"].as_u64().unwrap_or(0) as u8;
        let tol = v["tolerance"].as_i64().unwrap_or(0).max(0) as i32;
        let m = capture::pixel_matches(x, y, (r, g, b), tol)?;
        Ok(json!({ "match": m }))
    })
}

#[no_mangle]
pub extern "C" fn gui__screenshot(args: *const c_char) -> *const c_char {
    ffi_call(args, |v| {
        let region = region_from_value(&v);
        if let Some(path) = v["output"].as_str() {
            let p = capture::screenshot_to_file(region, path)?;
            Ok(json!({ "path": p }))
        } else {
            Ok(serde_json::to_value(capture::screenshot_raw(region)?)?)
        }
    })
}

fn region_from_value(v: &Value) -> Option<(i32, i32, u32, u32)> {
    let arr = v.get("region")?.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let l = arr[0].as_i64()? as i32;
    let t = arr[1].as_i64()? as i32;
    let w = arr[2].as_i64()?.max(0) as u32;
    let h = arr[3].as_i64()?.max(0) as u32;
    Some((l, t, w, h))
}

// ── pure helpers (no display / input) ───────────────────────────────────────

/// Parse a hotkey string `ctrl+shift+a` into `{keys, modifiers, key}` — the
/// final segment is the main key, the rest are modifiers. Keys are lowercased
/// and trimmed. Pure.
fn op_parse_hotkey(v: Value) -> Result<Value> {
    let s = v
        .get("hotkey")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing hotkey"))?;
    let keys: Vec<String> = s
        .split('+')
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    if keys.is_empty() {
        return Err(anyhow::anyhow!("empty hotkey"));
    }
    let key = keys[keys.len() - 1].clone();
    let modifiers: Vec<String> = keys[..keys.len() - 1].to_vec();
    Ok(json!({"keys": keys, "modifiers": modifiers, "key": key}))
}

/// Canonicalize a hotkey from parts into a single accelerator string — the
/// normalizing inverse of `parse_hotkey`. Modifier aliases collapse to the W3C
/// UI Events names (`ctrl`/`control` → `Control`; `alt`/`opt`/`option` → `Alt`;
/// `shift` → `Shift`; `meta`/`cmd`/`command`/`super`/`win`/`windows` → `Meta`),
/// duplicates are dropped, and the modifiers are emitted in the canonical order
/// `Control, Alt, Shift, Meta` followed by the key. A single-letter key is
/// upper-cased; other key names pass through trimmed. Accepts either the
/// `parse_hotkey` shape (`keys` array, last entry is the key) or explicit
/// `modifiers` + `key`. An unrecognized modifier is an error. Returns
/// `{hotkey, modifiers, key}`. Pure.
fn op_format_hotkey(v: Value) -> Result<Value> {
    let (mods_in, key_in): (Vec<String>, String) =
        if let Some(keys) = v.get("keys").and_then(Value::as_array) {
            let ks: Vec<String> = keys
                .iter()
                .filter_map(|k| k.as_str().map(str::to_string))
                .collect();
            if ks.is_empty() {
                return Err(anyhow::anyhow!("empty keys"));
            }
            let key = ks[ks.len() - 1].clone();
            (ks[..ks.len() - 1].to_vec(), key)
        } else {
            let key = v
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing key"))?
                .to_string();
            let mods = v
                .get("modifiers")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            (mods, key)
        };
    const ORDER: [&str; 4] = ["Control", "Alt", "Shift", "Meta"];
    let canon = |m: &str| -> Option<&'static str> {
        match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => Some("Control"),
            "alt" | "opt" | "option" => Some("Alt"),
            "shift" => Some("Shift"),
            "meta" | "cmd" | "command" | "super" | "win" | "windows" => Some("Meta"),
            _ => None,
        }
    };
    let mut present = [false; 4];
    for m in &mods_in {
        match canon(m) {
            Some(c) => present[ORDER.iter().position(|x| *x == c).unwrap()] = true,
            None => return Err(anyhow::anyhow!("unknown modifier `{m}`")),
        }
    }
    let key = key_in.trim();
    if key.is_empty() {
        return Err(anyhow::anyhow!("empty key"));
    }
    let key = if key.chars().count() == 1 && key.chars().next().unwrap().is_ascii_alphabetic() {
        key.to_ascii_uppercase()
    } else {
        key.to_string()
    };
    let modifiers: Vec<String> = ORDER
        .iter()
        .enumerate()
        .filter(|(i, _)| present[*i])
        .map(|(_, c)| c.to_string())
        .collect();
    let mut parts = modifiers.clone();
    parts.push(key.clone());
    Ok(json!({"hotkey": parts.join("+"), "modifiers": modifiers, "key": key}))
}

/// Parse a color string `#rgb`, `#rrggbb`, or `rgb(r,g,b)` into
/// `{r, g, b, hex}`. Pure.
fn op_parse_color(v: Value) -> Result<Value> {
    let raw = v
        .get("color")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing color"))?;
    let s = raw.trim();
    let (r, g, b) = if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            3 => {
                let comp = |c: &str| u8::from_str_radix(&c.repeat(2), 16);
                (
                    comp(&hex[0..1]).map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
                    comp(&hex[1..2]).map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
                    comp(&hex[2..3]).map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
                )
            }
            6 => (
                u8::from_str_radix(&hex[0..2], 16)
                    .map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
                u8::from_str_radix(&hex[2..4], 16)
                    .map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
                u8::from_str_radix(&hex[4..6], 16)
                    .map_err(|_| anyhow::anyhow!("invalid hex color: {s}"))?,
            ),
            _ => {
                return Err(anyhow::anyhow!(
                    "invalid hex color (want #rgb or #rrggbb): {s}"
                ))
            }
        }
    } else if let Some(inner) = s.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("rgb() needs 3 components: {s}"));
        }
        let p = |i: usize| {
            parts[i]
                .parse::<u8>()
                .map_err(|_| anyhow::anyhow!("rgb component out of range (0-255): {s}"))
        };
        (p(0)?, p(1)?, p(2)?)
    } else {
        return Err(anyhow::anyhow!(
            "unrecognized color `{s}` (want #rrggbb, #rgb, or rgb(r,g,b))"
        ));
    };
    Ok(json!({"r": r, "g": g, "b": b, "hex": format!("#{r:02x}{g:02x}{b:02x}")}))
}

/// Format an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) as a string in a
/// chosen notation — the string-emitting inverse of `parse_color`. opts: `color`
/// (or `a`), `format` (`"hex"` → `#rrggbb`, the default; or `"rgb"` → `rgb(r, g,
/// b)`). Components are clamped to 0-255; both notations round-trip through
/// `parse_color`. Returns `{r, g, b, format, formatted}`. Pure.
fn op_format_color(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let clamp = |x: i64| x.clamp(0, 255) as u8;
    let (r, g, b) = (clamp(r), clamp(g), clamp(b));
    let fmt = v.get("format").and_then(Value::as_str).unwrap_or("hex");
    let formatted = match fmt {
        "hex" => format!("#{r:02x}{g:02x}{b:02x}"),
        "rgb" => format!("rgb({r}, {g}, {b})"),
        other => {
            return Err(anyhow::anyhow!(
                "unknown color format `{other}` (want hex or rgb)"
            ))
        }
    };
    Ok(json!({"r": r, "g": g, "b": b, "format": fmt, "formatted": formatted}))
}

fn rgb_of(val: &Value) -> Result<(i64, i64, i64)> {
    if let Some(arr) = val.as_array() {
        if arr.len() == 3 {
            return Ok((
                arr[0].as_i64().unwrap_or(0),
                arr[1].as_i64().unwrap_or(0),
                arr[2].as_i64().unwrap_or(0),
            ));
        }
    }
    if val.is_object() {
        return Ok((
            val["r"].as_i64().unwrap_or(0),
            val["g"].as_i64().unwrap_or(0),
            val["b"].as_i64().unwrap_or(0),
        ));
    }
    Err(anyhow::anyhow!("expected [r,g,b] or {{r,g,b}}"))
}

/// Distance between two RGB colors `a` and `b` (each `[r,g,b]` or `{r,g,b}`),
/// as both `manhattan` (sum of abs diffs) and `euclidean`. For tolerance-based
/// pixel matching. Pure.
fn op_color_distance(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let manhattan = (ar - br).abs() + (ag - bg).abs() + (ab - bb).abs();
    let euclidean = (((ar - br).pow(2) + (ag - bg).pow(2) + (ab - bb).pow(2)) as f64).sqrt();
    Ok(json!({"manhattan": manhattan, "euclidean": euclidean}))
}

/// Linearize one 0-255 sRGB channel to 0-1 linear-light, per IEC 61966-2-1 (the
/// 0.04045 colorimetric breakpoint — distinct from WCAG's 0.03928 luminance one).
fn srgb_to_linear(c: i64) -> f64 {
    let c = (c as f64 / 255.0).clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert an 0-255 sRGB color to CIELAB (D65). Linear sRGB → XYZ uses the
/// standard sRGB/D65 matrix (Bruce Lindbloom); XYZ → L*a*b* normalizes by the D65
/// white (Xn 0.950489, Yn 1, Zn 1.088840) and applies the CIE ε/κ nonlinearity.
fn rgb_to_lab(r: i64, g: i64, b: i64) -> (f64, f64, f64) {
    let (rl, gl, bl) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let x = 0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl;
    let y = 0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl;
    let z = 0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl;
    let f = |t: f64| -> f64 {
        const EPS: f64 = 216.0 / 24389.0;
        const KAPPA: f64 = 24389.0 / 27.0;
        if t > EPS {
            t.cbrt()
        } else {
            (KAPPA * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.950489), f(y), f(z / 1.088840));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// CIEDE2000 (ΔE00) perceptual color difference between two CIELAB colors, with
/// the parametric factors kL=kC=kH=1. Standard Sharma et al. (2005) formulation —
/// see the unit tests for the published reference values it reproduces.
fn ciede2000(l1: f64, a1: f64, b1: f64, l2: f64, a2: f64, b2: f64) -> f64 {
    let deg = std::f64::consts::PI / 180.0;
    let pow7 = |x: f64| x.powi(7);
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let cbar = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (pow7(cbar) / (pow7(cbar) + pow7(25.0))).sqrt());
    let (a1p, a2p) = (a1 * (1.0 + g), a2 * (1.0 + g));
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();
    let cbarp = (c1p + c2p) / 2.0;
    let hue = |ap: f64, bb: f64| -> f64 {
        if ap == 0.0 && bb == 0.0 {
            return 0.0;
        }
        let mut h = bb.atan2(ap) / deg;
        if h < 0.0 {
            h += 360.0;
        }
        h
    };
    let h1p = hue(a1p, b1);
    let h2p = hue(a2p, b2);
    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p == 0.0 || c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let dcap_h = 2.0 * (c1p * c2p).sqrt() * (dhp / 2.0 * deg).sin();
    let lbarp = (l1 + l2) / 2.0;
    let hbarp = if c1p == 0.0 || c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * ((hbarp - 30.0) * deg).cos()
        + 0.24 * ((2.0 * hbarp) * deg).cos()
        + 0.32 * ((3.0 * hbarp + 6.0) * deg).cos()
        - 0.20 * ((4.0 * hbarp - 63.0) * deg).cos();
    let sl = 1.0 + (0.015 * (lbarp - 50.0).powi(2)) / (20.0 + (lbarp - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbarp;
    let sh = 1.0 + 0.015 * cbarp * t;
    let rc = (pow7(cbarp) / (pow7(cbarp) + pow7(25.0))).sqrt();
    let dtheta = 60.0 * (-(((hbarp - 275.0) / 25.0).powi(2))).exp();
    let rt = -2.0 * rc * (dtheta * deg).sin();
    ((dlp / sl).powi(2)
        + (dcp / sc).powi(2)
        + (dcap_h / sh).powi(2)
        + rt * (dcp / sc) * (dcap_h / sh))
        .sqrt()
}

/// Perceptual color difference ΔE00 (CIEDE2000) between two RGB colors — unlike
/// color_distance (plain RGB-space manhattan/euclidean), this converts both to
/// CIELAB (D65) and applies the CIEDE2000 formula, so the number tracks how
/// different the colors *look*. ΔE ≈ 1 is the just-noticeable threshold; 0 is
/// identical, ~100 spans black↔white. opts: `a`, `b` (each `[r,g,b]` or `{r,g,b}`).
/// Returns `{delta_e, lab_a:{l,a,b}, lab_b:{l,a,b}}`. Pure.
fn op_delta_e(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let (l1, a1, b1) = rgb_to_lab(ar, ag, ab);
    let (l2, a2, b2) = rgb_to_lab(br, bg, bb);
    let de = ciede2000(l1, a1, b1, l2, a2, b2);
    let r4 = |x: f64| (x * 10000.0).round() / 10000.0;
    Ok(json!({
        "delta_e": r4(de),
        "lab_a": {"l": r4(l1), "a": r4(a1), "b": r4(b1)},
        "lab_b": {"l": r4(l2), "a": r4(a2), "b": r4(b2)},
    }))
}

/// Blend two RGB colors `a` and `b` (each `[r,g,b]` or `{r,g,b}`) by `weight` —
/// the fraction of `b` in the result (default 0.5, an even mix; clamped to
/// [0,1]). Components are linearly interpolated in sRGB (a simple, gamma-naive
/// blend — not perceptual) and rounded; `weight` 0 returns `a`, 1 returns `b`.
/// For tints, shades, and gradient stops. Returns `{r, g, b, hex}` with 0-255
/// components. Pure.
fn op_mix(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let w = v
        .get("weight")
        .and_then(Value::as_f64)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let lerp = |x: i64, y: i64| -> u8 {
        (((x as f64) * (1.0 - w) + (y as f64) * w).round() as i64).clamp(0, 255) as u8
    };
    let (r, g, b) = (lerp(ar, br), lerp(ag, bg), lerp(ab, bb));
    Ok(json!({"r": r, "g": g, "b": b, "hex": format!("#{r:02x}{g:02x}{b:02x}")}))
}

/// WCAG 2.1 relative luminance of an sRGB color (components 0-255), in [0,1].
/// Each channel is linearized (`c/12.92` below the 0.03928 knee, else
/// `((c+0.055)/1.055)^2.4`) and weighted 0.2126/0.7152/0.0722.
fn relative_luminance(r: i64, g: i64, b: i64) -> f64 {
    let chan = |c: i64| -> f64 {
        let c = (c as f64 / 255.0).clamp(0.0, 1.0);
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * chan(r) + 0.7152 * chan(g) + 0.0722 * chan(b)
}

/// WCAG 2.1 contrast ratio between two colors `a` and `b` (each `[r,g,b]` or
/// `{r,g,b}`): `(L_light + 0.05) / (L_dark + 0.05)`, from 1 (identical) to 21
/// (black vs white). Reports the ratio (2 d.p.) plus whether it clears the WCAG
/// thresholds — AA normal ≥4.5, AA large ≥3, AAA normal ≥7, AAA large ≥4.5 — so
/// you can check a foreground/background pair for accessibility. Pure.
fn op_contrast_ratio(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let la = relative_luminance(ar, ag, ab);
    let lb = relative_luminance(br, bg, bb);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    let ratio = (hi + 0.05) / (lo + 0.05);
    let rounded = (ratio * 100.0).round() / 100.0;
    Ok(json!({
        "ratio": rounded,
        "aa_normal": rounded >= 4.5,
        "aa_large": rounded >= 3.0,
        "aaa_normal": rounded >= 7.0,
        "aaa_large": rounded >= 4.5,
    }))
}

/// WCAG 2.1 relative luminance of a single color (`color` as `[r,g,b]` or
/// `{r,g,b}`, components 0-255), in [0,1] — the building block `contrast_ratio`
/// combines. A common use is picking readable text: luminance > ~0.5 means a
/// light background (use dark text). Pure.
fn op_relative_luminance(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    Ok(json!({ "luminance": relative_luminance(r, g, b) }))
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to HSL per
/// the CSS Color spec: `h` in degrees [0,360), `s` and `l` in percent [0,100].
/// Useful for deriving hover/active shades by nudging lightness. Pure.
fn op_to_hsl(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (ri, gi, bi) = rgb_of(src)?;
    let (r, g, b) = (ri as f64 / 255.0, gi as f64 / 255.0, bi as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let (h, s) = if (max - min).abs() < f64::EPSILON {
        (0.0, 0.0) // achromatic
    } else {
        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if (max - r).abs() < f64::EPSILON {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h / 6.0, s)
    };
    Ok(json!({
        "h": h * 360.0,
        "s": s * 100.0,
        "l": l * 100.0,
    }))
}

/// Convert HSL back to RGB per the CSS Color spec — the inverse of `to_hsl`.
/// opts: `h` (degrees, wrapped into [0,360)), `s` and `l` (percent, clamped to
/// [0,100]). Returns `{r, g, b, hex}` with 0-255 components. Pure.
fn op_from_hsl(v: Value) -> Result<Value> {
    let h = v.get("h").and_then(Value::as_f64).unwrap_or(0.0);
    let s = v
        .get("s")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let l = v
        .get("l")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    // Normalize hue to [0,1).
    let h = (h.rem_euclid(360.0)) / 360.0;
    let (r, g, b) = if s == 0.0 {
        (l, l, l) // achromatic
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let hue2rgb = |p: f64, q: f64, mut t: f64| -> f64 {
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        (
            hue2rgb(p, q, h + 1.0 / 3.0),
            hue2rgb(p, q, h),
            hue2rgb(p, q, h - 1.0 / 3.0),
        )
    };
    let to_u8 = |x: f64| (x * 255.0).round() as u8;
    let (r, g, b) = (to_u8(r), to_u8(g), to_u8(b));
    Ok(json!({
        "r": r,
        "g": g,
        "b": b,
        "hex": format!("#{r:02x}{g:02x}{b:02x}"),
    }))
}

/// Rotate a color's hue by `degrees` in HSL space, keeping its saturation and
/// lightness — the basis of colour-scheme generation (complementary = 180°,
/// triadic = ±120°, analogous = ±30°). The colour is converted to HSL, the hue
/// advanced (wrapping mod 360; negative is allowed), and converted back. A grey
/// (saturation 0) is unchanged since it has no chroma to rotate. opts: `color`
/// (any form `parse_color` accepts), `degrees` (or `deg`). Returns
/// `{r, g, b, hex, h}` where `h` is the resulting hue. Pure.
fn op_rotate_hue(v: Value) -> Result<Value> {
    let degrees = v
        .get("degrees")
        .or_else(|| v.get("deg"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing degrees"))?;
    let color = v
        .get("color")
        .or_else(|| v.get("a"))
        .cloned()
        .unwrap_or(Value::Null);
    let hsl = op_to_hsl(json!({ "color": color }))?;
    let h = hsl["h"].as_f64().unwrap_or(0.0);
    let s = hsl["s"].as_f64().unwrap_or(0.0);
    let l = hsl["l"].as_f64().unwrap_or(0.0);
    let new_h = (h + degrees).rem_euclid(360.0);
    let mut out = op_from_hsl(json!({ "h": new_h, "s": s, "l": l }))?;
    out["h"] = json!(new_h);
    Ok(out)
}

/// Nudge a color's HSL lightness by `delta` percentage points, keeping its hue
/// and saturation — `lighten` for a positive delta, `darken` for a negative one
/// (the basis of hover/active/disabled shades). The color is converted to HSL,
/// `l` shifted and clamped to [0,100], and converted back. opts: `color` (`[r,g,b]`
/// or `{r,g,b}`), `delta` (or `amount`, percentage points). Returns
/// `{r, g, b, hex, l}` where `l` is the resulting lightness. Pure.
fn op_adjust_lightness(v: Value) -> Result<Value> {
    let delta = v
        .get("delta")
        .or_else(|| v.get("amount"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing delta"))?;
    let color = v
        .get("color")
        .or_else(|| v.get("a"))
        .cloned()
        .unwrap_or(Value::Null);
    let hsl = op_to_hsl(json!({ "color": color }))?;
    let h = hsl["h"].as_f64().unwrap_or(0.0);
    let s = hsl["s"].as_f64().unwrap_or(0.0);
    let l = hsl["l"].as_f64().unwrap_or(0.0);
    let new_l = (l + delta).clamp(0.0, 100.0);
    let mut out = op_from_hsl(json!({ "h": h, "s": s, "l": new_l }))?;
    out["l"] = json!(new_l);
    Ok(out)
}

/// Nudge a color's HSL saturation by `delta` percentage points, keeping its hue
/// and lightness — `saturate` for a positive delta, `desaturate` for a negative
/// one (set it to -100 for greyscale). The saturation-axis counterpart of
/// `adjust_lightness`: the color is converted to HSL, `s` shifted and clamped to
/// [0,100], and converted back. opts: `color` (`[r,g,b]` or `{r,g,b}`), `delta`
/// (or `amount`, percentage points). Returns `{r, g, b, hex, s}` where `s` is the
/// resulting saturation. Pure.
fn op_adjust_saturation(v: Value) -> Result<Value> {
    let delta = v
        .get("delta")
        .or_else(|| v.get("amount"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing delta"))?;
    let color = v
        .get("color")
        .or_else(|| v.get("a"))
        .cloned()
        .unwrap_or(Value::Null);
    let hsl = op_to_hsl(json!({ "color": color }))?;
    let h = hsl["h"].as_f64().unwrap_or(0.0);
    let s = hsl["s"].as_f64().unwrap_or(0.0);
    let l = hsl["l"].as_f64().unwrap_or(0.0);
    let new_s = (s + delta).clamp(0.0, 100.0);
    let mut out = op_from_hsl(json!({ "h": h, "s": new_s, "l": l }))?;
    out["s"] = json!(new_s);
    Ok(out)
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to HSV (a.k.a.
/// HSB) — the model most colour pickers use: `h` in degrees [0,360), `s` and `v`
/// in percent [0,100]. Distinct from HSL: `v` is the brightest channel, whereas
/// HSL's `l` is the midpoint, so the same colour reports different s/l vs s/v.
/// Pure.
fn op_to_hsv(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (ri, gi, bi) = rgb_of(src)?;
    let (r, g, b) = (ri as f64 / 255.0, gi as f64 / 255.0, bi as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max <= f64::EPSILON { 0.0 } else { d / max };
    let h = if d.abs() < f64::EPSILON {
        0.0 // achromatic
    } else {
        let hue = if (max - r).abs() < f64::EPSILON {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        hue / 6.0
    };
    Ok(json!({
        "h": h * 360.0,
        "s": s * 100.0,
        "v": max * 100.0,
    }))
}

/// Convert an HSV color (`h` degrees, `s`/`v` percent) back to RGB — the inverse
/// of `to_hsv`, mirroring `from_hsl`. `h` is normalized mod 360; `s`/`v` clamp to
/// [0,100]. Returns `{r, g, b, hex}` with components rounded to 0-255. Pure.
fn op_from_hsv(v: Value) -> Result<Value> {
    let h = v.get("h").and_then(Value::as_f64).unwrap_or(0.0);
    let s = v
        .get("s")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let val = v
        .get("v")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let h = h.rem_euclid(360.0);
    let c = val * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = val - c;
    let to_u8 = |y: f64| ((y + m) * 255.0).round() as u8;
    let (r, g, b) = (to_u8(r1), to_u8(g1), to_u8(b1));
    Ok(json!({
        "r": r,
        "g": g,
        "b": b,
        "hex": format!("#{r:02x}{g:02x}{b:02x}"),
    }))
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to HWB
/// (Hue-Whiteness-Blackness, CSS Color 4 §8) — the cylindrical space alongside
/// `to_hsl`/`to_hsv`. Hue is the HSV/HSL hue; whiteness is `min(r,g,b)` and
/// blackness is `1 - max(r,g,b)`, each as a percentage. opts: `color`/`a`. Returns
/// `{h, w, b}` (`h` degrees, `w`/`b` percent). Pure.
fn op_to_hwb(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (ri, gi, bi) = rgb_of(src)?;
    let (r, g, b) = (ri as f64 / 255.0, gi as f64 / 255.0, bi as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    // Hue is identical to to_hsv's (HSL/HSV share the hue angle).
    let h = if d.abs() < f64::EPSILON {
        0.0 // achromatic
    } else {
        let hue = if (max - r).abs() < f64::EPSILON {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        hue / 6.0
    };
    Ok(json!({
        "h": h * 360.0,
        "w": min * 100.0,
        "b": (1.0 - max) * 100.0,
    }))
}

/// Convert an HWB color (`h` degrees, `w`/`b` percent) back to RGB — the inverse
/// of `to_hwb` (CSS Color 4 §8.1). `h` is normalized mod 360; `w`/`b` clamp to
/// [0,100]. When `w + b >= 100%` the result is the gray `w / (w + b)`; otherwise
/// the pure hue (HSV with s=v=1) is scaled by `1 - w - b` and offset by `w`.
/// Returns `{r, g, b, hex}` with components rounded to 0-255. Pure.
fn op_from_hwb(v: Value) -> Result<Value> {
    let h = v.get("h").and_then(Value::as_f64).unwrap_or(0.0);
    let w = v
        .get("w")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let bl = v
        .get("b")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let to_u8 = |y: f64| (y * 255.0).round() as u8;
    let (r, g, b) = if w + bl >= 1.0 {
        let gray = w / (w + bl);
        (to_u8(gray), to_u8(gray), to_u8(gray))
    } else {
        // Pure hue: HSV with s=v=1 (c=1, m=0), same piecewise as from_hsv.
        let hp = h.rem_euclid(360.0) / 60.0;
        let x = 1.0 - ((hp % 2.0) - 1.0).abs();
        let (r1, g1, b1) = if hp < 1.0 {
            (1.0, x, 0.0)
        } else if hp < 2.0 {
            (x, 1.0, 0.0)
        } else if hp < 3.0 {
            (0.0, 1.0, x)
        } else if hp < 4.0 {
            (0.0, x, 1.0)
        } else if hp < 5.0 {
            (x, 0.0, 1.0)
        } else {
            (1.0, 0.0, x)
        };
        let span = 1.0 - w - bl;
        let mix = |c: f64| c * span + w;
        (to_u8(mix(r1)), to_u8(mix(g1)), to_u8(mix(b1)))
    };
    Ok(json!({
        "r": r,
        "g": g,
        "b": b,
        "hex": format!("#{r:02x}{g:02x}{b:02x}"),
    }))
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to CMYK using
/// the standard profile-free formula (`K = 1 - max(r,g,b)`; the rest scaled by
/// `1-K`). The print-world fourth space alongside `to_hsl`/`to_hsv`. `c,m,y,k`
/// are percentages [0,100]; pure black reports `k=100` with `c=m=y=0`. Pure.
fn op_to_cmyk(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (ri, gi, bi) = rgb_of(src)?;
    let (r, g, b) = (ri as f64 / 255.0, gi as f64 / 255.0, bi as f64 / 255.0);
    let k = 1.0 - r.max(g).max(b);
    let (c, m, y) = if (1.0 - k).abs() < f64::EPSILON {
        (0.0, 0.0, 0.0) // pure black: avoid divide-by-zero
    } else {
        let d = 1.0 - k;
        ((1.0 - r - k) / d, (1.0 - g - k) / d, (1.0 - b - k) / d)
    };
    Ok(json!({
        "c": c * 100.0,
        "m": m * 100.0,
        "y": y * 100.0,
        "k": k * 100.0,
    }))
}

/// Convert CMYK back to RGB — the inverse of `to_cmyk`. opts: `c`, `m`, `y`, `k`
/// (percent, clamped to [0,100]). Returns `{r, g, b, hex}` with 0-255
/// components. Pure.
fn op_from_cmyk(v: Value) -> Result<Value> {
    let comp = |k: &str| {
        v.get(k)
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .clamp(0.0, 100.0)
            / 100.0
    };
    let (c, m, y, k) = (comp("c"), comp("m"), comp("y"), comp("k"));
    let to_u8 = |chan: f64| (255.0 * (1.0 - chan) * (1.0 - k)).round() as u8;
    let (r, g, b) = (to_u8(c), to_u8(m), to_u8(y));
    Ok(json!({
        "r": r,
        "g": g,
        "b": b,
        "hex": format!("#{r:02x}{g:02x}{b:02x}"),
    }))
}

/// Pack three rounded/clamped 0-255 channels into the `{r, g, b, hex}` JSON the
/// color ops return. Centralizes the shape so every op emits an identical object.
fn rgb_out(r: u8, g: u8, b: u8) -> Value {
    json!({"r": r, "g": g, "b": b, "hex": format!("#{r:02x}{g:02x}{b:02x}")})
}

/// Round a float channel to the nearest 0-255 byte (clamping out-of-range).
fn clamp_u8(x: f64) -> u8 {
    x.round().clamp(0.0, 255.0) as u8
}

/// Invert an RGB color (`color`/`a` as `[r,g,b]` or `{r,g,b}`): each channel
/// becomes `255 - c`, the photographic-negative / RGB-complement. opts: `color`.
/// Returns `{r, g, b, hex}`. Pure.
fn op_invert(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let inv = |c: i64| (255 - c.clamp(0, 255)) as u8;
    Ok(rgb_out(inv(r), inv(g), inv(b)))
}

/// Convert an RGB color to grayscale using the Rec. 709 luma weights
/// (`0.2126 R + 0.7152 G + 0.0722 B`) — the same channel weighting WCAG
/// luminance uses, applied directly in sRGB (gamma-naive) so the gray reads as
/// the perceived brightness. opts: `color`/`a`. Returns `{r, g, b, hex, gray}`
/// where the three channels share the single 0-255 `gray` value. Pure.
fn op_grayscale(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let y = 0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64;
    let gray = clamp_u8(y);
    let mut out = rgb_out(gray, gray, gray);
    out["gray"] = json!(gray);
    Ok(out)
}

/// Apply the classic sepia-tone color matrix to an RGB color — the warm
/// brown-tinted look of aged photographs. Each output channel is a fixed linear
/// combination of the input (R'=0.393R+0.769G+0.189B, G'=0.349R+0.686G+0.168B,
/// B'=0.272R+0.534G+0.131B), clamped to 0-255. opts: `color`/`a`. Returns
/// `{r, g, b, hex}`. Pure.
fn op_sepia(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (ri, gi, bi) = rgb_of(src)?;
    let (r, g, b) = (ri as f64, gi as f64, bi as f64);
    let nr = 0.393 * r + 0.769 * g + 0.189 * b;
    let ng = 0.349 * r + 0.686 * g + 0.168 * b;
    let nb = 0.272 * r + 0.534 * g + 0.131 * b;
    Ok(rgb_out(clamp_u8(nr), clamp_u8(ng), clamp_u8(nb)))
}

/// Quantize each channel of an RGB color to `levels` evenly-spaced steps — the
/// poster / banding effect. With `levels` n (clamped to [2,256]), each channel
/// snaps to the nearest of n values spread across 0-255 (so `levels=2` gives a
/// pure 0/255 two-tone). opts: `color`/`a`, `levels` (default 4). Returns
/// `{r, g, b, hex, levels}`. Pure.
fn op_posterize(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let levels = v
        .get("levels")
        .and_then(Value::as_i64)
        .unwrap_or(4)
        .clamp(2, 256);
    let step = (levels - 1) as f64;
    let q = |c: i64| -> u8 {
        let c = c.clamp(0, 255) as f64 / 255.0;
        clamp_u8((c * step).round() / step * 255.0)
    };
    let mut out = rgb_out(q(r), q(g), q(b));
    out["levels"] = json!(levels);
    Ok(out)
}

/// Shift an RGB color's warmth by `amount` (or `delta`): a positive value warms
/// it (adds to red, subtracts from blue), a negative value cools it (the
/// reverse), with green untouched and every channel clamped to 0-255. A simple
/// additive white-balance nudge. opts: `color`/`a`, `amount`. Returns
/// `{r, g, b, hex}`. Pure.
fn op_temperature(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let amount = v
        .get("amount")
        .or_else(|| v.get("delta"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing amount"))?;
    let nr = clamp_u8(r as f64 + amount);
    let nb = clamp_u8(b as f64 - amount);
    Ok(rgb_out(nr, g.clamp(0, 255) as u8, nb))
}

/// Composite a foreground color over a background ("source-over"): each channel
/// is `fg*alpha + bg*(1-alpha)`. opts: `fg` (or `a`), `bg` (or `b`) — each
/// `[r,g,b]` or `{r,g,b}` — and `alpha` (0-1, default 1; clamped). `alpha` 1
/// returns `fg`, 0 returns `bg`. The straight-alpha blend behind translucent
/// overlays. Returns `{r, g, b, hex}`. Pure.
fn op_alpha_over(v: Value) -> Result<Value> {
    let fg = v.get("fg").or_else(|| v.get("a")).unwrap_or(&Value::Null);
    let bg = v.get("bg").or_else(|| v.get("b")).unwrap_or(&Value::Null);
    let (fr, fgn, fb) = rgb_of(fg)?;
    let (br, bgn, bb) = rgb_of(bg)?;
    let alpha = v
        .get("alpha")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let over = |f: i64, b: i64| clamp_u8(f as f64 * alpha + b as f64 * (1.0 - alpha));
    Ok(rgb_out(over(fr, br), over(fgn, bgn), over(fb, bb)))
}

/// Whether an RGB color is "dark" by WCAG relative luminance — `true` when the
/// luminance is below `threshold` (default 0.5). The common pick-a-text-color
/// test: a dark background wants light text and vice versa. opts: `color`/`a`,
/// `threshold` (0-1). Returns `{dark, luminance, threshold, text}` where `text`
/// is the suggested `"white"`/`"black"` foreground. Pure.
fn op_is_dark(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let threshold = v
        .get("threshold")
        .and_then(Value::as_f64)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let lum = relative_luminance(r, g, b);
    let dark = lum < threshold;
    Ok(json!({
        "dark": dark,
        "luminance": lum,
        "threshold": threshold,
        "text": if dark { "white" } else { "black" },
    }))
}

/// Convert an RGB color to CIELAB (D65) — the perceptually-uniform space the
/// `delta_e` formula works in, exposed directly. opts: `color`/`a`. Returns
/// `{l, a, b}` (`l` in [0,100], `a`/`b` roughly ±128). The inverse is
/// `from_lab`. Pure.
fn op_to_lab(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let (l, a, bb) = rgb_to_lab(r, g, b);
    Ok(json!({"l": l, "a": a, "b": bb}))
}

/// Convert one 0-1 linear-light channel back to an 0-255 sRGB byte — the inverse
/// of `srgb_to_linear`, per IEC 61966-2-1 (the 0.0031308 companding breakpoint).
fn linear_to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    clamp_u8(s * 255.0)
}

/// Convert a CIELAB (D65) color back to 0-255 sRGB — the inverse of `to_lab`
/// (Lab → XYZ via the CIE ε/κ nonlinearity and the D65 white, XYZ → linear sRGB
/// via the Lindbloom inverse matrix, then sRGB companding).
fn lab_to_rgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let finv = |t: f64| -> f64 {
        const EPS: f64 = 216.0 / 24389.0;
        const KAPPA: f64 = 24389.0 / 27.0;
        let t3 = t * t * t;
        if t3 > EPS {
            t3
        } else {
            (116.0 * t - 16.0) / KAPPA
        }
    };
    let (x, y, z) = (finv(fx) * 0.950489, finv(fy), finv(fz) * 1.088840);
    let rl = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let gl = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    (linear_to_srgb(rl), linear_to_srgb(gl), linear_to_srgb(bl))
}

/// Convert a CIELAB (D65) color back to RGB — the inverse of `to_lab`. opts: `l`
/// (0-100), `a`, `b`. Out-of-gamut results are clamped per channel. Returns
/// `{r, g, b, hex}`. Pure.
fn op_from_lab(v: Value) -> Result<Value> {
    let l = v.get("l").and_then(Value::as_f64).unwrap_or(0.0);
    let a = v.get("a").and_then(Value::as_f64).unwrap_or(0.0);
    let b = v.get("b").and_then(Value::as_f64).unwrap_or(0.0);
    let (r, g, bb) = lab_to_rgb(l, a, b);
    Ok(rgb_out(r, g, bb))
}

/// Blend two RGB colors in CIELAB space by `weight` (the fraction of `b`,
/// default 0.5, clamped to [0,1]) — the perceptual counterpart of `mix` (which
/// interpolates in sRGB). Each color is converted to Lab, the L/a/b coordinates
/// are linearly interpolated, and the result is converted back. opts: `a`, `b`
/// (each `[r,g,b]` or `{r,g,b}`), `weight`. `weight` 0 returns `a`, 1 returns
/// `b`. Returns `{r, g, b, hex}`. Pure.
fn op_blend_lab(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let w = v
        .get("weight")
        .and_then(Value::as_f64)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let (l1, a1, b1) = rgb_to_lab(ar, ag, ab);
    let (l2, a2, b2) = rgb_to_lab(br, bg, bb);
    let lerp = |x: f64, y: f64| x * (1.0 - w) + y * w;
    let (r, g, b) = lab_to_rgb(lerp(l1, l2), lerp(a1, a2), lerp(b1, b2));
    Ok(rgb_out(r, g, b))
}

/// Build an evenly-spaced gradient from color `a` to color `b` (each `[r,g,b]`
/// or `{r,g,b}`) with `steps` stops (clamped to [2,256], default 5) interpolated
/// in sRGB — endpoints inclusive, so a 2-stop gradient is just `[a, b]`. The
/// per-stop blend matches `mix`. opts: `a`, `b`, `steps`. Returns
/// `{stops: [{r,g,b,hex}, ...], steps}`. Pure.
fn op_gradient(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let steps = v
        .get("steps")
        .and_then(Value::as_i64)
        .unwrap_or(5)
        .clamp(2, 256);
    let denom = (steps - 1) as f64;
    let stops: Vec<Value> = (0..steps)
        .map(|i| {
            let w = i as f64 / denom;
            let lerp = |x: i64, y: i64| clamp_u8(x as f64 * (1.0 - w) + y as f64 * w);
            rgb_out(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
        })
        .collect();
    Ok(json!({"stops": stops, "steps": steps}))
}

/// Convert an 0-255 sRGB color to CIE XYZ (D65) — the linear tristimulus space
/// every other colorimetric space (`Lab`, `Oklab`) bridges through. Linear sRGB →
/// XYZ uses the standard sRGB/D65 matrix (Bruce Lindbloom), the same matrix
/// `rgb_to_lab` applies internally before the Lab nonlinearity. Returns the
/// nominally-[0,1] tristimulus values (white ≈ X 0.95047, Y 1, Z 1.08883).
fn rgb_to_xyz(r: i64, g: i64, b: i64) -> (f64, f64, f64) {
    let (rl, gl, bl) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    (
        0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl,
        0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl,
        0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl,
    )
}

/// Convert CIE XYZ (D65) back to 0-255 sRGB — the inverse of `rgb_to_xyz` via the
/// Lindbloom inverse matrix plus sRGB companding. Out-of-gamut channels clamp.
fn xyz_to_rgb(x: f64, y: f64, z: f64) -> (u8, u8, u8) {
    let rl = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let gl = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    (linear_to_srgb(rl), linear_to_srgb(gl), linear_to_srgb(bl))
}

/// Convert an 0-255 sRGB color to Oklab (Björn Ottosson, 2020; CSS Color 4 §9) —
/// a perceptually-uniform space like CIELAB but with markedly better hue
/// linearity and lightness prediction. Linear sRGB → LMS via Ottosson's M1,
/// cube-root nonlinearity, then LMS' → Lab via M2. `L` is in [0,1]; `a`/`b` are
/// roughly ±0.4 (green↔red / blue↔yellow opponent axes).
fn rgb_to_oklab(r: i64, g: i64, b: i64) -> (f64, f64, f64) {
    let (rl, gl, bl) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let l = 0.4122214708 * rl + 0.5363325363 * gl + 0.0514459929 * bl;
    let m = 0.2119034982 * rl + 0.6806995451 * gl + 0.1073969566 * bl;
    let s = 0.0883024619 * rl + 0.2817188376 * gl + 0.6299787005 * bl;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )
}

/// Convert an Oklab color back to 0-255 sRGB — the inverse of `rgb_to_oklab` (the
/// M2/M1 inverse matrices plus the cube). Out-of-gamut channels clamp.
fn oklab_to_rgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (lc, mc, sc) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let rl = 4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc;
    let gl = -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc;
    let bl = -0.0041960863 * lc - 0.7034186147 * mc + 1.7076147010 * sc;
    (linear_to_srgb(rl), linear_to_srgb(gl), linear_to_srgb(bl))
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to CIE XYZ
/// (D65) — the linear tristimulus space `to_lab` and `to_oklab` bridge through,
/// exposed directly. opts: `color`/`a`. Returns `{x, y, z}` (nominally [0,1];
/// white ≈ 0.95047, 1, 1.08883). The inverse is `from_xyz`. Pure.
fn op_to_xyz(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let (x, y, z) = rgb_to_xyz(r, g, b);
    Ok(json!({ "x": x, "y": y, "z": z }))
}

/// Convert a CIE XYZ (D65) color back to RGB — the inverse of `to_xyz`. opts: `x`,
/// `y`, `z` (nominally [0,1]). Out-of-gamut results clamp per channel. Returns
/// `{r, g, b, hex}`. Pure.
fn op_from_xyz(v: Value) -> Result<Value> {
    let x = v.get("x").and_then(Value::as_f64).unwrap_or(0.0);
    let y = v.get("y").and_then(Value::as_f64).unwrap_or(0.0);
    let z = v.get("z").and_then(Value::as_f64).unwrap_or(0.0);
    let (r, g, b) = xyz_to_rgb(x, y, z);
    Ok(rgb_out(r, g, b))
}

/// Convert an RGB color (`[r,g,b]` or `{r,g,b}`, components 0-255) to Oklab — the
/// modern perceptually-uniform space (Ottosson 2020, CSS Color 4) with better hue
/// linearity than CIELAB. opts: `color`/`a`. Returns `{l, a, b}` (`l` in [0,1],
/// `a`/`b` ≈ ±0.4). The inverse is `from_oklab`. Pure.
fn op_to_oklab(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let (l, a, bb) = rgb_to_oklab(r, g, b);
    Ok(json!({ "l": l, "a": a, "b": bb }))
}

/// Convert an Oklab color back to RGB — the inverse of `to_oklab`. opts: `l`
/// ([0,1]), `a`, `b` (≈ ±0.4). Out-of-gamut results clamp per channel. Returns
/// `{r, g, b, hex}`. Pure.
fn op_from_oklab(v: Value) -> Result<Value> {
    let l = v.get("l").and_then(Value::as_f64).unwrap_or(0.0);
    let a = v.get("a").and_then(Value::as_f64).unwrap_or(0.0);
    let b = v.get("b").and_then(Value::as_f64).unwrap_or(0.0);
    let (r, g, bb) = oklab_to_rgb(l, a, b);
    Ok(rgb_out(r, g, bb))
}

/// Blend two RGB colors in Oklab space by `weight` (the fraction of `b`, default
/// 0.5, clamped to [0,1]) — the most perceptually-even of the three blends
/// (`mix` in sRGB, `blend_lab` in CIELAB, this in Oklab). Each color is converted
/// to Oklab, the L/a/b coordinates linearly interpolated, and the result
/// converted back. opts: `a`, `b` (each `[r,g,b]` or `{r,g,b}`), `weight`.
/// `weight` 0 returns `a`, 1 returns `b`. Returns `{r, g, b, hex}`. Pure.
fn op_blend_oklab(v: Value) -> Result<Value> {
    let (ar, ag, ab) = rgb_of(v.get("a").unwrap_or(&Value::Null))?;
    let (br, bg, bb) = rgb_of(v.get("b").unwrap_or(&Value::Null))?;
    let w = v
        .get("weight")
        .and_then(Value::as_f64)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let (l1, a1, b1) = rgb_to_oklab(ar, ag, ab);
    let (l2, a2, b2) = rgb_to_oklab(br, bg, bb);
    let lerp = |x: f64, y: f64| x * (1.0 - w) + y * w;
    let (r, g, b) = oklab_to_rgb(lerp(l1, l2), lerp(a1, a2), lerp(b1, b2));
    Ok(rgb_out(r, g, b))
}

/// Generate a colour scheme from a base `color` by rotating its hue in HSL — the
/// composite designers reach for (where `rotate_hue` is the single-step
/// primitive). opts: `color` (any form `parse_color`/`rgb_of` accepts) and
/// `scheme`: `complementary` (base + 180°), `triadic` (±120°),
/// `analogous` (±30°), `split` (split-complementary, 150°/210°),
/// `tetradic` (rectangle, +60°/+180°/+240°), or `square` (+90°/+180°/+270°). The
/// base colour is always the first entry; a grey (saturation 0) yields copies of
/// itself. Returns `{scheme, colors: [{r,g,b,hex}, ...]}`. Pure.
fn op_scheme(v: Value) -> Result<Value> {
    let color = v
        .get("color")
        .or_else(|| v.get("a"))
        .cloned()
        .unwrap_or(Value::Null);
    let scheme = v
        .get("scheme")
        .and_then(Value::as_str)
        .unwrap_or("complementary");
    let offsets: &[f64] = match scheme {
        "complementary" | "complement" => &[0.0, 180.0],
        "triadic" | "triad" => &[0.0, 120.0, 240.0],
        "analogous" => &[0.0, 30.0, -30.0],
        "split" | "split-complementary" => &[0.0, 150.0, 210.0],
        "tetradic" | "rectangle" => &[0.0, 60.0, 180.0, 240.0],
        "square" => &[0.0, 90.0, 180.0, 270.0],
        other => {
            return Err(anyhow::anyhow!(
                "unknown scheme `{other}` (want complementary, triadic, analogous, split, tetradic, or square)"
            ))
        }
    };
    let colors: Vec<Value> = offsets
        .iter()
        .map(|&deg| {
            let mut c = op_rotate_hue(json!({ "color": color, "degrees": deg }))?;
            // `rotate_hue` adds an `h` field; drop it so each entry is the {r,g,b,hex} shape.
            if let Some(obj) = c.as_object_mut() {
                obj.remove("h");
            }
            Ok(c)
        })
        .collect::<Result<_>>()?;
    Ok(json!({ "scheme": scheme, "colors": colors }))
}

/// CSS basic + extended named colors used by `name_color`: the 16 HTML 4.01
/// names plus a few common CSS Color extras. Each is `(name, r, g, b)`. Kept
/// compact and exact so the nearest-match is auditable.
const NAMED_COLORS: &[(&str, u8, u8, u8)] = &[
    ("black", 0, 0, 0),
    ("white", 255, 255, 255),
    ("red", 255, 0, 0),
    ("lime", 0, 255, 0),
    ("blue", 0, 0, 255),
    ("yellow", 255, 255, 0),
    ("cyan", 0, 255, 255),
    ("magenta", 255, 0, 255),
    ("silver", 192, 192, 192),
    ("gray", 128, 128, 128),
    ("maroon", 128, 0, 0),
    ("olive", 128, 128, 0),
    ("green", 0, 128, 0),
    ("purple", 128, 0, 128),
    ("teal", 0, 128, 128),
    ("navy", 0, 0, 128),
    ("orange", 255, 165, 0),
    ("pink", 255, 192, 203),
    ("brown", 165, 42, 42),
    ("gold", 255, 215, 0),
];

/// Name the nearest CSS named color to an RGB `color` by perceptual distance —
/// the human-readable label `parse_color` can't give (it only round-trips
/// hex/`rgb()`). Each candidate in `NAMED_COLORS` is compared by ΔE00 (CIEDE2000,
/// the same metric `delta_e` exposes), so the match tracks how the colors *look*.
/// opts: `color`/`a` (any `[r,g,b]` or `{r,g,b}`). Returns `{name, delta_e,
/// r, g, b, hex}` for the closest name (`delta_e` 0 = an exact named color). Pure.
fn op_name_color(v: Value) -> Result<Value> {
    let src = v
        .get("color")
        .or_else(|| v.get("a"))
        .unwrap_or(&Value::Null);
    let (r, g, b) = rgb_of(src)?;
    let (l1, a1, b1) = rgb_to_lab(r, g, b);
    let mut best: Option<(&str, u8, u8, u8, f64)> = None;
    for &(name, nr, ng, nb) in NAMED_COLORS {
        let (l2, a2, b2) = rgb_to_lab(nr as i64, ng as i64, nb as i64);
        let de = ciede2000(l1, a1, b1, l2, a2, b2);
        if best.is_none_or(|(.., bd)| de < bd) {
            best = Some((name, nr, ng, nb, de));
        }
    }
    let (name, nr, ng, nb, de) = best.expect("NAMED_COLORS is non-empty");
    let de = (de * 10000.0).round() / 10000.0;
    Ok(json!({
        "name": name,
        "delta_e": de,
        "r": nr,
        "g": ng,
        "b": nb,
        "hex": format!("#{nr:02x}{ng:02x}{nb:02x}"),
    }))
}

#[no_mangle]
pub extern "C" fn gui__invert(args: *const c_char) -> *const c_char {
    ffi_call(args, op_invert)
}

#[no_mangle]
pub extern "C" fn gui__grayscale(args: *const c_char) -> *const c_char {
    ffi_call(args, op_grayscale)
}

#[no_mangle]
pub extern "C" fn gui__sepia(args: *const c_char) -> *const c_char {
    ffi_call(args, op_sepia)
}

#[no_mangle]
pub extern "C" fn gui__posterize(args: *const c_char) -> *const c_char {
    ffi_call(args, op_posterize)
}

#[no_mangle]
pub extern "C" fn gui__temperature(args: *const c_char) -> *const c_char {
    ffi_call(args, op_temperature)
}

#[no_mangle]
pub extern "C" fn gui__alpha_over(args: *const c_char) -> *const c_char {
    ffi_call(args, op_alpha_over)
}

#[no_mangle]
pub extern "C" fn gui__is_dark(args: *const c_char) -> *const c_char {
    ffi_call(args, op_is_dark)
}

#[no_mangle]
pub extern "C" fn gui__to_lab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_lab)
}

#[no_mangle]
pub extern "C" fn gui__from_lab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_lab)
}

#[no_mangle]
pub extern "C" fn gui__blend_lab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_blend_lab)
}

#[no_mangle]
pub extern "C" fn gui__gradient(args: *const c_char) -> *const c_char {
    ffi_call(args, op_gradient)
}

#[no_mangle]
pub extern "C" fn gui__parse_hotkey(args: *const c_char) -> *const c_char {
    ffi_call(args, op_parse_hotkey)
}

#[no_mangle]
pub extern "C" fn gui__format_hotkey(args: *const c_char) -> *const c_char {
    ffi_call(args, op_format_hotkey)
}

#[no_mangle]
pub extern "C" fn gui__to_hsv(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_hsv)
}

#[no_mangle]
pub extern "C" fn gui__from_hsv(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_hsv)
}

#[no_mangle]
pub extern "C" fn gui__to_hwb(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_hwb)
}

#[no_mangle]
pub extern "C" fn gui__from_hwb(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_hwb)
}

#[no_mangle]
pub extern "C" fn gui__to_cmyk(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_cmyk)
}

#[no_mangle]
pub extern "C" fn gui__from_cmyk(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_cmyk)
}

#[no_mangle]
pub extern "C" fn gui__parse_color(args: *const c_char) -> *const c_char {
    ffi_call(args, op_parse_color)
}

#[no_mangle]
pub extern "C" fn gui__format_color(args: *const c_char) -> *const c_char {
    ffi_call(args, op_format_color)
}

#[no_mangle]
pub extern "C" fn gui__color_distance(args: *const c_char) -> *const c_char {
    ffi_call(args, op_color_distance)
}

#[no_mangle]
pub extern "C" fn gui__delta_e(args: *const c_char) -> *const c_char {
    ffi_call(args, op_delta_e)
}

#[no_mangle]
pub extern "C" fn gui__mix(args: *const c_char) -> *const c_char {
    ffi_call(args, op_mix)
}

#[no_mangle]
pub extern "C" fn gui__rotate_hue(args: *const c_char) -> *const c_char {
    ffi_call(args, op_rotate_hue)
}

#[no_mangle]
pub extern "C" fn gui__adjust_lightness(args: *const c_char) -> *const c_char {
    ffi_call(args, op_adjust_lightness)
}

#[no_mangle]
pub extern "C" fn gui__adjust_saturation(args: *const c_char) -> *const c_char {
    ffi_call(args, op_adjust_saturation)
}

#[no_mangle]
pub extern "C" fn gui__contrast_ratio(args: *const c_char) -> *const c_char {
    ffi_call(args, op_contrast_ratio)
}

#[no_mangle]
pub extern "C" fn gui__relative_luminance(args: *const c_char) -> *const c_char {
    ffi_call(args, op_relative_luminance)
}

#[no_mangle]
pub extern "C" fn gui__to_hsl(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_hsl)
}

#[no_mangle]
pub extern "C" fn gui__from_hsl(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_hsl)
}

#[no_mangle]
pub extern "C" fn gui__to_xyz(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_xyz)
}

#[no_mangle]
pub extern "C" fn gui__from_xyz(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_xyz)
}

#[no_mangle]
pub extern "C" fn gui__to_oklab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_to_oklab)
}

#[no_mangle]
pub extern "C" fn gui__from_oklab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_from_oklab)
}

#[no_mangle]
pub extern "C" fn gui__blend_oklab(args: *const c_char) -> *const c_char {
    ffi_call(args, op_blend_oklab)
}

#[no_mangle]
pub extern "C" fn gui__scheme(args: *const c_char) -> *const c_char {
    ffi_call(args, op_scheme)
}

#[no_mangle]
pub extern "C" fn gui__name_color(args: *const c_char) -> *const c_char {
    ffi_call(args, op_name_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_and_free(p: *const c_char) -> Value {
        assert!(!p.is_null());
        let bytes = unsafe { CStr::from_ptr(p).to_bytes().to_vec() };
        unsafe { stryke_free_cstring(p as *mut c_char) };
        serde_json::from_slice(&bytes).expect("ffi return is valid JSON")
    }

    #[test]
    fn free_cstring_handles_null() {
        unsafe { stryke_free_cstring(std::ptr::null_mut()) };
    }

    #[test]
    fn ffi_call_returns_error_json_on_panic() {
        let v = read_and_free(ffi_call(std::ptr::null(), |_| -> Result<Value> {
            panic!("boom");
        }));
        assert!(v["error"].is_string());
    }

    #[test]
    fn ffi_call_returns_error_json_on_err() {
        let v = read_and_free(ffi_call(std::ptr::null(), |_| -> Result<Value> {
            Err(anyhow::anyhow!("intentional"))
        }));
        assert_eq!(v["error"].as_str().unwrap(), "intentional");
    }

    #[test]
    fn ffi_call_passes_args_to_handler() {
        // Inject `{"x":7,"y":9}`, sum the fields, expect 16 back.
        let in_str = CString::new(r#"{"x":7,"y":9}"#).unwrap();
        let out = read_and_free(ffi_call(in_str.as_ptr(), |v| {
            Ok(json!({
                "sum": v["x"].as_i64().unwrap() + v["y"].as_i64().unwrap()
            }))
        }));
        assert_eq!(out["sum"].as_i64().unwrap(), 16);
    }

    #[test]
    fn region_from_value_round_trips() {
        let v = json!({"region": [10, 20, 100, 200]});
        assert_eq!(region_from_value(&v), Some((10, 20, 100u32, 200u32)));
    }

    #[test]
    fn region_from_value_clamps_negative_size_to_zero() {
        let v = json!({"region": [0, 0, -5, -7]});
        assert_eq!(region_from_value(&v), Some((0, 0, 0u32, 0u32)));
    }

    #[test]
    fn region_from_value_rejects_short_arrays() {
        let v = json!({"region": [1, 2, 3]});
        assert!(region_from_value(&v).is_none());
    }

    /// **Bug class: off-by-one on the OTHER side of the length check.**
    /// The guard is `arr.len() != 4`, so a 5+-element region (e.g. a caller
    /// appending a stray trailing field) is rejected to `None` exactly like
    /// the short-array case. Only the short side was pinned; this nails the
    /// over-long side so a future `>= 4` / `> 4` rewrite (which would start
    /// accepting `[l, t, w, h, junk]` and read the wrong 4 slots) is caught.
    #[test]
    fn region_from_value_rejects_over_long_arrays() {
        let v = json!({"region": [1, 2, 3, 4, 5]});
        assert!(region_from_value(&v).is_none());
    }

    /// **Bug class: JSON number-type coercion silently discards the region.**
    /// `serde_json::Value::as_i64()` returns `None` for a *fractional* number
    /// (`100.5`), so a single non-integer coordinate makes `region_from_value`
    /// return `None` for the WHOLE region. Downstream (`gui__screenshot`) treats
    /// `None` as "no region" and captures the FULL screen instead of the
    /// requested crop — a silent, surprising widening, not an error. This pins
    /// that a float coordinate does not partially parse; if a future change
    /// starts `as_f64().map(|f| f as i32)`-coercing, this test forces the
    /// behavior change to be deliberate.
    #[test]
    fn region_from_value_drops_whole_region_on_fractional_coord() {
        let v = json!({"region": [0, 0, 100.5, 200]});
        assert!(
            region_from_value(&v).is_none(),
            "fractional width must collapse the region to None (full-screen \
             fallback), not silently truncate to a partial crop"
        );
    }

    /// **Bug class: stringly-typed coordinate silently discards the region.**
    /// A `.stk` caller that builds the region with a string element
    /// (`"10"` instead of `10`) hits `as_i64() == None` and the entire region
    /// is dropped to `None` → full-screen capture. Distinct from the
    /// fractional case: this is the wrong-JSON-*type* path, not the
    /// wrong-number-*subtype* path. Pins that no string→int coercion is
    /// silently performed at the FFI boundary.
    #[test]
    fn region_from_value_drops_whole_region_on_string_coord() {
        let v = json!({"region": ["10", 20, 100, 200]});
        assert!(region_from_value(&v).is_none());
    }

    #[test]
    fn region_from_value_returns_none_when_absent() {
        let v = json!({});
        assert!(region_from_value(&v).is_none());
    }

    /// `region_from_value` casts width/height with `as u32`. A JSON value
    /// that fits in i64 but exceeds u32::MAX (e.g. 5_000_000_000) will be
    /// silently truncated by `as u32` — the caller asks for a 5 GP region
    /// and `capture::screenshot_*` crops to a different small value with no
    /// error. Pins the current behavior: failing this test means someone
    /// added input validation (good), passing means the silent truncation
    /// bug is still live and should be fixed by validating range before
    /// the cast.
    #[test]
    fn region_from_value_silently_truncates_width_above_u32_max() {
        let big: i64 = (u32::MAX as i64) + 1; // 4_294_967_296
        let v = json!({"region": [0, 0, big, 100]});
        let got = region_from_value(&v).expect("should parse");
        // Bug evidence: width comes back as 0 instead of error or saturation.
        assert_eq!(got.2, 0, "width should saturate or error; got {}", got.2);
        // Height fits, sanity-check it wasn't broken.
        assert_eq!(got.3, 100);
    }

    /// Same bug class on the top-left coords: an i64 region origin past
    /// i32::MAX gets `as i32`-truncated into a small (possibly negative)
    /// value, sending the capture to a wildly different region with no
    /// error. Pin the current behavior so a future fix is intentional.
    #[test]
    fn region_from_value_silently_truncates_left_top_above_i32_max() {
        let big: i64 = (i32::MAX as i64) + 1; // 2_147_483_648
        let v = json!({"region": [big, big, 10, 10]});
        let got = region_from_value(&v).expect("should parse");
        // i32::MAX + 1 wraps to i32::MIN under `as i32`.
        assert_eq!(
            got.0,
            i32::MIN,
            "left should error or saturate; got {}",
            got.0
        );
        assert_eq!(
            got.1,
            i32::MIN,
            "top should error or saturate; got {}",
            got.1
        );
    }

    /// `ffi_call` parses the input as JSON via `serde_json::from_slice`. If
    /// parsing fails, the handler still runs with `Value::Null` instead of
    /// erroring — meaning a caller that mis-serializes its dict gets the
    /// default-fallback path silently. This pins that fallback behavior so
    /// it's intentional (not an accident), and catches a future regression
    /// that would (correctly) start erroring on garbage input — at which
    /// point this test should be updated alongside the spec change.
    #[test]
    fn ffi_call_swallows_invalid_json_into_null_value() {
        let in_str = CString::new("not json at all {{{").unwrap();
        let out = read_and_free(ffi_call(in_str.as_ptr(), |v| {
            // Handler sees Value::Null, NOT an error.
            Ok(json!({ "saw_null": v.is_null() }))
        }));
        assert!(out["saw_null"].as_bool().unwrap());
    }

    // ── pure helpers (no display / input) ────────────────────────────────────

    #[test]
    fn parse_hotkey_splits_modifiers_from_key() {
        let v = op_parse_hotkey(json!({"hotkey": "Ctrl+Shift+A"})).unwrap();
        assert_eq!(v["keys"], json!(["ctrl", "shift", "a"]), "lowercased");
        assert_eq!(v["modifiers"], json!(["ctrl", "shift"]));
        assert_eq!(v["key"], json!("a"));
        // A single key has no modifiers.
        let single = op_parse_hotkey(json!({"hotkey": "enter"})).unwrap();
        assert_eq!(single["modifiers"], json!([]));
        assert_eq!(single["key"], json!("enter"));
        assert!(op_parse_hotkey(json!({"hotkey": "  "})).is_err());
    }

    #[test]
    fn format_hotkey_canonicalizes_and_inverts_parse_hotkey() {
        // Explicit modifiers + key: aliases canonicalize, order is fixed.
        let v = op_format_hotkey(json!({"modifiers": ["cmd", "shift"], "key": "a"})).unwrap();
        assert_eq!(v["hotkey"], json!("Shift+Meta+A"));
        assert_eq!(v["modifiers"], json!(["Shift", "Meta"]));
        assert_eq!(v["key"], json!("A"));
        // Out-of-order, aliased, duplicated modifiers all normalize.
        let messy =
            op_format_hotkey(json!({"modifiers": ["option", "ctrl", "control"], "key": "Tab"}))
                .unwrap();
        assert_eq!(
            messy["hotkey"],
            json!("Control+Alt+Tab"),
            "order Control,Alt,Shift,Meta; dup dropped"
        );
        // Consumes the parse_hotkey `keys` shape directly (round-trip normalize).
        let parsed = op_parse_hotkey(json!({"hotkey": "super+alt+k"})).unwrap();
        let back = op_format_hotkey(parsed).unwrap();
        assert_eq!(back["hotkey"], json!("Alt+Meta+K"));
        // A modifier-free key just upper-cases a single letter.
        assert_eq!(
            op_format_hotkey(json!({"key": "z"})).unwrap()["hotkey"],
            json!("Z")
        );
        // Errors: unknown modifier, empty key, no key.
        assert!(op_format_hotkey(json!({"modifiers": ["hyper"], "key": "a"})).is_err());
        assert!(op_format_hotkey(json!({"key": "   "})).is_err());
        assert!(op_format_hotkey(json!({})).is_err());
    }

    #[test]
    fn parse_color_hex_and_rgb_forms() {
        let long = op_parse_color(json!({"color": "#ff8800"})).unwrap();
        assert_eq!(long["r"], json!(255));
        assert_eq!(long["g"], json!(136));
        assert_eq!(long["b"], json!(0));
        assert_eq!(long["hex"], json!("#ff8800"));
        // Shorthand #RGB expands each nibble.
        let short = op_parse_color(json!({"color": "#f80"})).unwrap();
        assert_eq!(short["hex"], json!("#ff8800"), "#f80 expands to #ff8800");
        // rgb() form.
        let rgb = op_parse_color(json!({"color": "rgb(0, 128, 255)"})).unwrap();
        assert_eq!(rgb["g"], json!(128));
        assert_eq!(rgb["hex"], json!("#0080ff"));
        assert!(
            op_parse_color(json!({"color": "rgb(0,300,0)"})).is_err(),
            "component > 255"
        );
        assert!(op_parse_color(json!({"color": "blue"})).is_err());
    }

    #[test]
    fn format_color_emits_hex_and_rgb_and_round_trips() {
        // Default format is hex; accepts [r,g,b] and {r,g,b}.
        assert_eq!(
            op_format_color(json!({"color": [255, 136, 0]})).unwrap()["formatted"],
            json!("#ff8800")
        );
        assert_eq!(
            op_format_color(json!({"color": {"r": 0, "g": 128, "b": 255}, "format": "hex"}))
                .unwrap()["formatted"],
            json!("#0080ff")
        );
        // rgb() notation.
        assert_eq!(
            op_format_color(json!({"color": [0, 128, 255], "format": "rgb"})).unwrap()["formatted"],
            json!("rgb(0, 128, 255)")
        );
        // Components clamp to 0-255.
        assert_eq!(
            op_format_color(json!({"color": [300, -5, 128]})).unwrap()["formatted"],
            json!("#ff0080")
        );
        // Both notations round-trip through parse_color.
        for fmt in ["hex", "rgb"] {
            let s = op_format_color(json!({"color": [12, 200, 90], "format": fmt})).unwrap()
                ["formatted"]
                .clone();
            let back = op_parse_color(json!({ "color": s })).unwrap();
            assert_eq!(
                [back["r"].clone(), back["g"].clone(), back["b"].clone()],
                [json!(12), json!(200), json!(90)],
                "round-trip via {fmt}"
            );
        }
        assert!(op_format_color(json!({"color": [1, 2, 3], "format": "hsl"})).is_err());
    }

    #[test]
    fn color_distance_manhattan_and_euclidean() {
        // Accepts both [r,g,b] and {r,g,b} shapes.
        let v = op_color_distance(json!({"a": [0, 0, 0], "b": {"r": 3, "g": 4, "b": 0}})).unwrap();
        assert_eq!(v["manhattan"], json!(7), "3+4+0");
        assert_eq!(v["euclidean"], json!(5.0), "3-4-5 triangle");
        let same = op_color_distance(json!({"a": [10, 20, 30], "b": [10, 20, 30]})).unwrap();
        assert_eq!(same["manhattan"], json!(0));
    }

    #[test]
    fn ciede2000_matches_published_reference_pairs() {
        // Sharma et al. (2005) test data, row 1.
        assert!(
            (ciede2000(50.0, 2.6772, -79.7751, 50.0, 0.0, -82.7485) - 2.0425).abs() < 1e-3,
            "Sharma row 1 = 2.0425"
        );
        // A blue-pair from the same dataset.
        assert!(
            (ciede2000(50.0, 3.1571, -77.2803, 50.0, 0.0, -82.7485) - 2.8615).abs() < 1e-3,
            "Sharma row 2 = 2.8615"
        );
        // michel-leonard reference pair (Sharma variant 2.91460).
        assert!(
            (ciede2000(93.1, 39.1, -1.8, 93.6, 33.8, 1.8) - 2.9146).abs() < 1e-3,
            "reference pair = 2.9146"
        );
        // Identical colors → 0; a pure-lightness gray pair has no chroma/hue term.
        assert!(ciede2000(50.0, 0.0, 0.0, 50.0, 0.0, 0.0).abs() < 1e-9);
    }

    #[test]
    fn rgb_to_lab_matches_reference_colors() {
        let (lw, aw, bw) = rgb_to_lab(255, 255, 255);
        assert!(
            (lw - 100.0).abs() < 1e-3 && aw.abs() < 0.02 && bw.abs() < 0.02,
            "white → L100"
        );
        let (lk, ak, bk) = rgb_to_lab(0, 0, 0);
        assert!(
            lk.abs() < 1e-9 && ak.abs() < 1e-9 && bk.abs() < 1e-9,
            "black → 0,0,0"
        );
        // sRGB pure red: L*53.2408 a*80.0925 b*67.2032 (standard D65 reference).
        let (lr, ar, br) = rgb_to_lab(255, 0, 0);
        assert!((lr - 53.2408).abs() < 0.05, "red L");
        assert!((ar - 80.0925).abs() < 0.05, "red a");
        assert!((br - 67.2032).abs() < 0.05, "red b");
    }

    #[test]
    fn delta_e_perceptual_difference_end_to_end() {
        let de = |a: Value, b: Value| {
            op_delta_e(json!({ "a": a, "b": b })).unwrap()["delta_e"]
                .as_f64()
                .unwrap()
        };
        // Identical colors → 0.
        assert_eq!(de(json!([10, 20, 30]), json!([10, 20, 30])), 0.0);
        // Black ↔ white spans ~100 (pure lightness, no chroma).
        assert!((de(json!([0, 0, 0]), json!([255, 255, 255])) - 100.0).abs() < 0.01);
        // The op also surfaces the CIELAB coordinates it computed.
        let full = op_delta_e(json!({"a": [255, 0, 0], "b": [0, 0, 255]})).unwrap();
        assert!((full["lab_a"]["l"].as_f64().unwrap() - 53.2408).abs() < 0.05);
        assert!(
            full["delta_e"].as_f64().unwrap() > 50.0,
            "red vs blue is a large ΔE"
        );
        assert!(op_delta_e(json!({"a": [0, 0, 0]})).is_err());
    }

    #[test]
    fn mix_blends_by_weight() {
        // Default weight 0.5: an even mix of black and white is mid-gray.
        let g = op_mix(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap();
        assert_eq!(g["hex"], json!("#808080")); // round(127.5) = 128
                                                // weight 0 returns `a`, 1 returns `b`.
        assert_eq!(
            op_mix(json!({"a": [0, 0, 0], "b": [255, 255, 255], "weight": 0.0})).unwrap()["hex"],
            json!("#000000")
        );
        assert_eq!(
            op_mix(json!({"a": [0, 0, 0], "b": [255, 255, 255], "weight": 1.0})).unwrap()["hex"],
            json!("#ffffff")
        );
        // Red↔blue even mix is purple; both color shapes accepted.
        assert_eq!(
            op_mix(json!({"a": [255, 0, 0], "b": {"r": 0, "g": 0, "b": 255}})).unwrap()["hex"],
            json!("#800080")
        );
        // A fractional weight interpolates per channel.
        let q = op_mix(json!({"a": [0, 0, 0], "b": [200, 100, 40], "weight": 0.25})).unwrap();
        assert_eq!(q["r"], json!(50));
        assert_eq!(q["g"], json!(25));
        assert_eq!(q["b"], json!(10));
        // Out-of-range weight clamps.
        assert_eq!(
            op_mix(json!({"a": [10, 10, 10], "b": [200, 200, 200], "weight": 5.0})).unwrap()["hex"],
            json!("#c8c8c8") // clamped to 1.0 → all b
        );
        // Bad/missing color shape errors.
        assert!(op_mix(json!({"a": [1, 2], "b": [3, 4, 5]})).is_err());
        assert!(op_mix(json!({"b": [3, 4, 5]})).is_err());
    }

    #[test]
    fn rotate_hue_generates_colour_schemes() {
        let hex = |color: Value, deg: f64| {
            op_rotate_hue(json!({ "color": color, "degrees": deg })).unwrap()["hex"]
                .as_str()
                .unwrap()
                .to_string()
        };
        // Red's triad: +120° → green, +240° → blue.
        assert_eq!(hex(json!([255, 0, 0]), 120.0), "#00ff00");
        assert_eq!(hex(json!([255, 0, 0]), 240.0), "#0000ff");
        // Complement (180°): red → cyan, green → magenta.
        assert_eq!(hex(json!([255, 0, 0]), 180.0), "#00ffff");
        assert_eq!(hex(json!([0, 255, 0]), 180.0), "#ff00ff");
        // Wrapping: +360° is a no-op, negative rotates the other way (== +330).
        assert_eq!(hex(json!([255, 0, 0]), 360.0), "#ff0000");
        assert_eq!(
            hex(json!([255, 0, 0]), -30.0),
            hex(json!([255, 0, 0]), 330.0)
        );
        // The resulting hue is reported and wrapped into [0,360).
        assert_eq!(
            op_rotate_hue(json!({"color": [255, 0, 0], "degrees": 120})).unwrap()["h"],
            json!(120.0)
        );
        // A grey has no chroma, so rotating its hue leaves it unchanged.
        assert_eq!(hex(json!([128, 128, 128]), 90.0), "#808080");
        // Missing degrees / color errors.
        assert!(op_rotate_hue(json!({"color": [255, 0, 0]})).is_err());
        assert!(op_rotate_hue(json!({"degrees": 90})).is_err());
    }

    #[test]
    fn adjust_lightness_lightens_and_darkens() {
        let hex = |color: Value, delta: f64| {
            op_adjust_lightness(json!({ "color": color, "delta": delta })).unwrap()["hex"]
                .as_str()
                .unwrap()
                .to_string()
        };
        // Red is HSL(0,100%,50%): +25 → HSL(0,100%,75%) = #ff8080, +50 → white.
        assert_eq!(hex(json!([255, 0, 0]), 25.0), "#ff8080");
        assert_eq!(hex(json!([255, 0, 0]), 50.0), "#ffffff");
        // A negative delta darkens; -50 takes red to black.
        assert_eq!(hex(json!([255, 0, 0]), -50.0), "#000000");
        // Hue and saturation are preserved (only l moves): red stays red-ish.
        let lighter = op_adjust_lightness(json!({"color": [255, 0, 0], "delta": 10.0})).unwrap();
        assert_eq!(lighter["l"], json!(60.0));
        // Clamping at both ends; `amount` is an alias for `delta`.
        assert_eq!(hex(json!([0, 0, 0]), 1000.0), "#ffffff");
        assert_eq!(
            op_adjust_lightness(json!({"color": [255, 255, 255], "amount": -100.0})).unwrap()
                ["hex"],
            json!("#000000")
        );
        // Missing delta / color errors.
        assert!(op_adjust_lightness(json!({"color": [255, 0, 0]})).is_err());
        assert!(op_adjust_lightness(json!({"delta": 10})).is_err());
    }

    #[test]
    fn adjust_saturation_saturates_and_desaturates() {
        let f = |color: Value, delta: f64| {
            op_adjust_saturation(json!({ "color": color, "delta": delta })).unwrap()
        };
        // Fully desaturating red (s 100 → 0) yields a mid-grey at the same lightness.
        let g = f(json!([255, 0, 0]), -100.0);
        assert_eq!(g["hex"], json!("#808080"));
        assert_eq!(g["s"], json!(0.0));
        // Red is already fully saturated; saturating further clamps at 100 and
        // leaves the color unchanged.
        let r = f(json!([255, 0, 0]), 50.0);
        assert_eq!(r["s"], json!(100.0));
        assert_eq!(r["hex"], json!("#ff0000"));
        // Hue and lightness are preserved (only s moves): l stays 50.
        let half = f(json!([255, 0, 0]), -50.0);
        assert_eq!(half["s"], json!(50.0));
        // `amount` alias; clamping at the low end.
        assert_eq!(
            op_adjust_saturation(json!({"color": [255, 0, 0], "amount": -1000.0})).unwrap()["s"],
            json!(0.0)
        );
        // Missing delta / color errors.
        assert!(op_adjust_saturation(json!({"color": [255, 0, 0]})).is_err());
        assert!(op_adjust_saturation(json!({"delta": 10})).is_err());
    }

    #[test]
    fn contrast_ratio_matches_wcag_reference_values() {
        // Black vs white is the maximum 21:1 — everything passes.
        let bw = op_contrast_ratio(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap();
        assert_eq!(bw["ratio"], json!(21.0));
        assert_eq!(bw["aa_normal"], json!(true));
        assert_eq!(bw["aaa_normal"], json!(true));
        // Order-independent.
        assert_eq!(
            op_contrast_ratio(json!({"a": [255, 255, 255], "b": [0, 0, 0]})).unwrap()["ratio"],
            json!(21.0)
        );
        // Identical colors are 1:1 — fails every threshold.
        let same = op_contrast_ratio(json!({"a": {"r": 18, "g": 52, "b": 86}, "b": [18, 52, 86]}))
            .unwrap();
        assert_eq!(same["ratio"], json!(1.0));
        assert_eq!(same["aa_large"], json!(false));
        // #777777 on white = 4.48 — just under AA normal (4.5), clears AA large.
        let gray =
            op_contrast_ratio(json!({"a": [0x77, 0x77, 0x77], "b": [255, 255, 255]})).unwrap();
        assert_eq!(gray["ratio"], json!(4.48));
        assert_eq!(gray["aa_normal"], json!(false));
        assert_eq!(gray["aa_large"], json!(true));
        // #2563eb on white = 5.17 — passes AA normal, fails AAA normal (7).
        let blue =
            op_contrast_ratio(json!({"a": [0x25, 0x63, 0xeb], "b": [255, 255, 255]})).unwrap();
        assert_eq!(blue["ratio"], json!(5.17));
        assert_eq!(blue["aa_normal"], json!(true));
        assert_eq!(blue["aaa_normal"], json!(false));
        assert!(op_contrast_ratio(json!({"a": [0, 0, 0]})).is_err());
    }

    #[test]
    fn relative_luminance_matches_wcag_endpoints() {
        let lum = |c: Value| {
            op_relative_luminance(json!({ "color": c })).unwrap()["luminance"]
                .as_f64()
                .unwrap()
        };
        // Black is 0, white is 1 (the luminance endpoints).
        assert!(lum(json!([0, 0, 0])).abs() < 1e-9);
        assert!((lum(json!([255, 255, 255])) - 1.0).abs() < 1e-9);
        // Pure channels carry their WCAG weights exactly.
        assert!(
            (lum(json!([255, 0, 0])) - 0.2126).abs() < 1e-9,
            "red weight"
        );
        assert!(
            (lum(json!([0, 255, 0])) - 0.7152).abs() < 1e-9,
            "green weight"
        );
        assert!(
            (lum(json!([0, 0, 255])) - 0.0722).abs() < 1e-9,
            "blue weight"
        );
        // Agrees with the value contrast_ratio derives: CR(c, black) = (L+0.05)/0.05.
        let l = lum(json!({"r": 0x25, "g": 0x63, "b": 0xeb}));
        let cr = op_contrast_ratio(json!({"a": [0x25, 0x63, 0xeb], "b": [0, 0, 0]})).unwrap()
            ["ratio"]
            .as_f64()
            .unwrap();
        assert!(((l + 0.05) / 0.05 * 100.0).round() / 100.0 - cr < 1e-9);
        assert!(op_relative_luminance(json!({})).is_err());
    }

    #[test]
    fn to_hsl_matches_css_reference_colors() {
        let approx = |got: &Value, key: &str, want: f64| {
            let g = got[key].as_f64().unwrap();
            assert!((g - want).abs() < 0.1, "{key}: got {g}, want {want}");
        };
        // Pure red → h0 s100 l50.
        let red = op_to_hsl(json!({"color": [255, 0, 0]})).unwrap();
        approx(&red, "h", 0.0);
        approx(&red, "s", 100.0);
        approx(&red, "l", 50.0);
        // Pure green (0,128,0) → h120 s100 l~25.1.
        let green = op_to_hsl(json!({"color": [0, 128, 0]})).unwrap();
        approx(&green, "h", 120.0);
        approx(&green, "s", 100.0);
        approx(&green, "l", 25.1);
        // White and black are achromatic (s0), l100 / l0.
        let white = op_to_hsl(json!({"color": {"r": 255, "g": 255, "b": 255}})).unwrap();
        approx(&white, "s", 0.0);
        approx(&white, "l", 100.0);
        let black = op_to_hsl(json!({"color": [0, 0, 0]})).unwrap();
        approx(&black, "s", 0.0);
        approx(&black, "l", 0.0);
        // Blue → h240.
        approx(
            &op_to_hsl(json!({"color": [0, 0, 255]})).unwrap(),
            "h",
            240.0,
        );
    }

    #[test]
    fn to_hsv_matches_reference_colors() {
        let approx = |got: &Value, key: &str, want: f64| {
            let g = got[key].as_f64().unwrap();
            assert!((g - want).abs() < 0.1, "{key}: got {g}, want {want}");
        };
        // Pure red → h0 s100 v100 (oracle-verified via colorsys).
        let red = op_to_hsv(json!({"color": [255, 0, 0]})).unwrap();
        approx(&red, "h", 0.0);
        approx(&red, "s", 100.0);
        approx(&red, "v", 100.0);
        // Green (0,128,0) → h120 s100 v~50.2 (V is the brightest channel, not the
        // HSL midpoint of ~25.1).
        let green = op_to_hsv(json!({"color": [0, 128, 0]})).unwrap();
        approx(&green, "h", 120.0);
        approx(&green, "s", 100.0);
        approx(&green, "v", 50.2);
        // Gray (128,128,128) → s0 v~50.2.
        let gray = op_to_hsv(json!({"color": {"r": 128, "g": 128, "b": 128}})).unwrap();
        approx(&gray, "s", 0.0);
        approx(&gray, "v", 50.2);
        // White v100, black v0.
        approx(
            &op_to_hsv(json!({"color": [255, 255, 255]})).unwrap(),
            "v",
            100.0,
        );
        approx(&op_to_hsv(json!({"color": [0, 0, 0]})).unwrap(), "v", 0.0);
        // Cyan (0,255,255) → h180.
        approx(
            &op_to_hsv(json!({"color": [0, 255, 255]})).unwrap(),
            "h",
            180.0,
        );
    }

    #[test]
    fn from_hsl_inverts_to_hsl_for_reference_colors() {
        // CSS reference HSL → exact RGB + hex.
        let red = op_from_hsl(json!({"h": 0, "s": 100, "l": 50})).unwrap();
        assert_eq!(red["r"], json!(255));
        assert_eq!(red["g"], json!(0));
        assert_eq!(red["b"], json!(0));
        assert_eq!(red["hex"], json!("#ff0000"));
        assert_eq!(
            op_from_hsl(json!({"h": 240, "s": 100, "l": 50})).unwrap()["hex"],
            json!("#0000ff")
        );
        // Achromatic: s0 → grey scaled by l.
        assert_eq!(
            op_from_hsl(json!({"h": 0, "s": 0, "l": 100})).unwrap()["hex"],
            json!("#ffffff")
        );
        assert_eq!(
            op_from_hsl(json!({"h": 0, "s": 0, "l": 0})).unwrap()["hex"],
            json!("#000000")
        );
        // Round-trip: RGB → to_hsl → from_hsl reproduces the original.
        for rgb in [[18, 52, 86], [200, 100, 50], [0, 128, 0]] {
            let hsl = op_to_hsl(json!({"color": rgb})).unwrap();
            let back = op_from_hsl(json!({"h": hsl["h"], "s": hsl["s"], "l": hsl["l"]})).unwrap();
            assert_eq!(
                back["r"].as_i64().unwrap(),
                rgb[0],
                "r round-trips for {rgb:?}"
            );
            assert_eq!(
                back["g"].as_i64().unwrap(),
                rgb[1],
                "g round-trips for {rgb:?}"
            );
            assert_eq!(
                back["b"].as_i64().unwrap(),
                rgb[2],
                "b round-trips for {rgb:?}"
            );
        }
        // Hue wraps; out-of-range s/l clamp.
        assert_eq!(
            op_from_hsl(json!({"h": 360, "s": 100, "l": 50})).unwrap()["hex"],
            json!("#ff0000"),
            "360° wraps to 0° (red)"
        );
    }

    #[test]
    fn from_hsv_inverts_to_hsv_for_reference_colors() {
        // Reference HSV → exact RGB + hex (v is the brightest channel).
        assert_eq!(
            op_from_hsv(json!({"h": 0, "s": 100, "v": 100})).unwrap()["hex"],
            json!("#ff0000")
        );
        assert_eq!(
            op_from_hsv(json!({"h": 120, "s": 100, "v": 100})).unwrap()["hex"],
            json!("#00ff00")
        );
        assert_eq!(
            op_from_hsv(json!({"h": 30, "s": 100, "v": 100})).unwrap()["hex"],
            json!("#ff8000"),
            "orange"
        );
        // Achromatic: s0 → grey scaled by v (HSV v is the level, not HSL's l).
        assert_eq!(
            op_from_hsv(json!({"h": 0, "s": 0, "v": 100})).unwrap()["hex"],
            json!("#ffffff")
        );
        assert_eq!(
            op_from_hsv(json!({"h": 0, "s": 0, "v": 0})).unwrap()["hex"],
            json!("#000000")
        );
        // Round-trip: RGB → to_hsv → from_hsv reproduces the original.
        for rgb in [[255, 0, 0], [0, 255, 255], [255, 128, 0], [128, 128, 128]] {
            let hsv = op_to_hsv(json!({"color": rgb})).unwrap();
            let back = op_from_hsv(json!({"h": hsv["h"], "s": hsv["s"], "v": hsv["v"]})).unwrap();
            assert_eq!(
                back["r"].as_i64().unwrap(),
                rgb[0],
                "r round-trips for {rgb:?}"
            );
            assert_eq!(
                back["g"].as_i64().unwrap(),
                rgb[1],
                "g round-trips for {rgb:?}"
            );
            assert_eq!(
                back["b"].as_i64().unwrap(),
                rgb[2],
                "b round-trips for {rgb:?}"
            );
        }
        // Hue wraps; out-of-range s/v clamp.
        assert_eq!(
            op_from_hsv(json!({"h": 360, "s": 100, "v": 100})).unwrap()["hex"],
            json!("#ff0000"),
            "360° wraps to 0° (red)"
        );
    }

    #[test]
    fn to_hwb_matches_reference_colors() {
        let hwb = |rgb: [i64; 3]| op_to_hwb(json!({ "color": rgb })).unwrap();
        // A fully saturated hue has zero whiteness and zero blackness.
        let red = hwb([255, 0, 0]);
        assert_eq!(red["h"], json!(0.0));
        assert_eq!(red["w"], json!(0.0));
        assert_eq!(red["b"], json!(0.0));
        // White: all whiteness; black: all blackness.
        assert_eq!(hwb([255, 255, 255])["w"], json!(100.0));
        assert_eq!(hwb([255, 255, 255])["b"], json!(0.0));
        assert_eq!(hwb([0, 0, 0])["b"], json!(100.0));
        assert_eq!(hwb([0, 0, 0])["w"], json!(0.0));
        // Orange keeps the HSV hue with w=b=0.
        let orange = hwb([255, 128, 0]);
        assert!((orange["h"].as_f64().unwrap() - 30.118).abs() < 0.1);
        assert_eq!(orange["w"], json!(0.0));
        assert_eq!(orange["b"], json!(0.0));
    }

    #[test]
    fn from_hwb_inverts_to_hwb_for_reference_colors() {
        let hex = |h: f64, w: f64, b: f64| {
            op_from_hwb(json!({"h": h, "w": w, "b": b})).unwrap()["hex"]
                .as_str()
                .unwrap()
                .to_string()
        };
        // Pure hues.
        assert_eq!(hex(0.0, 0.0, 0.0), "#ff0000");
        assert_eq!(hex(120.0, 0.0, 0.0), "#00ff00");
        assert_eq!(hex(30.0, 0.0, 0.0), "#ff8000");
        // Whiteness lightens, blackness darkens (red + 50%).
        assert_eq!(hex(0.0, 50.0, 0.0), "#ff8080");
        assert_eq!(hex(0.0, 0.0, 50.0), "#800000");
        // w + b >= 100% collapses to the normalized gray w/(w+b).
        assert_eq!(hex(0.0, 100.0, 0.0), "#ffffff");
        assert_eq!(hex(0.0, 0.0, 100.0), "#000000");
        assert_eq!(hex(0.0, 50.0, 50.0), "#808080");
        assert_eq!(hex(0.0, 60.0, 40.0), "#999999", "60/100 white → 0.6 gray");
        // Round-trip: RGB → to_hwb → from_hwb reproduces the original.
        for rgb in [
            [255, 0, 0],
            [0, 255, 255],
            [255, 128, 0],
            [128, 128, 128],
            [12, 200, 90],
        ] {
            let h = op_to_hwb(json!({"color": rgb})).unwrap();
            let back = op_from_hwb(json!({"h": h["h"], "w": h["w"], "b": h["b"]})).unwrap();
            assert_eq!(
                [
                    back["r"].as_i64().unwrap(),
                    back["g"].as_i64().unwrap(),
                    back["b"].as_i64().unwrap()
                ],
                rgb,
                "round-trips for {rgb:?}"
            );
        }
        // Hue wraps; out-of-range w/b clamp.
        assert_eq!(hex(360.0, 0.0, 0.0), "#ff0000", "360° wraps to red");
    }

    #[test]
    fn cmyk_round_trips_rgb() {
        // Oracle-verified reference colors (standard profile-free conversion).
        let approx = |v: &Value, key: &str, want: f64| {
            let got = v[key].as_f64().unwrap();
            assert!((got - want).abs() < 0.01, "{key}: got {got}, want {want}");
        };
        let red = op_to_cmyk(json!({"color": [255, 0, 0]})).unwrap();
        approx(&red, "c", 0.0);
        approx(&red, "m", 100.0);
        approx(&red, "y", 100.0);
        approx(&red, "k", 0.0);
        // Pure black: K=100, the CMY channels stay 0 (no divide-by-zero).
        let black = op_to_cmyk(json!({"color": [0, 0, 0]})).unwrap();
        approx(&black, "c", 0.0);
        approx(&black, "k", 100.0);
        // White: all zero.
        let white = op_to_cmyk(json!({"color": [255, 255, 255]})).unwrap();
        approx(&white, "k", 0.0);
        approx(&white, "c", 0.0);
        // A mid color, oracle: (64,128,192) → c66.667 m33.333 y0 k24.706.
        let mid = op_to_cmyk(json!({"color": {"r": 64, "g": 128, "b": 192}})).unwrap();
        approx(&mid, "c", 66.667);
        approx(&mid, "m", 33.333);
        approx(&mid, "y", 0.0);
        approx(&mid, "k", 24.706);
        // from_cmyk inverts to_cmyk exactly for every reference color.
        for rgb in [
            [255, 0, 0],
            [0, 0, 0],
            [255, 255, 255],
            [64, 128, 192],
            [255, 128, 0],
            [17, 34, 51],
        ] {
            let cmyk = op_to_cmyk(json!({ "color": rgb })).unwrap();
            let back = op_from_cmyk(json!({
                "c": cmyk["c"], "m": cmyk["m"], "y": cmyk["y"], "k": cmyk["k"],
            }))
            .unwrap();
            assert_eq!(
                back["r"].as_i64().unwrap(),
                rgb[0],
                "r round-trips for {rgb:?}"
            );
            assert_eq!(
                back["g"].as_i64().unwrap(),
                rgb[1],
                "g round-trips for {rgb:?}"
            );
            assert_eq!(
                back["b"].as_i64().unwrap(),
                rgb[2],
                "b round-trips for {rgb:?}"
            );
        }
        // Out-of-range CMYK clamps rather than producing garbage.
        // c→100 (zeroes red), m→0, y→0 (full green+blue) = cyan.
        assert_eq!(
            op_from_cmyk(json!({"c": 200, "m": -50, "y": 0, "k": 0})).unwrap()["hex"],
            json!("#00ffff"),
            "c clamps to 100 (→0 red), m/y stay 0 (→cyan)"
        );
    }

    #[test]
    fn xyz_matches_reference_and_round_trips() {
        let approx = |v: &Value, key: &str, want: f64| {
            let got = v[key].as_f64().unwrap();
            assert!((got - want).abs() < 1e-4, "{key}: got {got}, want {want}");
        };
        // D65 white is the reference white point X 0.95047, Y 1, Z 1.08883.
        let white = op_to_xyz(json!({"color": [255, 255, 255]})).unwrap();
        approx(&white, "x", 0.95047);
        approx(&white, "y", 1.0);
        approx(&white, "z", 1.08883);
        // Pure red is the first column of the sRGB→XYZ matrix.
        let red = op_to_xyz(json!({"color": [255, 0, 0]})).unwrap();
        approx(&red, "x", 0.4124564);
        approx(&red, "y", 0.2126729);
        approx(&red, "z", 0.0193339);
        // Black is the origin.
        let black = op_to_xyz(json!({"color": [0, 0, 0]})).unwrap();
        approx(&black, "x", 0.0);
        // from_xyz inverts to_xyz exactly for every reference color.
        for rgb in [
            [255, 0, 0],
            [0, 0, 0],
            [255, 255, 255],
            [18, 52, 86],
            [200, 100, 50],
            [12, 200, 90],
        ] {
            let xyz = op_to_xyz(json!({ "color": rgb })).unwrap();
            let back = op_from_xyz(json!({"x": xyz["x"], "y": xyz["y"], "z": xyz["z"]})).unwrap();
            assert_eq!(
                [
                    back["r"].as_i64().unwrap(),
                    back["g"].as_i64().unwrap(),
                    back["b"].as_i64().unwrap()
                ],
                rgb,
                "round-trips for {rgb:?}"
            );
        }
        assert!(op_to_xyz(json!({})).is_err());
    }

    #[test]
    fn oklab_matches_reference_and_round_trips() {
        let approx = |v: &Value, key: &str, want: f64| {
            let got = v[key].as_f64().unwrap();
            assert!((got - want).abs() < 1e-4, "{key}: got {got}, want {want}");
        };
        // White → L1 with near-zero opponent axes (Ottosson reference).
        let white = op_to_oklab(json!({"color": [255, 255, 255]})).unwrap();
        approx(&white, "l", 1.0);
        assert!(white["a"].as_f64().unwrap().abs() < 1e-4, "white a≈0");
        assert!(white["b"].as_f64().unwrap().abs() < 1e-4, "white b≈0");
        // sRGB pure red: L0.6280 a0.2249 b0.1258 (Ottosson 2020 reference).
        let red = op_to_oklab(json!({"color": [255, 0, 0]})).unwrap();
        approx(&red, "l", 0.6280);
        approx(&red, "a", 0.2249);
        approx(&red, "b", 0.1258);
        // Black is the origin.
        let black = op_to_oklab(json!({"color": [0, 0, 0]})).unwrap();
        approx(&black, "l", 0.0);
        // from_oklab inverts to_oklab exactly for every reference color.
        for rgb in [
            [255, 0, 0],
            [0, 0, 0],
            [255, 255, 255],
            [18, 52, 86],
            [0, 128, 0],
            [12, 200, 90],
        ] {
            let ok = op_to_oklab(json!({ "color": rgb })).unwrap();
            let bk = op_from_oklab(json!({"l": ok["l"], "a": ok["a"], "b": ok["b"]})).unwrap();
            assert_eq!(
                [
                    bk["r"].as_i64().unwrap(),
                    bk["g"].as_i64().unwrap(),
                    bk["b"].as_i64().unwrap()
                ],
                rgb,
                "round-trips for {rgb:?}"
            );
        }
        assert!(op_to_oklab(json!({})).is_err());
    }

    #[test]
    fn blend_oklab_interpolates_in_oklab_space() {
        // weight 0 returns a, 1 returns b (the endpoints are exact).
        assert_eq!(
            op_blend_oklab(json!({"a": [10, 20, 30], "b": [200, 100, 40], "weight": 0.0})).unwrap()
                ["hex"],
            json!("#0a141e")
        );
        assert_eq!(
            op_blend_oklab(json!({"a": [10, 20, 30], "b": [200, 100, 40], "weight": 1.0})).unwrap()
                ["hex"],
            json!("#c86428")
        );
        // The even black↔white blend lands at the mid-Oklab-L gray (#636363,
        // Oklab L 0.5), distinct from sRGB `mix` (#808080) — proving it
        // interpolates in Oklab, not sRGB.
        let g = op_blend_oklab(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap();
        assert_eq!(g["hex"], json!("#636363"));
        assert_ne!(
            g["hex"],
            op_mix(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap()["hex"]
        );
        // Out-of-range weight clamps; bad/missing color shapes error.
        assert_eq!(
            op_blend_oklab(json!({"a": [0, 0, 0], "b": [255, 255, 255], "weight": 9.0})).unwrap()
                ["hex"],
            json!("#ffffff")
        );
        assert!(op_blend_oklab(json!({"a": [1, 2], "b": [3, 4, 5]})).is_err());
    }

    #[test]
    fn scheme_generates_hue_rotated_palettes() {
        let hexes = |base: Value, scheme: &str| -> Vec<String> {
            op_scheme(json!({ "color": base, "scheme": scheme })).unwrap()["colors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["hex"].as_str().unwrap().to_string())
                .collect()
        };
        // The base color is always first.
        assert_eq!(hexes(json!([255, 0, 0]), "complementary")[0], "#ff0000");
        // Complementary of red is cyan (180°).
        assert_eq!(
            hexes(json!([255, 0, 0]), "complementary"),
            ["#ff0000", "#00ffff"]
        );
        // Triadic of red is red/green/blue (0/120/240°).
        assert_eq!(
            hexes(json!([255, 0, 0]), "triadic"),
            ["#ff0000", "#00ff00", "#0000ff"]
        );
        // Square adds 90/180/270°: red → chartreuse-ish, cyan, blue-violet.
        assert_eq!(hexes(json!([255, 0, 0]), "square").len(), 4);
        assert_eq!(hexes(json!([255, 0, 0]), "square")[2], "#00ffff");
        // Entry count matches the scheme arity.
        assert_eq!(hexes(json!([255, 0, 0]), "analogous").len(), 3);
        assert_eq!(hexes(json!([255, 0, 0]), "tetradic").len(), 4);
        // Each entry is the {r,g,b,hex} shape with the `h` field dropped.
        let first =
            &op_scheme(json!({"color": [255, 0, 0], "scheme": "triadic"})).unwrap()["colors"][0];
        assert!(first.get("hex").is_some());
        assert!(
            first.get("h").is_none(),
            "the rotate_hue `h` field is stripped"
        );
        // A grey has no hue to rotate, so every entry is the same grey.
        assert_eq!(
            hexes(json!([128, 128, 128]), "triadic"),
            ["#808080", "#808080", "#808080"]
        );
        // Unknown scheme / missing color errors.
        assert!(op_scheme(json!({"color": [255, 0, 0], "scheme": "spiral"})).is_err());
        assert!(op_scheme(json!({"scheme": "triadic"})).is_err());
    }

    #[test]
    fn name_color_finds_nearest_css_name() {
        // An exact named color matches itself with ΔE 0.
        let red = op_name_color(json!({"color": [255, 0, 0]})).unwrap();
        assert_eq!(red["name"], json!("red"));
        assert_eq!(red["delta_e"], json!(0.0));
        assert_eq!(red["hex"], json!("#ff0000"));
        // Endpoints land on black / white.
        assert_eq!(
            op_name_color(json!({"color": [0, 0, 0]})).unwrap()["name"],
            json!("black")
        );
        assert_eq!(
            op_name_color(json!({"color": [255, 255, 255]})).unwrap()["name"],
            json!("white")
        );
        // A near-orange snaps to "orange" (a nonzero, small ΔE).
        let near = op_name_color(json!({"color": [250, 160, 5]})).unwrap();
        assert_eq!(near["name"], json!("orange"));
        assert!(
            near["delta_e"].as_f64().unwrap() > 0.0,
            "an inexact color has ΔE > 0"
        );
        // {r,g,b} shape accepted; a mid teal-ish color resolves to "teal".
        assert_eq!(
            op_name_color(json!({"color": {"r": 0, "g": 130, "b": 130}})).unwrap()["name"],
            json!("teal")
        );
        // Missing color errors.
        assert!(op_name_color(json!({})).is_err());
    }

    #[test]
    fn invert_is_the_rgb_complement() {
        // Each channel is 255 - c, so red ↔ cyan and a mid-gray maps near itself.
        assert_eq!(
            op_invert(json!({"color": [255, 0, 0]})).unwrap()["hex"],
            json!("#00ffff")
        );
        assert_eq!(
            op_invert(json!({"color": [0, 0, 0]})).unwrap()["hex"],
            json!("#ffffff")
        );
        assert_eq!(
            op_invert(json!({"color": [255, 255, 255]})).unwrap()["hex"],
            json!("#000000")
        );
        // 18 → 237, 52 → 203, 86 → 169; accepts the {r,g,b} shape too.
        let v = op_invert(json!({"color": {"r": 18, "g": 52, "b": 86}})).unwrap();
        assert_eq!(
            [v["r"].clone(), v["g"].clone(), v["b"].clone()],
            [json!(237), json!(203), json!(169)]
        );
        // Inverting twice is the identity.
        let once = op_invert(json!({"color": [10, 120, 230]})).unwrap();
        let twice =
            op_invert(json!({"color": [once["r"].clone(), once["g"].clone(), once["b"].clone()]}))
                .unwrap();
        assert_eq!(
            [twice["r"].clone(), twice["g"].clone(), twice["b"].clone()],
            [json!(10), json!(120), json!(230)]
        );
        assert!(op_invert(json!({"color": [1, 2]})).is_err());
    }

    #[test]
    fn grayscale_uses_rec709_luma_weights() {
        // White and black are fixed points.
        let w = op_grayscale(json!({"color": [255, 255, 255]})).unwrap();
        assert_eq!(w["gray"], json!(255));
        assert_eq!(w["hex"], json!("#ffffff"));
        assert_eq!(
            op_grayscale(json!({"color": [0, 0, 0]})).unwrap()["gray"],
            json!(0)
        );
        // Pure channels carry the Rec.709 weights * 255, rounded:
        // red round(0.2126*255)=54, green round(0.7152*255)=182, blue round(0.0722*255)=18.
        assert_eq!(
            op_grayscale(json!({"color": [255, 0, 0]})).unwrap()["gray"],
            json!(54)
        );
        assert_eq!(
            op_grayscale(json!({"color": [0, 255, 0]})).unwrap()["gray"],
            json!(182)
        );
        assert_eq!(
            op_grayscale(json!({"color": [0, 0, 255]})).unwrap()["gray"],
            json!(18)
        );
        // The three channels all equal the single gray value (it's a gray).
        let g = op_grayscale(json!({"color": [12, 200, 90]})).unwrap();
        assert_eq!(g["r"], g["g"]);
        assert_eq!(g["g"], g["b"]);
        assert_eq!(g["r"], g["gray"]);
        assert!(op_grayscale(json!({})).is_err());
    }

    #[test]
    fn sepia_matrix_matches_hand_computed_values() {
        // White: R'=255*1.351=344.6→clamp 255, G'=255*1.203=306.8→255,
        // B'=255*0.937=238.9→round 239 (the classic warm clamp).
        let w = op_sepia(json!({"color": [255, 255, 255]})).unwrap();
        assert_eq!(
            [w["r"].clone(), w["g"].clone(), w["b"].clone()],
            [json!(255), json!(255), json!(239)]
        );
        // Black stays black (0 in → 0 out for every row).
        assert_eq!(
            op_sepia(json!({"color": [0, 0, 0]})).unwrap()["hex"],
            json!("#000000")
        );
        // Pure red: R'=round(0.393*255)=100, G'=round(0.349*255)=89, B'=round(0.272*255)=69.
        let r = op_sepia(json!({"color": [255, 0, 0]})).unwrap();
        assert_eq!(
            [r["r"].clone(), r["g"].clone(), r["b"].clone()],
            [json!(100), json!(89), json!(69)]
        );
        assert!(op_sepia(json!({"color": "nope"})).is_err());
    }

    #[test]
    fn posterize_quantizes_to_even_levels() {
        // levels=2 → pure 0/255 two-tone (snap each channel to the nearer end).
        let two = op_posterize(json!({"color": [10, 130, 250], "levels": 2})).unwrap();
        assert_eq!(two["levels"], json!(2));
        // 10/255≈0.039→0, 130/255≈0.51→round(0.51)=1→255, 250→255.
        assert_eq!(
            [two["r"].clone(), two["g"].clone(), two["b"].clone()],
            [json!(0), json!(255), json!(255)]
        );
        // levels=3 → steps {0, 127.5→128, 255}; 64/255≈0.251*2=0.502→round=1→128.
        let three = op_posterize(json!({"color": [64, 0, 255], "levels": 3})).unwrap();
        assert_eq!(
            [three["r"].clone(), three["g"].clone(), three["b"].clone()],
            [json!(128), json!(0), json!(255)]
        );
        // Endpoints are always representable for any level count.
        assert_eq!(
            op_posterize(json!({"color": [255, 255, 255], "levels": 7})).unwrap()["hex"],
            json!("#ffffff")
        );
        // levels below 2 clamps to 2 (avoids divide-by-zero on a single level).
        assert_eq!(
            op_posterize(json!({"color": [10, 10, 10], "levels": 1})).unwrap()["levels"],
            json!(2)
        );
        assert!(op_posterize(json!({"levels": 4})).is_err());
    }

    #[test]
    fn temperature_warms_and_cools() {
        // Positive warms: +40 to red, -40 to blue, green untouched.
        let warm = op_temperature(json!({"color": [100, 120, 140], "amount": 40})).unwrap();
        assert_eq!(
            [warm["r"].clone(), warm["g"].clone(), warm["b"].clone()],
            [json!(140), json!(120), json!(100)]
        );
        // Negative cools (the reverse).
        let cool = op_temperature(json!({"color": [100, 120, 140], "amount": -40})).unwrap();
        assert_eq!(
            [cool["r"].clone(), cool["g"].clone(), cool["b"].clone()],
            [json!(60), json!(120), json!(180)]
        );
        // Channels clamp at 0/255; `delta` is an alias for `amount`.
        let clamped = op_temperature(json!({"color": [250, 0, 5], "delta": 100})).unwrap();
        assert_eq!(
            [clamped["r"].clone(), clamped["b"].clone()],
            [json!(255), json!(0)]
        );
        assert!(op_temperature(json!({"color": [1, 2, 3]})).is_err());
    }

    #[test]
    fn alpha_over_composites_source_over() {
        // alpha 1 returns fg, alpha 0 returns bg.
        assert_eq!(
            op_alpha_over(json!({"fg": [255, 0, 0], "bg": [0, 0, 255], "alpha": 1})).unwrap()
                ["hex"],
            json!("#ff0000")
        );
        assert_eq!(
            op_alpha_over(json!({"fg": [255, 0, 0], "bg": [0, 0, 255], "alpha": 0})).unwrap()
                ["hex"],
            json!("#0000ff")
        );
        // alpha 0.5 is the per-channel midpoint (round(127.5)=128).
        let half =
            op_alpha_over(json!({"fg": [0, 0, 0], "bg": [255, 255, 255], "alpha": 0.5})).unwrap();
        assert_eq!(half["hex"], json!("#808080"));
        // Default alpha is 1 (opaque fg); `a`/`b` alias `fg`/`bg`.
        assert_eq!(
            op_alpha_over(json!({"a": [12, 34, 56], "b": [200, 200, 200]})).unwrap()["hex"],
            json!("#0c2238")
        );
        // Out-of-range alpha clamps.
        assert_eq!(
            op_alpha_over(json!({"fg": [10, 10, 10], "bg": [200, 200, 200], "alpha": 5})).unwrap()
                ["hex"],
            json!("#0a0a0a")
        );
        assert!(op_alpha_over(json!({"fg": [1, 2, 3]})).is_err());
    }

    #[test]
    fn is_dark_thresholds_relative_luminance() {
        // Black is dark → suggest white text; white is light → suggest black text.
        let black = op_is_dark(json!({"color": [0, 0, 0]})).unwrap();
        assert_eq!(black["dark"], json!(true));
        assert_eq!(black["text"], json!("white"));
        let white = op_is_dark(json!({"color": [255, 255, 255]})).unwrap();
        assert_eq!(white["dark"], json!(false));
        assert_eq!(white["text"], json!("black"));
        // The reported luminance agrees with relative_luminance and is the
        // value compared against the threshold.
        let mid = op_is_dark(json!({"color": [0, 128, 0]})).unwrap();
        let lum = mid["luminance"].as_f64().unwrap();
        assert!((lum - relative_luminance(0, 128, 0)).abs() < 1e-12);
        assert_eq!(mid["dark"], json!(lum < 0.5));
        // A custom threshold flips the verdict; it's echoed back.
        let t = op_is_dark(json!({"color": [200, 200, 200], "threshold": 0.99})).unwrap();
        assert_eq!(t["threshold"], json!(0.99));
        assert_eq!(t["dark"], json!(true));
        assert!(op_is_dark(json!({})).is_err());
    }

    #[test]
    fn to_lab_matches_rgb_to_lab_helper() {
        // The op is a thin JSON wrapper over rgb_to_lab; pin red's reference L*a*b*.
        let red = op_to_lab(json!({"color": [255, 0, 0]})).unwrap();
        assert!((red["l"].as_f64().unwrap() - 53.2408).abs() < 0.05);
        assert!((red["a"].as_f64().unwrap() - 80.0925).abs() < 0.05);
        assert!((red["b"].as_f64().unwrap() - 67.2032).abs() < 0.05);
        // White → L100, near-zero chroma; black → all zero.
        let w = op_to_lab(json!({"color": [255, 255, 255]})).unwrap();
        assert!((w["l"].as_f64().unwrap() - 100.0).abs() < 1e-3);
        assert!(w["a"].as_f64().unwrap().abs() < 0.02);
        let k = op_to_lab(json!({"color": [0, 0, 0]})).unwrap();
        assert!(k["l"].as_f64().unwrap().abs() < 1e-9);
        assert!(op_to_lab(json!({"color": [1, 2]})).is_err());
    }

    #[test]
    fn from_lab_inverts_to_lab() {
        // Reference endpoints.
        assert_eq!(
            op_from_lab(json!({"l": 0, "a": 0, "b": 0})).unwrap()["hex"],
            json!("#000000")
        );
        assert_eq!(
            op_from_lab(json!({"l": 100, "a": 0, "b": 0})).unwrap()["hex"],
            json!("#ffffff")
        );
        // Round-trip: RGB → to_lab → from_lab reproduces the original (±1 LSB
        // from the float round-trip is acceptable for an 8-bit pipeline).
        for rgb in [
            [255, 0, 0],
            [0, 128, 0],
            [0, 0, 255],
            [18, 52, 86],
            [200, 100, 50],
            [12, 200, 90],
        ] {
            let lab = op_to_lab(json!({"color": rgb})).unwrap();
            let back = op_from_lab(json!({"l": lab["l"], "a": lab["a"], "b": lab["b"]})).unwrap();
            for (i, key) in ["r", "g", "b"].iter().enumerate() {
                let got = back[*key].as_i64().unwrap();
                assert!(
                    (got - rgb[i]).abs() <= 1,
                    "{key} round-trips for {rgb:?}: got {got} want {}",
                    rgb[i]
                );
            }
        }
        // Out-of-gamut Lab clamps per channel rather than wrapping.
        let oog = op_from_lab(json!({"l": 200, "a": 0, "b": 0})).unwrap();
        assert_eq!(oog["hex"], json!("#ffffff"));
    }

    #[test]
    fn blend_lab_interpolates_in_lab_space() {
        // Endpoints are exact (weight 0 → a, weight 1 → b).
        assert_eq!(
            op_blend_lab(json!({"a": [255, 0, 0], "b": [0, 0, 255], "weight": 0})).unwrap()["hex"],
            json!("#ff0000")
        );
        assert_eq!(
            op_blend_lab(json!({"a": [255, 0, 0], "b": [0, 0, 255], "weight": 1})).unwrap()["hex"],
            json!("#0000ff")
        );
        // Black↔white midpoint in Lab is L=50, which is a darker gray than the
        // sRGB midpoint #808080 that `mix` produces — that difference IS the
        // reason this op exists. L=50 → linear ~0.184 → sRGB ~0.466 → ~119.
        let lab_mid = op_blend_lab(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap();
        let srgb_mid = op_mix(json!({"a": [0, 0, 0], "b": [255, 255, 255]})).unwrap();
        let lm = lab_mid["r"].as_i64().unwrap();
        assert!((lm - 119).abs() <= 1, "Lab midpoint gray ≈119, got {lm}");
        assert!(
            lm < srgb_mid["r"].as_i64().unwrap(),
            "Lab midpoint is darker than the sRGB midpoint"
        );
        assert!(op_blend_lab(json!({"a": [1, 2]})).is_err());
    }

    #[test]
    fn gradient_emits_inclusive_evenly_spaced_stops() {
        // A 2-stop gradient is just [a, b].
        let two = op_gradient(json!({"a": [0, 0, 0], "b": [255, 255, 255], "steps": 2})).unwrap();
        let stops = two["stops"].as_array().unwrap();
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0]["hex"], json!("#000000"));
        assert_eq!(stops[1]["hex"], json!("#ffffff"));
        // A 5-stop black→white gradient: round(255*i/4) = 0, 64, 128, 191, 255.
        let five = op_gradient(json!({"a": [0, 0, 0], "b": [255, 255, 255], "steps": 5})).unwrap();
        let s = five["stops"].as_array().unwrap();
        assert_eq!(s.len(), 5);
        let grays: Vec<i64> = s.iter().map(|c| c["r"].as_i64().unwrap()).collect();
        assert_eq!(grays, vec![0, 64, 128, 191, 255]);
        assert_eq!(five["steps"], json!(5));
        // Endpoints always match a and b exactly regardless of step count.
        let g = op_gradient(json!({"a": [255, 0, 0], "b": [0, 0, 255], "steps": 9})).unwrap();
        let gs = g["stops"].as_array().unwrap();
        assert_eq!(gs.first().unwrap()["hex"], json!("#ff0000"));
        assert_eq!(gs.last().unwrap()["hex"], json!("#0000ff"));
        // steps below 2 clamps to 2.
        assert_eq!(
            op_gradient(json!({"a": [0, 0, 0], "b": [1, 1, 1], "steps": 1})).unwrap()["steps"],
            json!(2)
        );
        assert!(op_gradient(json!({"a": [1, 2], "b": [3, 4, 5]})).is_err());
    }
}

/// Bug-class pins for the **public FFI dispatch contract** — what `.stk`
/// scripts and stryke's `rust_ffi.rs::load_cdylib` bridge depend on.
///
/// These are deliberately separated from `mod tests` above: those tests
/// cover internal helpers (`region_from_value`, `ffi_call`); this module
/// tests the `extern "C"` exports themselves the way the dlopen consumer
/// would, surfacing regressions that internal-helper tests would miss.
///
/// Nothing here exercises a real display or input device — `gui__key_keys`
/// is a constant-string-table return, so it runs identically on headless
/// CI runners (Linux + macOS GitHub Actions).
#[cfg(test)]
mod ffi_dispatch_invariants {
    use super::*;

    /// Free an FFI return pointer and parse the bytes as JSON. Mirrors what
    /// stryke's bridge does on every call.
    fn read_and_free(p: *const c_char) -> Value {
        assert!(!p.is_null(), "FFI export returned a null pointer");
        let bytes = unsafe { CStr::from_ptr(p).to_bytes().to_vec() };
        unsafe { stryke_free_cstring(p as *mut c_char) };
        serde_json::from_slice(&bytes).expect("FFI return must be valid JSON")
    }

    /// **Bug class: public discovery contract drift.**
    ///
    /// `gui__key_keys` is how `.stk` scripts learn which key names they can
    /// pass to `gui key press/down/up`. The cdylib commits to returning a
    /// JSON **array of strings** — refactoring `KEYBOARD_KEY_NAMES` to a
    /// struct, enum, or object (or accidentally wrapping it in
    /// `{"names": [...]}`) would silently break every script that iterates
    /// the return value as an array.
    ///
    /// Not a mirror test: a mirror would just compare lengths against the
    /// constant. This test asserts the **JSON shape contract** (array,
    /// every element is a string, contains specific cross-platform anchor
    /// names) AND the **FFI memory contract** (non-null pointer, valid
    /// UTF-8, freeable via `stryke_free_cstring`). A regression that
    /// changed the export to return a JSON object or omitted an anchor key
    /// would fail here.
    #[test]
    fn gui_key_keys_returns_json_array_of_strings_with_cross_platform_anchors() {
        let v = read_and_free(gui__key_keys(std::ptr::null()));
        let arr = v
            .as_array()
            .expect("gui__key_keys must return a JSON array (not object/null)");
        assert!(
            !arr.is_empty(),
            "gui__key_keys must not return an empty array (would mean .stk discovery is dead)"
        );
        for (i, e) in arr.iter().enumerate() {
            assert!(
                e.is_string(),
                "gui__key_keys element {i} must be a string, got {e:?}"
            );
        }
        // Anchor: names that must be in the table on every supported OS. If
        // someone reorders the const into a struct, these go missing.
        let set: std::collections::HashSet<&str> = arr.iter().filter_map(|e| e.as_str()).collect();
        for anchor in ["enter", "tab", "space", "ctrl", "shift", "f1"] {
            assert!(
                set.contains(anchor),
                "gui__key_keys output missing anchor {anchor:?} — public discovery contract broke"
            );
        }
    }

    /// **Bug class: FFI tolerates non-object top-level JSON without panic.**
    ///
    /// stryke's bridge always passes a NUL-terminated CString, but a bare
    /// JSON literal at top level (`42`, `"hi"`, `true`, `null`, `[]`) is
    /// still legal JSON. `ffi_call` parses with `serde_json::from_slice`
    /// and the handler then indexes the result with `v["x"]`. If any
    /// handler's parse path assumed object-only input via
    /// `as_object().unwrap()` we'd panic — though the panic would be
    /// swallowed into a `{"error": "panicked"}` JSON return. This test
    /// pins that **bare top-level JSON literals do NOT panic the FFI**
    /// and produce the valid array return.
    ///
    /// Not a mirror: tests the behavior across multiple non-object
    /// top-level shapes (`42`, `true`, `[]`) in one shot, against the real
    /// extern symbol (`gui__key_keys` ignores args, so it's safe — it
    /// can't trigger real GUI side effects). Catches a regression where
    /// someone tightens `ffi_call` to require object input without
    /// updating the panic-catch arm.
    #[test]
    fn gui_key_keys_tolerates_non_object_args_without_panic() {
        for bad in ["42", "true", "null", "\"hi\"", "[]"] {
            let cs = CString::new(bad).unwrap();
            let v = read_and_free(gui__key_keys(cs.as_ptr()));
            // gui__key_keys ignores its input, so on ANY parseable JSON it
            // must still return the key-name array. A panic would have
            // been mapped to `{"error": "..."}` by ffi_call's
            // catch_unwind, which would fail this assertion.
            assert!(
                v.is_array(),
                "gui__key_keys must return an array for input {bad:?}, got {v:?}"
            );
        }
    }

    /// **Bug class: `stryke_free_cstring` is null-safe under repeat.**
    ///
    /// stryke's bridge guards null before calling `stryke_free_cstring`,
    /// but the cdylib export itself promises null-safety per its
    /// `# Safety` doc. The existing `free_cstring_handles_null` test
    /// calls it once; this test hammers it many times to surface any
    /// state regression (someone adds a global counter, allocator, or
    /// panic on null).
    ///
    /// Not a mirror: stresses the contract instead of just calling once.
    /// Catches a regression where the null branch starts touching shared
    /// state, or where someone replaces the early return with a
    /// `CString::from_raw(p)` that would segfault on null.
    #[test]
    fn stryke_free_cstring_null_is_idempotent_under_repeat() {
        for _ in 0..1024 {
            unsafe { stryke_free_cstring(std::ptr::null_mut()) };
        }
    }
}
