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

    #[test]
    fn region_from_value_returns_none_when_absent() {
        let v = json!({});
        assert!(region_from_value(&v).is_none());
    }
}
