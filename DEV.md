# Touchveil: developer notes

All the knobs, building, and the patched layer-shika. For installing and using it, see [README.md](README.md).

## How it works

- **touchveil-daemon** (systemd user service) grabs the touchscreen exclusively
  (`EVIOCGRAB`) and forwards normal touches through a virtual device (`Touchveil virtual
  touchscreen`). When an overlay is connected, the edge gesture and all touches while the
  overlay is open go to the overlay instead, over a Unix socket
  (`$XDG_RUNTIME_DIR/touchveil.sock`). If the touchscreen is missing or unplugged, the
  service waits and takes it over again when it is back.
- **touchveil-overlay** (app with tray icon) draws the panel with Slint on a Wayland
  layer-shell surface. Its input region is empty: it gets all touches from the daemon, so
  touches and clicks pass through the invisible surface. While the overlay is closed
  nothing runs: no polling, no redrawing. Only one instance runs at a time (lock file next
  to the socket).
- **touchveil-common** holds what both share: edges and rotation, the socket protocol,
  paths and the instance lock.

`install.sh` copies `packaging/70-touchveil.rules` to `/etc/udev/rules.d/`, which gives the
logged-in user access to touchscreens and `/dev/uinput` via `uaccess` (no group, no
re-login), loads the `uinput` module at boot, installs the binaries to `$PREFIX/bin`
(default `~/.local`), the app entry and the `touchveil-daemon` user service.

### The gesture

The daemon sorts every finger into a role:

- A finger that lands in the **edge strip** (`strip_width`, corners are twice as big)
  while an overlay is connected and closed becomes a *candidate*. Its events are buffered.
- When `fingers` candidates are down, the **gesture** starts: their events are swallowed
  and the daemon sends the progress (inward distance / `travel`, 0..1) to the overlay,
  which moves the panel with the fingers.
- On release, the overlay opens if the progress is at least `open_threshold`, or if the
  fingers were **flicked** inward (`fling`: speed over the last 100 ms of at least 1.5
  progress/s, and progress of at least 0.1). A fast flick back to the edge cancels even
  above the threshold.
- A candidate that leaves the strip (with some slack) or is lifted before the gesture
  starts becomes a normal touch; its buffered events are replayed, so apps see the whole
  movement. With `arm_window_ms > 0`, a candidate is also released after that time
  (default `0` = no time limit). The trade-off: without a time limit, holding or dragging
  right at the edge only reaches apps once the finger leaves the edge or is lifted.
- While the overlay is open, all new touches go to it; the overlay does its own hit
  testing.
- Without a connected overlay, the daemon forwards everything, edge swipes included.

### Socket protocol

Line-based text, see `crates/common/src/protocol.rs`.

| Daemon → overlay | Meaning |
|------------------|---------|
| `progress <0..1>` | Gesture progress (`0` also means the gesture was cancelled). |
| `open` / `close` | Open or close the overlay. |
| `down <slot> <x> <y>`, `move …`, `up <slot>` | Touches while open, normalized 0..1 and already rotated. |
| `hint <x> <y>` / `hint off` | A finger is in the edge strip (edge hint). |

| Overlay → daemon | Meaning |
|------------------|---------|
| `open` | Open without the gesture (tray). |
| `close` | Close and give the touchscreen back. |

If the overlay does not read for 200 ms, the daemon drops the connection and frees the
touchscreen.

### Commands and timeouts

Widget commands run via `sh -c` in their own process group, so a timeout kills the whole
tree. Defaults: `state`, `value` and slider commands 2 s, display and toggle commands 5 s;
`timeout_ms` (100 to 600000) changes this per widget. A display that times out shows "–".
Buttons have no timeout and may start long-running programs. Read commands (`state`,
`value`, displays) also have anything they leave in the background killed after reading;
toggle and slider commands may start background programs.

Each slider has its own worker: while a command runs, newer values replace the pending
one, so dragging never piles up processes and the last value always runs. A toggle
ignores taps while its command is still running.

### Animations and performance

The panel position is driven by one animated Slint property: while a finger drags, it
follows without delay; on release, open, close and snap-back it glides with
`ease-out-quart` over `animation_ms`. The content fades in during the second half.

Rendering is paced by the compositor's frame callbacks (input at 120 Hz still renders
~60 frames/s). When idle the overlay uses no CPU and has no wakeups. Measured with
software rendering (llvmpipe) in a headless compositor: the router needs ~0.1 µs per
touch frame when passing through and ~2 µs while the overlay is open; the overlay's own
code uses 11–17 % CPU while dragging (the rest is software GL), and there is no memory
growth over repeated open/close cycles.

## All options

`config.example.toml` lists every option with a comment. The ones the README skips:

| Option | Default | Effect |
|--------|---------|--------|
| `device` | first touchscreen | Name filter (`*` wildcards) when there are several touch devices. |
| `socket` | `$XDG_RUNTIME_DIR/touchveil.sock` | Socket path (read at start). |
| `rotate` | `0` | Rotation of the touchscreen against the image: 0, 90, 180, 270. |
| `[gesture] strip_width` | `0.04` | Edge strip width as a share of the screen (0.01 to 0.25). |
| `[gesture] fingers` | `2` | Fingers needed. |
| `[gesture] arm_window_ms` | `0` | Time a candidate waits for the other fingers, `0` = no limit. |
| `[gesture] travel` | `0.35` | Distance (share of the screen) for 100 % progress. |
| `[gesture] open_threshold` | `0.5` | Progress at which releasing opens. |
| `[gesture] fling` | `true` | Fast flick opens below the threshold, fast flick back cancels. |
| `[actions] on_open`, `on_close` | | Commands (argument list) run on open/close. |
| `[overlay] title`, `close_button` | `"Touchveil"`, `true` | Header of the panel. |
| `[overlay] tap_outside_closes` | `true` | Tap next to the panel closes it. |
| `[overlay] swipe_close` | `0.25` | How far (share of the panel) it must be pushed back to close. |
| `[overlay] show_touches` | `false` | Draw touch points (for testing). |
| `[theme] dim`, `border`, `border_width`, `radius`, `gap`, `padding`, `muted`, `widget`, `pressed`, `text` | | Remaining look settings. |
| widget `timeout_ms` | see above | Command timeout per widget. |
| slider `min`, `max`, `step` | `0`, `100`, `1` | Value range. |
| display `interval_ms` | `1000` | Refresh interval while open. |

Changes to the daemon part (`device`, `rotate`, `[gesture]`, `[actions]`) need
`systemctl --user restart touchveil-daemon`; the overlay reloads its part on the next open.

## Project structure

```
Touchveil/
├── Cargo.toml, Cargo.lock    Cargo workspace
├── config.example.toml       commented example config (install.sh copies it)
├── install.sh                build, install, permissions, service (--uninstall)
├── crates/
│   ├── common/src/           shared by daemon and overlay
│   │   ├── edge.rs             edges, corners, rotation
│   │   ├── protocol.rs         messages over the socket
│   │   └── paths.rs            socket and config paths, instance lock
│   ├── daemon/src/
│   │   ├── main.rs             startup: arguments, config, lock, socket
│   │   ├── server.rs           main loop: wait for the touchscreen, serve it
│   │   ├── router.rs           routes each finger to programs, gesture or overlay
│   │   ├── router/tests.rs     its tests
│   │   ├── device.rs           find the touchscreen, virtual device
│   │   ├── config.rs           [gesture], [actions], device, rotate
│   │   └── filter.rs           name filter for device
│   └── overlay/
│       ├── ui/overlay.slint    rendering (Slint)
│       └── src/
│           ├── main.rs         startup: monitor, layer shell surface, event loop
│           ├── app/            overlay state
│           │   ├── mod.rs        messages and state
│           │   ├── view.rs       pass theme, layout, widgets to Slint
│           │   ├── touch.rs      hit testing, tap, drag, push back
│           │   └── widgets.rs    commands of buttons, toggles, sliders, displays
│           ├── config.rs       [overlay], [theme], [grid], [[widget]], live reload
│           ├── layout.rs       geometry: panel, grid, hit areas
│           ├── exec.rs         commands with timeout and process groups
│           ├── link.rs         connection to the daemon
│           ├── tray.rs         tray icon
│           ├── ticker.rs       clock for displays and timeouts while open
│           └── outputs.rs      monitor selection
├── packaging/                udev rule, systemd service, app entry
└── vendor/
    ├── layer-shika/            patched copy (see below)
    └── layer-shika-patches/    the changes as one patch
```

## Building and testing

```sh
cargo build --release
cargo test
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
shellcheck install.sh
# run locally with the example config:
cargo run -p touchveil-daemon -- --config config.example.toml
cargo run -p touchveil-overlay -- --config config.example.toml
```

The same checks run in GitHub Actions on every pull request and every push to `main`
(`.github/workflows/ci.yml`).

The daemon's state machine is tested without hardware: `router/tests.rs` feeds evdev
events into the router with a recording sink and a socket pair as the overlay. The overlay
can be tested without a touchscreen by talking to its socket directly: start a small
script that listens on the socket, run `touchveil-overlay --config … --monitor …`, then
send lines such as `progress 1.0`, `open`, `down 0 0.5 0.5`, `up 0`. This works in a
headless Sway (`WLR_BACKENDS=headless`), also without any input devices (see "Mouse and
keyboard only when present" below).

## Changes to layer-shika

The copy in `vendor/layer-shika/` is based on fork commit `0a68433`. All changes are also
in a single patch, `vendor/layer-shika-patches/0001-touchveil-patches-for-layer-shika.patch`,
which can be applied to the fork with `git am`; after that, `vendor/layer-shika/` can be
replaced by a Git dependency. `Cargo.lock` is not part of the patch; `cargo` updates it.

- **Output names at startup:** the `OutputPolicy` got no connector names at startup, so
  the monitor could not be selected by name. `prefetch_output_metadata()` is now called
  before the surfaces are created.
- **Output size:** because of the prefetch, the mode events arrived before there were any
  surfaces, and the overlay stayed 1×1 pixel. The mode is now remembered and applied later.
- **Rendering without a Wayland event:** if the event loop was woken by another source
  (here the socket), `process_events` bailed out on `WouldBlock` before rendering, and the
  overlay showed the previous state.
- **Animations:** the event loop slept until an event arrived, and Slint animations
  jumped to the end. During an animation it now redraws at the frame rate (~60 Hz).
  Slint's clock is also advanced before every callback; otherwise an animation started
  after an idle phase began in the past and was over immediately.
- **Mouse and keyboard only when present:** `wl_pointer` and `wl_keyboard` were always
  requested. wlroots compositors such as Sway abort this with a protocol error when these
  devices are missing. They are now only created once the seat reports them, and released
  when they go away. Also, every surface held a copy of the pointer that released it for
  everyone when the surface was removed (e.g. monitor unplugged).
- **Without winit:** the Slint feature `backend-winit-wayland` was not needed (layer-shika
  brings its own platform) but pulled in winit and ~55 more crates. In exchange, Slint is
  now at least 1.18.1 and `wayland-backend` with `client_system` is listed directly.
- **Without screen lock and popups:** Touchveil uses neither `ext-session-lock` nor
  `xdg_popup`. Both are removed along with their domain types and API (~6,300 lines). A
  `PopupWindow` (also `ContextMenu`, `ComboBox`) in the `.slint` file therefore no longer
  works.
