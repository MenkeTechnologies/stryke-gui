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
