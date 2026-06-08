//! `stryke-gui-helper` — bridge binary for the stryke `gui` package.
//!
//! GUI automation (mouse, keyboard, screen, pixel, screenshot) backed by
//! the `enigo` (input) and `xcap` (capture) crates. Isolating these
//! native-dep features in this helper keeps the stryke core binary slim
//! and free of the X11/Wayland system libraries enigo/xcap link against.
//!
//! Action subcommands exit 0 on success. Query subcommands (mouse pos,
//! screen size, pixel, screenshot --raw, key keys) print a single JSON
//! line to stdout; the `.stk` wrappers parse it with `from_json`.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod capture;
mod common;
mod keyboard;
mod mouse;

use crate::common::{emit_json, parse_button};

#[derive(Parser, Debug)]
#[command(
    name = "stryke-gui-helper",
    version,
    about = "GUI automation (mouse, keyboard, screen, pixel, screenshot) for the stryke `gui` package"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    /// Mouse motion, buttons, and wheel.
    #[command(subcommand)]
    Mouse(MouseCmd),

    /// Keyboard press / type / hotkey.
    #[command(subcommand)]
    Key(KeyCmd),

    /// Screen size + bounds queries.
    #[command(subcommand)]
    Screen(ScreenCmd),

    /// Read the RGB color at a screen coordinate. Prints `{"r","g","b"}`.
    Pixel { x: u32, y: u32 },

    /// Test whether a pixel matches an `r g b` color within a tolerance.
    /// Prints `{"match": bool}`.
    PixelMatch {
        x: u32,
        y: u32,
        r: u8,
        g: u8,
        b: u8,
        #[arg(long, default_value_t = 0)]
        tolerance: i32,
    },

    /// Capture the primary display (or a region with --region L T W H).
    /// With --output PATH writes a PNG and prints the path; otherwise
    /// prints `{"width","height","rgba":[…]}` JSON.
    Screenshot {
        #[arg(long, value_names = ["L", "T", "W", "H"], num_args = 4)]
        region: Option<Vec<i64>>,
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum MouseCmd {
    /// Current cursor position. Prints `{"x","y"}`.
    Pos,
    /// Absolute move to X Y, optional --duration seconds (tween).
    Move {
        x: i32,
        y: i32,
        #[arg(long, default_value_t = 0.0)]
        duration: f64,
    },
    /// Relative move by DX DY, optional --duration seconds (tween).
    MoveRel {
        dx: i32,
        dy: i32,
        #[arg(long, default_value_t = 0.0)]
        duration: f64,
    },
    /// Press-drag to X Y, optional --duration and --button.
    Drag {
        x: i32,
        y: i32,
        #[arg(long, default_value_t = 0.0)]
        duration: f64,
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Press-drag by DX DY, optional --duration and --button.
    DragRel {
        dx: i32,
        dy: i32,
        #[arg(long, default_value_t = 0.0)]
        duration: f64,
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Click; optional --x/--y move first, --clicks, --interval, --button.
    Click {
        #[arg(long)]
        x: Option<i32>,
        #[arg(long)]
        y: Option<i32>,
        #[arg(long, default_value_t = 1)]
        clicks: i64,
        #[arg(long, default_value_t = 0.0)]
        interval: f64,
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Press a mouse button without releasing (--button).
    Down {
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Release a mouse button (--button).
    Up {
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Vertical wheel scroll by CLICKS (positive = up); optional --x/--y.
    Scroll {
        clicks: i32,
        #[arg(long)]
        x: Option<i32>,
        #[arg(long)]
        y: Option<i32>,
    },
    /// Horizontal wheel scroll by CLICKS; optional --x/--y.
    Hscroll {
        clicks: i32,
        #[arg(long)]
        x: Option<i32>,
        #[arg(long)]
        y: Option<i32>,
    },
}

#[derive(Subcommand, Debug)]
enum KeyCmd {
    /// Press a named key, optional --presses and --interval.
    Press {
        name: String,
        #[arg(long, default_value_t = 1)]
        presses: i64,
        #[arg(long, default_value_t = 0.0)]
        interval: f64,
    },
    /// Hold a key down (no release).
    Down { name: String },
    /// Release a held key.
    Up { name: String },
    /// Type a literal UTF-8 string, optional --interval between chars.
    Type {
        text: String,
        #[arg(long, default_value_t = 0.0)]
        interval: f64,
    },
    /// Chord: press NAMES in order, release in reverse. Optional --interval.
    Hotkey {
        names: Vec<String>,
        #[arg(long, default_value_t = 0.0)]
        interval: f64,
    },
    /// List every recognized key name. Prints a JSON array.
    Keys,
}

#[derive(Subcommand, Debug)]
enum ScreenCmd {
    /// Primary display dimensions. Prints `{"width","height"}`.
    Size,
    /// Whether X Y is within the primary display. Prints `{"on_screen":bool}`.
    OnScreen { x: i32, y: i32 },
}

fn region_tuple(region: &Option<Vec<i64>>) -> Option<(i32, i32, u32, u32)> {
    region.as_ref().map(|v| {
        (
            v[0] as i32,
            v[1] as i32,
            v[2].max(0) as u32,
            v[3].max(0) as u32,
        )
    })
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("stryke-gui-helper: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Top::Mouse(c) => mouse_dispatch(c),
        Top::Key(c) => key_dispatch(c),
        Top::Screen(c) => screen_dispatch(c),
        Top::Pixel { x, y } => emit_json(&capture::pixel(x, y)?),
        Top::PixelMatch {
            x,
            y,
            r,
            g,
            b,
            tolerance,
        } => {
            let m = capture::pixel_matches(x, y, (r, g, b), tolerance.max(0))?;
            emit_json(&serde_json::json!({ "match": m }))
        }
        Top::Screenshot { region, output } => {
            let reg = region_tuple(&region);
            match output {
                Some(path) => emit_json(&serde_json::json!({
                    "path": capture::screenshot_to_file(reg, &path)?
                })),
                None => emit_json(&capture::screenshot_raw(reg)?),
            }
        }
    }
}

fn mouse_dispatch(c: MouseCmd) -> Result<()> {
    match c {
        MouseCmd::Pos => emit_json(&mouse::pos()?),
        MouseCmd::Move { x, y, duration } => mouse::mv(x, y, duration),
        MouseCmd::MoveRel { dx, dy, duration } => mouse::mv_rel(dx, dy, duration),
        MouseCmd::Drag {
            x,
            y,
            duration,
            button,
        } => mouse::drag(x, y, duration, parse_button(&button), false),
        MouseCmd::DragRel {
            dx,
            dy,
            duration,
            button,
        } => mouse::drag(dx, dy, duration, parse_button(&button), true),
        MouseCmd::Click {
            x,
            y,
            clicks,
            interval,
            button,
        } => mouse::click(x, y, clicks, interval, parse_button(&button)),
        MouseCmd::Down { button } => mouse::button_hold(parse_button(&button), true),
        MouseCmd::Up { button } => mouse::button_hold(parse_button(&button), false),
        MouseCmd::Scroll { clicks, x, y } => mouse::scroll(clicks, x, y, false),
        MouseCmd::Hscroll { clicks, x, y } => mouse::scroll(clicks, x, y, true),
    }
}

fn key_dispatch(c: KeyCmd) -> Result<()> {
    match c {
        KeyCmd::Press {
            name,
            presses,
            interval,
        } => keyboard::press(&name, presses, interval),
        KeyCmd::Down { name } => keyboard::down(&name),
        KeyCmd::Up { name } => keyboard::up(&name),
        KeyCmd::Type { text, interval } => keyboard::type_text(&text, interval),
        KeyCmd::Hotkey { names, interval } => keyboard::hotkey(&names, interval),
        KeyCmd::Keys => emit_json(&keyboard::KEYBOARD_KEY_NAMES),
    }
}

fn screen_dispatch(c: ScreenCmd) -> Result<()> {
    match c {
        ScreenCmd::Size => emit_json(&mouse::screen_size()?),
        ScreenCmd::OnScreen { x, y } => {
            emit_json(&serde_json::json!({ "on_screen": mouse::on_screen(x, y)? }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn region_tuple_none_passes_through() {
        assert!(region_tuple(&None).is_none());
    }

    #[test]
    fn region_tuple_valid_region_unchanged() {
        let r = region_tuple(&Some(vec![10, 20, 100, 200])).unwrap();
        assert_eq!(r, (10, 20, 100, 200));
    }

    #[test]
    fn region_tuple_clamps_negative_size_to_zero() {
        // W/H are u32 — a negative input must clamp, not wrap to a giant
        // value via `as u32`. Catches the classic sign-cast footgun.
        let r = region_tuple(&Some(vec![0, 0, -5, -7])).unwrap();
        assert_eq!(r, (0, 0, 0, 0));
    }

    #[test]
    fn region_tuple_preserves_negative_offsets() {
        // L/T are i32 and *do* allow negatives so callers can clip into
        // off-screen regions; downstream `capture` clamps with `.max(0)`.
        let r = region_tuple(&Some(vec![-3, -4, 50, 60])).unwrap();
        assert_eq!(r, (-3, -4, 50, 60));
    }

    #[test]
    fn cli_definition_is_internally_consistent() {
        // clap panics at build time when a derive is malformed (duplicate
        // long flags, conflicting defaults, etc.). This forces that
        // validation into the test run.
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_parses_mouse_pos() {
        let cli = Cli::try_parse_from(["stryke-gui-helper", "mouse", "pos"]).unwrap();
        assert!(matches!(cli.cmd, Top::Mouse(MouseCmd::Pos)));
    }

    #[test]
    fn cli_parses_mouse_click_defaults() {
        let cli = Cli::try_parse_from(["stryke-gui-helper", "mouse", "click"]).unwrap();
        match cli.cmd {
            Top::Mouse(MouseCmd::Click {
                x,
                y,
                clicks,
                interval,
                button,
            }) => {
                assert!(x.is_none());
                assert!(y.is_none());
                assert_eq!(clicks, 1);
                assert_eq!(interval, 0.0);
                assert_eq!(button, "left");
            }
            other => panic!("expected mouse click, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_mouse_click_with_flags() {
        let cli = Cli::try_parse_from([
            "stryke-gui-helper",
            "mouse",
            "click",
            "--x",
            "100",
            "--y",
            "200",
            "--clicks",
            "3",
            "--button",
            "right",
        ])
        .unwrap();
        match cli.cmd {
            Top::Mouse(MouseCmd::Click {
                x,
                y,
                clicks,
                button,
                ..
            }) => {
                assert_eq!(x, Some(100));
                assert_eq!(y, Some(200));
                assert_eq!(clicks, 3);
                assert_eq!(button, "right");
            }
            other => panic!("expected mouse click, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_key_press_and_hotkey() {
        let cli = Cli::try_parse_from(["stryke-gui-helper", "key", "press", "enter"]).unwrap();
        assert!(matches!(cli.cmd, Top::Key(KeyCmd::Press { .. })));

        let cli = Cli::try_parse_from(["stryke-gui-helper", "key", "hotkey", "ctrl", "shift", "t"])
            .unwrap();
        match cli.cmd {
            Top::Key(KeyCmd::Hotkey { names, .. }) => {
                assert_eq!(names, vec!["ctrl", "shift", "t"]);
            }
            other => panic!("expected key hotkey, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_pixel_and_pixel_match() {
        let cli = Cli::try_parse_from(["stryke-gui-helper", "pixel", "10", "20"]).unwrap();
        assert!(matches!(cli.cmd, Top::Pixel { x: 10, y: 20 }));

        let cli = Cli::try_parse_from([
            "stryke-gui-helper",
            "pixel-match",
            "10",
            "20",
            "255",
            "128",
            "0",
            "--tolerance",
            "5",
        ])
        .unwrap();
        match cli.cmd {
            Top::PixelMatch {
                x,
                y,
                r,
                g,
                b,
                tolerance,
            } => {
                assert_eq!((x, y, r, g, b, tolerance), (10, 20, 255, 128, 0, 5));
            }
            other => panic!("expected pixel-match, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_screenshot_region() {
        // --region needs exactly 4 values per the `num_args = 4` config.
        let cli = Cli::try_parse_from([
            "stryke-gui-helper",
            "screenshot",
            "--region",
            "0",
            "0",
            "100",
            "200",
        ])
        .unwrap();
        match cli.cmd {
            Top::Screenshot { region, output } => {
                assert_eq!(region, Some(vec![0, 0, 100, 200]));
                assert!(output.is_none());
            }
            other => panic!("expected screenshot, got {other:?}"),
        }

        // 3 values must fail.
        assert!(Cli::try_parse_from([
            "stryke-gui-helper",
            "screenshot",
            "--region",
            "0",
            "0",
            "100",
        ])
        .is_err());
    }

    #[test]
    fn cli_rejects_unknown_top_level_subcommand() {
        assert!(Cli::try_parse_from(["stryke-gui-helper", "bogus"]).is_err());
    }
}
