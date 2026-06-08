//! Mouse synthesis + position/size queries (enigo-backed).

use std::thread::sleep as thread_sleep;
use std::time::Duration;

use anyhow::Result;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Mouse};
use serde::Serialize;

use crate::common::make_enigo;

#[derive(Serialize)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

#[derive(Serialize)]
pub struct Size {
    pub width: i64,
    pub height: i64,
}

/// Animate the mouse from its current location to `(tx, ty)` over
/// `duration` seconds using linear interpolation at 60 fps. `duration <=
/// 0` is a single-step warp, matching pyautogui's default.
fn linear_move(e: &mut Enigo, tx: i32, ty: i32, duration: f64) -> Result<(), enigo::InputError> {
    if duration <= 0.0 {
        return e.move_mouse(tx, ty, Coordinate::Abs);
    }
    let (sx, sy) = e.location()?;
    let steps = ((duration * 60.0).max(2.0)) as i32;
    let step_dur = Duration::from_secs_f64(duration / steps as f64);
    let dx = (tx - sx) as f64;
    let dy = (ty - sy) as f64;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = sx as f64 + dx * t;
        let y = sy as f64 + dy * t;
        e.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
        thread_sleep(step_dur);
    }
    Ok(())
}

fn maybe_move_to(e: &mut Enigo, x: Option<i32>, y: Option<i32>) -> Result<(), enigo::InputError> {
    if let (Some(x), Some(y)) = (x, y) {
        e.move_mouse(x, y, Coordinate::Abs)?;
    }
    Ok(())
}

pub fn pos() -> Result<Point> {
    let e = make_enigo()?;
    let (x, y) = e.location()?;
    Ok(Point {
        x: x as i64,
        y: y as i64,
    })
}

pub fn screen_size() -> Result<Size> {
    let e = make_enigo()?;
    let (w, h) = e.main_display()?;
    Ok(Size {
        width: w as i64,
        height: h as i64,
    })
}

pub fn on_screen(x: i32, y: i32) -> Result<bool> {
    let e = make_enigo()?;
    let (w, h) = e.main_display()?;
    Ok(x >= 0 && y >= 0 && x < w && y < h)
}

pub fn mv(x: i32, y: i32, duration: f64) -> Result<()> {
    let mut e = make_enigo()?;
    linear_move(&mut e, x, y, duration)?;
    Ok(())
}

pub fn mv_rel(dx: i32, dy: i32, duration: f64) -> Result<()> {
    let mut e = make_enigo()?;
    if duration <= 0.0 {
        e.move_mouse(dx, dy, Coordinate::Rel)?;
    } else {
        let (sx, sy) = e.location()?;
        linear_move(&mut e, sx + dx, sy + dy, duration)?;
    }
    Ok(())
}

pub fn drag(x: i32, y: i32, duration: f64, button: Button, relative: bool) -> Result<()> {
    let mut e = make_enigo()?;
    let (sx, sy) = e.location()?;
    let (tx, ty) = if relative { (sx + x, sy + y) } else { (x, y) };
    e.button(button, Direction::Press)?;
    let move_res = linear_move(&mut e, tx, ty, duration);
    // Always release even if the move errored — otherwise a stuck
    // mouse-button state survives the failure and the user has to manually
    // click to recover.
    let release_res = e.button(button, Direction::Release);
    move_res?;
    release_res?;
    Ok(())
}

pub fn click(
    x: Option<i32>,
    y: Option<i32>,
    clicks: i64,
    interval: f64,
    button: Button,
) -> Result<()> {
    let clicks = clicks.max(1);
    let interval = interval.max(0.0);
    let mut e = make_enigo()?;
    maybe_move_to(&mut e, x, y)?;
    for i in 0..clicks {
        e.button(button, Direction::Click)?;
        if interval > 0.0 && i + 1 < clicks {
            thread_sleep(Duration::from_secs_f64(interval));
        }
    }
    Ok(())
}

pub fn button_hold(button: Button, press: bool) -> Result<()> {
    let mut e = make_enigo()?;
    let dir = if press {
        Direction::Press
    } else {
        Direction::Release
    };
    e.button(button, dir)?;
    Ok(())
}

pub fn scroll(clicks: i32, x: Option<i32>, y: Option<i32>, horizontal: bool) -> Result<()> {
    let axis = if horizontal {
        Axis::Horizontal
    } else {
        Axis::Vertical
    };
    let mut e = make_enigo()?;
    maybe_move_to(&mut e, x, y)?;
    e.scroll(clicks, axis)?;
    Ok(())
}
