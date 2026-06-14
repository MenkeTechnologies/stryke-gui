//! Pixel reads + screenshots (xcap-backed). macOS prompts for Screen
//! Recording permission on the first capture call.

use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Serialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Serialize)]
pub struct ScreenshotRaw {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn primary_monitor() -> Result<xcap::Monitor> {
    let mons = xcap::Monitor::all().map_err(|e| anyhow!("display enumeration failed: {e}"))?;
    mons.into_iter()
        .next()
        .ok_or_else(|| anyhow!("no displays detected"))
}

pub fn pixel(x: u32, y: u32) -> Result<Rgb> {
    let mon = primary_monitor()?;
    let img = mon.capture_image()?;
    if x >= img.width() || y >= img.height() {
        return Err(anyhow!(
            "pixel({x}, {y}) out of bounds {}x{}",
            img.width(),
            img.height()
        ));
    }
    let p = img.get_pixel(x, y);
    Ok(Rgb {
        r: p[0],
        g: p[1],
        b: p[2],
    })
}

pub fn pixel_matches(x: u32, y: u32, target: (u8, u8, u8), tol: i32) -> Result<bool> {
    let mon = primary_monitor()?;
    let img = mon.capture_image()?;
    if x >= img.width() || y >= img.height() {
        return Ok(false);
    }
    let p = img.get_pixel(x, y);
    let dr = (p[0] as i32 - target.0 as i32).abs();
    let dg = (p[1] as i32 - target.1 as i32).abs();
    let db = (p[2] as i32 - target.2 as i32).abs();
    Ok(dr <= tol && dg <= tol && db <= tol)
}

/// Capture the full primary display, or a `(L, T, W, H)` region of it.
fn capture(region: Option<(i32, i32, u32, u32)>) -> Result<image::RgbaImage> {
    let mon = primary_monitor()?;
    let full = mon.capture_image()?;
    match region {
        None => Ok(full),
        Some((l, t, w, h)) => {
            Ok(image::imageops::crop_imm(&full, l.max(0) as u32, t.max(0) as u32, w, h).to_image())
        }
    }
}

/// Save the capture to `path` as PNG and return the path.
pub fn screenshot_to_file(region: Option<(i32, i32, u32, u32)>, path: &str) -> Result<String> {
    let img = capture(region)?;
    img.save(path)?;
    Ok(path.to_string())
}

/// Return the capture as a `{width, height, rgba}` JSON payload.
pub fn screenshot_raw(region: Option<(i32, i32, u32, u32)>) -> Result<ScreenshotRaw> {
    let img = capture(region)?;
    Ok(ScreenshotRaw {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

#[derive(Serialize)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub primary: bool,
}

fn monitor_by_id(id: u32) -> Result<xcap::Monitor> {
    let mons = xcap::Monitor::all().map_err(|e| anyhow!("display enumeration failed: {e}"))?;
    mons.into_iter()
        .find(|m| m.id().unwrap_or(0) == id)
        .ok_or_else(|| anyhow!("no display with id {id}"))
}

/// Save a full capture of the monitor with `id` (from `displays`) to `path`.
pub fn display_screenshot_to_file(id: u32, path: &str) -> Result<String> {
    monitor_by_id(id)?.capture_image()?.save(path)?;
    Ok(path.to_string())
}

/// Capture the monitor with `id` as a `{width, height, rgba}` payload.
pub fn display_screenshot_raw(id: u32) -> Result<ScreenshotRaw> {
    let img = monitor_by_id(id)?.capture_image()?;
    Ok(ScreenshotRaw {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// Enumerate every connected monitor with its virtual-desktop position, size,
/// HiDPI scale, and which one is primary. `GUI::screen_size` only reports the
/// primary display; this covers multi-monitor layouts.
pub fn displays() -> Result<Vec<Display>> {
    let mons = xcap::Monitor::all().map_err(|e| anyhow!("display enumeration failed: {e}"))?;
    let mut out = Vec::with_capacity(mons.len());
    for m in mons {
        out.push(Display {
            id: m.id().unwrap_or(0),
            name: m.name().unwrap_or_default(),
            x: m.x().unwrap_or(0),
            y: m.y().unwrap_or(0),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            scale: m.scale_factor().unwrap_or(1.0),
            primary: m.is_primary().unwrap_or(false),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_serializes_with_r_g_b_keys() {
        let c = Rgb { r: 1, g: 2, b: 3 };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(v["r"], 1);
        assert_eq!(v["g"], 2);
        assert_eq!(v["b"], 3);
        assert_eq!(v.as_object().unwrap().len(), 3);
    }

    #[test]
    fn screenshot_raw_serializes_with_expected_keys() {
        let s = ScreenshotRaw {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255],
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(v["width"], 2);
        assert_eq!(v["height"], 1);
        assert_eq!(v["rgba"].as_array().unwrap().len(), 8);
        assert_eq!(v["rgba"][3], 255);
    }
}
