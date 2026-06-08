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

### `[GUI AUTOMATION FOR STRYKE // MOUSE + KEYBOARD + SCREEN + PIXEL]`

> *"PyAutoGUI, one stryke pipe away."*

GUI automation for stryke — mouse motion/buttons/wheel, keyboard
press/type/hotkey, screen size + bounds, pixel reads, and screenshots.
A PyAutoGUI-equivalent surface, opt-in as a package so the stryke core
binary stays slim and free of X11/Wayland system libraries.

### [`strykelang`](https://github.com/MenkeTechnologies/strykelang) &middot; [`stryke-aws`](https://github.com/MenkeTechnologies/stryke-aws)

---

## Table of Contents

- [\[0x00\] Why this is a package, not a builtin](#0x00-why-this-is-a-package-not-a-builtin)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Quick start](#0x02-quick-start)
- [\[0x03\] CLI: `gui`](#0x03-cli-gui)
- [\[0x04\] API reference](#0x04-api-reference)
- [\[0x05\] Helper protocol](#0x05-helper-protocol)
- [\[0x06\] Permissions](#0x06-permissions)
- [\[0x07\] Examples](#0x07-examples)
- [\[0x08\] Tests](#0x08-tests)
- [\[0x09\] Layout](#0x09-layout)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] Why this is a package, not a builtin

The `enigo` (input) and `xcap` (capture) crates link against the platform
input/display stack. On Linux that means the X11 **and** Wayland client
libraries plus xkb, dbus, and PipeWire — a stack of `-dev` packages that
would otherwise have to be installed before `cargo install strykelang`
could even build. Baking them into core forces that cost on every user,
GUI or not.

`stryke-gui` follows the `stryke-aws` shape: a thin stryke library spawns
a small Rust helper binary (`stryke-gui-helper`) per call and parses JSON
over the pipe. The native deps live only here, opt-in, so the stryke core
install needs zero external system libraries.

## [0x01] Install

Linux only — install the system libraries enigo/xcap link against first
(macOS and Windows need nothing extra):

```sh
sudo apt-get install -y libwayland-dev libxkbcommon-dev libxcb1-dev \
  libxrandr-dev libxi-dev libxcursor-dev libdbus-1-dev pkg-config
```

Then build the helper and register the CLIs:

```sh
cd ~/RustroverProjects/stryke-gui
cargo build --release            # produces target/release/stryke-gui-helper
s pkg install -g .               # installs `gui` and `gui-build` CLIs
```

Or:

```sh
make install
```

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

## [0x03] CLI: `gui`

`gui` is a thin pass-through to the helper binary, plus a stryke-side
`gui build`:

```sh
gui mouse pos                       # {"x":...,"y":...}
gui screen size                     # {"width":...,"height":...}
gui mouse move 400 300 --duration 0.5
gui key type "hello" --interval 0.05
gui key hotkey ctrl t
gui pixel 100 100                   # {"r":..,"g":..,"b":..}
gui screenshot --output /tmp/s.png
gui build                           # cargo build --release the helper
gui help                            # helper --help
```

## [0x04] API reference

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

## [0x05] Helper protocol

The `.stk` library shells out to `stryke-gui-helper` and parses its
output:

- **Action** subcommands (move, click, type, …) exit `0` on success.
- **Query** subcommands print a single JSON line to stdout:
  - `mouse pos` → `{"x","y"}`
  - `screen size` → `{"width","height"}`
  - `screen on-screen X Y` → `{"on_screen":bool}`
  - `pixel X Y` → `{"r","g","b"}`
  - `pixel-match …` → `{"match":bool}`
  - `screenshot` (no `--output`) → `{"width","height","rgba":[…]}`
  - `key keys` → JSON array of names

Set `STRYKE_GUI_DEBUG=1` to log each helper command to stderr. Set
`STRYKE_GUI_HELPER=/path/to/stryke-gui-helper` to override binary
discovery.

## [0x06] Permissions

- **macOS** — the first mouse/keyboard call prompts for **Accessibility**
  access; the first pixel/screenshot call prompts for **Screen Recording**
  (System Settings → Privacy & Security). Both are one-time grants for the
  terminal app running `s`.
- **Wayland (Linux)** — requires the `wlroots-virtual-pointer` protocol
  (mouse/keyboard) and the `org.freedesktop.portal.Screenshot` portal
  (pixel/screen).
- **X11 (Linux)** and **Windows** — no permission gates.

## [0x07] Examples

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

## [0x08] Tests

```sh
make test            # cargo test + `s test t/`
```

`t/test_gui.stk` covers the argv build + JSON parse plumbing (`key keys`,
`--version`) — the parts that run without input/screen permissions. The
mouse/keyboard/pixel ops can't run unattended, so they aren't asserted in
CI.

## [0x09] Layout

```
stryke.toml            stryke package manifest (name = `gui`)
Cargo.toml             stryke-gui-helper binary manifest
src/
  main.rs              clap CLI + dispatch
  common.rs            enigo init, button parse, JSON output
  mouse.rs             motion / buttons / wheel / size / pos
  keyboard.rs          key-name table + press / type / hotkey
  capture.rs           pixel + screenshot (xcap)
lib/GUI.stk            stryke wrappers (shell out + parse JSON)
bin/gui.stk            `gui` CLI launcher
bin/gui-build.stk      `gui-build` helper-build launcher
examples/              runnable demos
t/test_gui.stk         plumbing tests
```

## [0xFF] License

MIT © MenkeTechnologies
