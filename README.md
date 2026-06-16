```
 ███████╗████████╗██████╗ ██╗   ██╗██╗  ██╗███████╗
 ██╔════╝╚══██╔══╝██╔══██╗╚██╗ ██╔╝██║ ██╔╝██╔════╝
 ███████╗   ██║   ██████╔╝ ╚████╔╝ █████╔╝ █████╗
 ╚════██║   ██║   ██╔══██╗  ╚██╔╝  ██╔═██╗ ██╔══╝
 ███████║   ██║   ██║  ██║   ██║   ██║  ██╗███████╗
 ╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝
                   [ g u i ]
```

[![CI](https://github.com/MenkeTechnologies/stryke-gui/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/stryke-gui/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![stryke](https://img.shields.io/badge/stryke-package-cyan.svg)](https://github.com/MenkeTechnologies/strykelang)

### `[GUI AUTOMATION FOR STRYKE // MOUSE + KEYBOARD + SCREEN + PIXEL + CLIPBOARD]`

> *"PyAutoGUI, one stryke pipe away."*

GUI automation for stryke — mouse motion/buttons/wheel, keyboard
press/type/hotkey, screen size + bounds, pixel reads, and screenshots.
Shipped as a precompiled cdylib that stryke dlopens in-process on first
`use GUI`. No subprocess fork per call; the `Enigo` input handle persists
across calls inside the cdylib for the life of the stryke process.

### [`strykelang`](https://github.com/MenkeTechnologies/strykelang) &middot; [`stryke-aws`](https://github.com/MenkeTechnologies/stryke-aws)

---

## Table of Contents

- [\[0x00\] How this loads](#0x00-how-this-loads)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Quick start](#0x02-quick-start)
- [\[0x03\] API reference](#0x03-api-reference)
- [\[0x04\] Permissions](#0x04-permissions)
- [\[0x05\] Examples](#0x05-examples)
- [\[0x06\] Tests](#0x06-tests)
- [\[0x07\] Build from source](#0x07-build-from-source)
- [\[0x08\] Layout](#0x08-layout)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] How this loads

`stryke-gui` is a cdylib package: each `extern "C" fn gui__*` in `src/lib.rs`
is a JSON-string-in / JSON-string-out wrapper around the `mouse` /
`keyboard` / `capture` modules. On first `use GUI`:

1. stryke's package resolver finds the installed package in
   `~/.stryke/store/stryke-gui@<version>/`.
2. The package's `[ffi]` section names the exports.
3. stryke `dlopen`s `lib/libstryke_gui.{dylib,so}` next to `lib/GUI.stk`.
4. Every export gets registered in stryke's FFI registry with signature
   `*const c_char -> *const c_char`.
5. The `lib/GUI.stk` wrapper just JSON-encodes args, calls the FFI symbol,
   and parses the JSON return.

Every `GUI::*` call is now a direct function call into the cdylib — no
`fork(2)`, no `exec(2)`, no JSON-over-pipe round-trip, no `Enigo::new()`
per invocation. The Enigo input handle is held in a process-global
`OnceCell<Mutex<Enigo>>` (`src/common.rs::enigo_lock`) and reused across
calls.

The previous shape (v0.1.x) shipped a `stryke-gui-helper` binary spawned
once per `GUI::*` call; that path is gone and the helper sources have been
recast as cdylib exports.

## [0x01] Install

Stryke must be installed first (see [strykelang](https://github.com/MenkeTechnologies/strykelang)).
Then, on macOS or Linux:

```sh
s pkg install -g github.com/MenkeTechnologies/stryke-gui
```

This fetches the prebuilt release tarball for your host triple
(`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`), verifies its SHA-256, extracts into
`~/.stryke/store/stryke-gui@<version>/`, and registers the cdylib for `use GUI`.
No `cargo`, no `rustc`, no per-target build step on the user's machine.

Pin a specific release:

```sh
s pkg install -g github.com/MenkeTechnologies/stryke-gui@v0.3.1
```

Override the auto-detected host triple (e.g. for musl) via
`STRYKE_TARGET=x86_64-unknown-linux-musl s pkg install -g github.com/...`.

## [0x02] Quick start

```perl
use GUI

# where is the cursor, how big is the screen?
my ($x, $y) = GUI::mouse_pos()
my ($w, $h) = GUI::screen_size()
p "cursor ($x, $y) on ${w}x${h}"

# move + click + type
GUI::mouse_move(int($w / 2), int($h / 2), 0.3)   # animated over 0.3s
GUI::mouse_click()
GUI::key_type("hello from stryke", 0.05)
GUI::key_hotkey("cmd", "s")                       # ⌘S  (ctrl on Linux/Win)

# read a pixel, grab a screenshot
my ($r, $g, $b) = GUI::pixel(100, 100)
my $path = GUI::screenshot("/tmp/shot.png")
```

## [0x03] API reference

All functions live in the `GUI::` namespace (`use GUI`). Coordinates use a
top-left origin; the primary display only.

### Position / size

| Function | Returns |
|----------|---------|
| `GUI::mouse_pos()` | `($x, $y)` |
| `GUI::screen_size()` / `GUI::mouse_size()` | `($w, $h)` |
| `GUI::on_screen($x, $y)` | `1` / `0` |

### Mouse motion

| Function | Notes |
|----------|-------|
| `GUI::mouse_move($x, $y, $duration=0)` | absolute; `$duration>0` tweens at 60fps |
| `GUI::mouse_move_rel($dx, $dy, $duration=0)` | relative |
| `GUI::mouse_drag($x, $y, $duration=0, $button="left")` | press-drag-release |
| `GUI::mouse_drag_rel($dx, $dy, $duration=0, $button="left")` | relative drag |

### Mouse buttons

| Function | Notes |
|----------|-------|
| `GUI::mouse_click(%opts)` | `x`, `y`, `clicks=1`, `interval=0`, `button="left"` |
| `GUI::mouse_right_click($x?, $y?)` | |
| `GUI::mouse_middle_click($x?, $y?)` | |
| `GUI::mouse_double_click($x?, $y?, $button="left")` | |
| `GUI::mouse_triple_click($x?, $y?, $button="left")` | |
| `GUI::mouse_down($button="left")` / `GUI::mouse_up($button="left")` | hold / release |

### Mouse wheel

| Function | Notes |
|----------|-------|
| `GUI::mouse_scroll($clicks, $x?, $y?)` | vertical (positive = up) |
| `GUI::mouse_hscroll($clicks, $x?, $y?)` | horizontal |

### Keyboard

| Function | Notes |
|----------|-------|
| `GUI::key_press($name, $presses=1, $interval=0)` | discrete press |
| `GUI::key_down($name)` / `GUI::key_up($name)` | hold / release |
| `GUI::key_type($text, $interval=0)` | layout-correct literal text |
| `GUI::key_hotkey(@keys)` | chord: press in order, release in reverse |
| `GUI::keyboard_keys()` | list of every recognized key name |

### Pixel / screenshot

| Function | Returns |
|----------|---------|
| `GUI::pixel($x, $y)` | `($r, $g, $b)` |
| `GUI::pixel_matches_color($x, $y, [$r,$g,$b], $tol=0)` | `1` / `0` |
| `GUI::screenshot($path?)` | `$path`, or `($w, $h, \@rgba)` |
| `GUI::screenshot_region($l, $t, $w, $h, $path?)` | `$path`, or `($w, $h, \@rgba)` |

### Clipboard

| Function | Returns |
|----------|---------|
| `GUI::clipboard_get()` | clipboard text (`""` if none) |
| `GUI::clipboard_set($text)` | `1` |

### Pure helpers (no display / input)

These touch no device — string/color parsing that runs headless:

| Function | Returns |
|----------|---------|
| `GUI::parse_hotkey("ctrl+shift+a")` | `{ keys, modifiers, key }` — last segment is the key, rest are modifiers |
| `GUI::parse_color("#ff8800")` | `{ r, g, b, hex }` — accepts `#rgb`, `#rrggbb`, `rgb(r,g,b)` |
| `GUI::color_distance($a, $b)` | `{ manhattan, euclidean }` — for pixel-match tolerance; each color `[r,g,b]` or `{r,g,b}` |
| `GUI::contrast_ratio($a, $b)` | `{ ratio, aa_normal, aa_large, aaa_normal, aaa_large }` — WCAG 2.1 contrast (1–21) + threshold flags for accessibility |
| `GUI::to_hsl($color)` | `{ h, s, l }` — RGB → HSL (CSS spec); h in degrees 0-360, s/l in percent; for deriving shades by nudging lightness |
| `GUI::to_hsv($color)` | `{ h, s, v }` — RGB → HSV/HSB (the colour-picker model); v is the brightest channel (vs HSL's midpoint l) |
| `GUI::from_hsl($h, $s, $l)` | `{ r, g, b, hex }` — HSL → RGB (CSS spec); inverse of `to_hsl`, h wraps, s/l clamp |
| `GUI::from_hsv($h, $s, $v)` | `{ r, g, b, hex }` — HSV/HSB → RGB; inverse of `to_hsv`, h wraps, s/v clamp |

### Displays

| Function | Returns |
|----------|---------|
| `GUI::displays()` | one hash per monitor: `{ id, name, x, y, width, height, scale, primary }` |
| `GUI::display_screenshot($id, $path?)` | `$path`, or `($w, $h, \@rgba)` — capture a specific monitor by id |

## [0x04] Permissions

- **macOS** — the first mouse/keyboard call prompts for **Accessibility**
  access; the first pixel/screenshot call prompts for **Screen Recording**
  (System Settings → Privacy & Security). Both are one-time grants for the
  terminal app running `s`.
- **Wayland (Linux)** — requires the `wlroots-virtual-pointer` protocol
  (mouse/keyboard) and the `org.freedesktop.portal.Screenshot` portal
  (pixel/screen).
- **X11 (Linux)** and **Windows** — no permission gates.

## [0x05] Examples

```sh
s examples/gui_screen_info.stk      # read-only display + pointer introspection
s examples/gui_mouse_circle.stk     # animate the cursor in a circle
s examples/gui_typewriter.stk       # typewriter effect via key_type
s examples/gui_hotkey.stk           # chord key combos
s examples/gui_drag.stk             # press-drag-release
s examples/gui_scroll_demo.stk      # wheel events
s examples/gui_pixel_probe.stk      # sample pixel colors
s examples/gui_screenshot.stk       # full + region capture
s examples/activity_maintainer.stk  # keep a session active
```

## [0x06] Tests

```sh
make test            # cargo test + `s test t/`
```

`cargo test` covers the FFI plumbing (JSON-in/out wrapper, error-on-panic
behavior, free-cstring contract, region parsing). `t/test_gui.stk` covers
the end-to-end stryke → FFI → cdylib call path via the permission-free
`GUI::keyboard_keys()` query. Mouse/keyboard/pixel ops can't run unattended
in CI (they need real OS permissions) so they aren't asserted there.

## [0x07] Build from source

Consumers don't need this — the install path fetches a prebuilt
artifact for the host triple from GitHub Releases. Contributors building
the cdylib locally:

Linux build deps:

```sh
sudo apt-get install -y libwayland-dev libxkbcommon-dev libxcb1-dev \
  libxrandr-dev libxi-dev libxcursor-dev libdbus-1-dev pkg-config \
  libpipewire-0.3-dev libspa-0.2-dev libegl-dev libgl1-mesa-dev \
  libgles-dev libgbm-dev libxcb-randr0-dev libxinerama-dev libudev-dev
```

Then:

```sh
cd ~/RustroverProjects/stryke-gui
cargo build --release            # → target/release/libstryke_gui.{dylib,so}
```

Stryke's FFI loader looks for the cdylib in `lib/`, then `target/release/`,
then `target/debug/` (see `try_load_ffi_for` in
`strykelang/strykelang/pkg/commands.rs`). So once `cargo build` produces
the dev artifact, `s examples/gui_mouse_circle.stk` works against the
local checkout without a separate install step.

To install the local build into the global store as a drop-in for a
released version (overwriting whatever `s pkg install -g
github.com/...` previously placed there):

```sh
s pkg install -g .
```

## [0x08] Layout

```
stryke.toml            stryke package manifest with [ffi] table
Cargo.toml             stryke_gui cdylib crate manifest
src/
  lib.rs               #[no_mangle] extern "C" gui__* exports + ffi_call wrapper
  common.rs            persistent Enigo via OnceCell + button parser
  mouse.rs             motion / buttons / wheel / size / pos
  keyboard.rs          key-name table + press / type / hotkey
  capture.rs           pixel + screenshot (xcap)
lib/GUI.stk            stryke wrappers (JSON args → FFI symbol → JSON return)
examples/              runnable demos
t/test_gui.stk         plumbing tests (permission-free FFI surface)
bin/gui-test.stk       installable smoke-test launcher (~/.stryke/bin/gui-test)
.github/workflows/
  ci.yml               cargo check/clippy/test/doc per push
  release.yml          per-triple cdylib build matrix → GitHub Release
```

## [0xFF] License

MIT © MenkeTechnologies
