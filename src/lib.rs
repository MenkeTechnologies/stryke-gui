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
