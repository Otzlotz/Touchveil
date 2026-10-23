# Touchveil

A touchscreen overlay for Hyprland. Swipe two fingers in from the edge of the screen and a
panel with your own buttons, toggles, sliders and displays slides in. While it is open,
touches only reach the panel. When it is closed, the touchscreen works as usual.

## Install

You need Hyprland, Rust 1.92 or newer and a few libraries:

```sh
# Arch
sudo pacman -S --needed rust libxkbcommon fontconfig wayland mesa
# Debian/Ubuntu
sudo apt install cargo libxkbcommon-dev libfontconfig-dev libwayland-dev libegl-dev pkg-config
# Fedora
sudo dnf install cargo libxkbcommon-devel fontconfig-devel wayland-devel mesa-libEGL-devel
```

Then run:

```sh
./install.sh
```

It builds Touchveil, installs it to `~/.local/bin`, creates your config, sets up access
to the touchscreen (asks for `sudo` once) and starts the background service.

Start **Touchveil** from the application menu. It sits in the tray (e.g. Waybar). To start
it with Hyprland, add to `~/.config/hypr/hyprland.conf`:

```ini
exec-once = ~/.local/bin/touchveil-overlay
```

With **more than one monitor**, tell Touchveil which one is the touchscreen in
`~/.config/touchveil/config.toml` (names from `hyprctl monitors`):

```toml
[overlay]
monitor = "DP-3"
```

**Update:** `git pull && ./install.sh` (your config is kept). **Uninstall:**
`./install.sh --uninstall`.

## Use

- **Open:** put two fingers on the right edge and drag them into the screen. A glowing
  strip shows where the edge is. Let go after about halfway, or just flick quickly.
- **Close:** push the panel back to the edge, tap next to it, or tap ✕.
- **Tray icon:** tap it to open the overlay. Its menu has *Open overlay*, *Edit config*
  and *Quit*.

Hard to open? Try these under `[gesture]` in the config, then run
`systemctl --user restart touchveil-daemon`:

```toml
[gesture]
edge = "right"        # or left, top, bottom, top-left, top-right, bottom-left, bottom-right
strip_width = 0.06    # wider edge zone (default 0.04)
fingers = 1           # one finger is enough (default 2)
```

## Customize

Everything is in `~/.config/touchveil/config.toml`. It starts as a copy of
[`config.example.toml`](config.example.toml), which has comments for every option and
ready-made widgets for volume, brightness, microphone, media keys, time and battery.
Changes apply the next time you open the overlay. If the file has a mistake, the overlay
keeps the old settings and shows the error.

Widgets sit in a grid (default 4 columns × 6 rows). `col`/`row` count from 1, `w`/`h`
say how many cells a widget covers. Commands run through `sh -c`, so pipes and `&&` work:

```toml
[[widget]]
type = "slider"
label = "Volume"
exec = "wpctl set-volume @DEFAULT_AUDIO_SINK@ {value}%"
value = "wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print $2 * 100}'"
unit = "%"
col = 1
row = 2
w = 4
```

| Type | Needs | What it does |
|------|-------|--------------|
| `button` | `exec` | Runs `exec` when tapped. |
| `toggle` | `on`, `off`, optional `state` | Runs `on` or `off`; `state` reads the current state (`true`/`false` or exit code). |
| `slider` | `exec` with `{value}`, optional `value`, `min`, `max`, `step`, `unit` | Runs `exec` while you drag; `value` reads the current value. |
| `display` | `exec`, optional `interval_ms` | Shows the output of `exec` while the overlay is open. |

Every widget can have a `label` and its own `accent` color. Buttons and toggles can have
`confirm = true` to need a second tap (good for shutdown), and buttons can have
`close = true` to close the overlay afterwards.

Useful settings:

| Setting | Default | Effect |
|---------|---------|--------|
| `[overlay] size` | `0.45` | Panel size as a share of the screen (`1.0` = full screen). |
| `[overlay] auto_close_s` | `0` | Close after this many seconds without touch (`0` = never). |
| `[overlay] close_after_action` | `false` | Every button closes the overlay after a tap. |
| `[overlay] edge_hint` | `true` | The glowing strip at the edge. |
| `[theme] accent`, `panel`, `font_size`, … | | Colors (`#rrggbb`), sizes and spacing. |
| `[theme] animation_ms` | `220` | Animation speed, `0` turns animations off. |

## Troubleshooting

| Problem | Fix |
|---------|-----|
| Tray tooltip says "Service not reachable" | `systemctl --user enable --now touchveil-daemon` |
| No tray icon | Your bar needs a tray (e.g. Waybar's `tray` module). The gesture works without it. |
| "No read access to /dev/input" | Run `./install.sh` again. |
| "Multiple monitors …" | Set `monitor = "..."` under `[overlay]` (see Install). |
| The wrong touch device is used | Set `device = "..."` at the top of the config (name from `libinput list-devices`). |
| Touches land on the wrong monitor | In Hyprland: `input:touchdevice:output = DP-3`. |
| Touches in the panel are rotated | Set `rotate = 90` (or `180`, `270`) at the top of the config. |
| Holding or dragging right at the edge reacts late | Set `arm_window_ms = 90` under `[gesture]`. |

Logs: `journalctl --user -u touchveil-daemon -f` for the service; start
`touchveil-overlay` in a terminal to see the overlay's messages.

How it works, building from source and project details: [DEV.md](DEV.md).
