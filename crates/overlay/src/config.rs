//! Overlay part of the shared config.toml: [overlay], [theme], [grid], [[widget]].
//! Unknown sections (e.g. [gesture] of the daemon) are ignored.

use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
pub use touchveil_common::Edge;

/// Color from "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s
            .trim()
            .strip_prefix('#')
            .ok_or_else(|| format!("color \"{s}\": must start with #"))?;
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()
            .ok_or_else(|| format!("color \"{s}\": not a hex number"))?;
        let pair = |i: usize| digits[i] * 16 + digits[i + 1];
        match digits.len() {
            3 | 4 => {
                let a = if digits.len() == 4 {
                    digits[3] * 17
                } else {
                    255
                };
                Ok(Color([digits[0] * 17, digits[1] * 17, digits[2] * 17, a]))
            }
            6 => Ok(Color([pair(0), pair(2), pair(4), 255])),
            8 => Ok(Color([pair(0), pair(2), pair(4), pair(6)])),
            _ => Err(format!(
                "color \"{s}\": expected #rgb, #rrggbb or #rrggbbaa"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Panel {
    /// Monitor (connector name from `hyprctl monitors`). Only needed with several monitors;
    /// read at startup.
    pub monitor: Option<String>,
    /// Heading in the panel ("" hides it, provided close_button = false as well).
    pub title: String,
    /// Size of the panel as a fraction of the screen (1.0 = fullscreen): width when it
    /// comes from the left/right, height for top/bottom.
    #[serde(alias = "width")]
    pub size: f32,
    /// Close button in the header.
    pub close_button: bool,
    /// A tap next to the panel closes the overlay.
    pub tap_outside_closes: bool,
    /// Show touch points (for testing).
    pub show_touches: bool,
    /// Glowing strip at the edge while a finger is in the start zone of the gesture.
    pub edge_hint: bool,
    /// How far (fraction of the panel size) the panel has to be pushed towards the edge
    /// to close (0.05 to 0.9).
    pub swipe_close: f32,
    /// Close after this many seconds without a touch (0 = never).
    pub auto_close_s: u32,
    /// Buttons close the overlay after a tap (can be changed per button with `close`).
    pub close_after_action: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            monitor: None,
            title: "Touchveil".into(),
            size: 0.45,
            close_button: true,
            tap_outside_closes: true,
            show_touches: false,
            edge_hint: true,
            swipe_close: 0.25,
            auto_close_s: 0,
            close_after_action: false,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub panel: Color,
    /// Dimming behind the panel, 0 to 1.
    pub dim: f32,
    pub text: Color,
    /// Secondary color for the labels of displays and sliders.
    pub muted: Color,
    pub widget: Color,
    pub pressed: Color,
    pub accent: Color,
    pub border: Color,
    pub border_width: f32,
    pub radius: f32,
    pub font_size: f32,
    /// Spacing between widgets.
    pub gap: f32,
    /// Inner padding of the panel.
    pub padding: f32,
    /// Duration of opening, closing and snapping back in ms; 0 turns animations off.
    pub animation_ms: u32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            panel: Color([0x17, 0x1a, 0x21, 0xf5]),
            dim: 0.55,
            text: Color([0xf2, 0xf2, 0xf7, 0xff]),
            muted: Color([0x9a, 0xa0, 0xad, 0xff]),
            widget: Color([0x25, 0x2a, 0x35, 0xff]),
            pressed: Color([0x3a, 0x41, 0x50, 0xff]),
            accent: Color([0x4d, 0x9f, 0xff, 0xff]),
            border: Color([0xff, 0xff, 0xff, 0x14]),
            border_width: 1.0,
            radius: 14.0,
            font_size: 22.0,
            gap: 12.0,
            padding: 24.0,
            animation_ms: 220,
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Grid {
    pub columns: u32,
    pub rows: u32,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            columns: 4,
            rows: 6,
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Kind {
    /// A tap runs `exec`.
    Button {
        exec: String,
        /// Close the overlay afterwards; if unset, `overlay.close_after_action` applies.
        #[serde(default)]
        close: Option<bool>,
    },
    /// On/off. `state` prints the state (true/1/on/yes) or reports it via exit code 0.
    Toggle {
        on: String,
        off: String,
        #[serde(default)]
        state: Option<String>,
    },
    /// Dragging sets a value, `exec` gets it as {value}. `value` reads the initial value.
    Slider {
        exec: String,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        min: f64,
        #[serde(default = "default_max")]
        max: f64,
        #[serde(default = "default_step")]
        step: f64,
        #[serde(default)]
        unit: String,
    },
    /// Shows the output of `exec`, only updated while the overlay is open.
    Display {
        exec: String,
        #[serde(default = "default_interval")]
        interval_ms: u64,
    },
}

fn default_max() -> f64 {
    100.0
}
fn default_step() -> f64 {
    1.0
}
fn default_interval() -> u64 {
    1000
}
fn one() -> u32 {
    1
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Widget {
    #[serde(flatten)]
    pub kind: Kind,
    #[serde(default)]
    pub label: String,
    /// Column and row, counted from 1.
    pub col: u32,
    pub row: u32,
    /// Width and height in grid cells.
    #[serde(default = "one")]
    pub w: u32,
    #[serde(default = "one")]
    pub h: u32,
    /// Own accent color instead of theme.accent.
    #[serde(default)]
    pub accent: Option<Color>,
    /// Timeout of this widget's commands in ms (buttons have none).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Only a second tap within [`CONFIRM_WINDOW`] triggers (button, toggle).
    #[serde(default)]
    pub confirm: bool,
}

/// How long a widget with `confirm = true` waits for the second tap.
pub const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

/// What a command runs for; determines the default timeout.
#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    /// `state`, `value`: should answer immediately.
    Read,
    /// Command of a display, may e.g. briefly go to the network.
    Display,
    /// `on`/`off` of a toggle.
    Toggle,
    /// `exec` of a slider.
    Slider,
}

impl Widget {
    /// Timeout for a command: the widget's `timeout_ms` or the default.
    pub fn timeout(&self, purpose: Purpose) -> Duration {
        let default_ms = match purpose {
            Purpose::Read | Purpose::Slider => 2_000,
            Purpose::Display | Purpose::Toggle => 5_000,
        };
        Duration::from_millis(self.timeout_ms.unwrap_or(default_ms))
    }
}

/// From [gesture] the overlay only needs the edge and strip width (for the edge hint);
/// the rest belongs to the daemon.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(default)]
pub struct Gesture {
    pub edge: Edge,
    pub strip_width: f32,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            edge: Edge::default(),
            strip_width: 0.04, // as in the daemon
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub socket: Option<PathBuf>,
    pub gesture: Gesture,
    pub overlay: Panel,
    pub theme: Theme,
    pub grid: Grid,
    pub widget: Vec<Widget>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&mut self) -> Result<(), String> {
        let g = &mut self.grid;
        g.columns = g.columns.clamp(1, 32);
        g.rows = g.rows.clamp(1, 32);
        let p = &mut self.overlay;
        p.size = p.size.clamp(0.1, 1.0);
        p.swipe_close = p.swipe_close.clamp(0.05, 0.9);
        p.auto_close_s = p.auto_close_s.min(3600);
        self.theme.dim = self.theme.dim.clamp(0.0, 1.0);
        self.theme.animation_ms = self.theme.animation_ms.min(2_000);
        self.gesture.strip_width = self.gesture.strip_width.clamp(0.01, 0.25);

        for (i, w) in self.widget.iter().enumerate() {
            let name = if w.label.is_empty() {
                format!("Widget {}", i + 1)
            } else {
                format!("\"{}\"", w.label)
            };
            if w.col < 1 || w.row < 1 || w.w < 1 || w.h < 1 {
                return Err(format!("{name}: col, row, w and h start at 1"));
            }
            let right = w.col.saturating_add(w.w - 1);
            let bottom = w.row.saturating_add(w.h - 1);
            if right > self.grid.columns || bottom > self.grid.rows {
                return Err(format!(
                    "{name}: lies outside the grid ({}x{})",
                    self.grid.columns, self.grid.rows
                ));
            }
            if w.timeout_ms.is_some_and(|t| !(100..=600_000).contains(&t)) {
                return Err(format!("{name}: timeout_ms must be between 100 and 600000"));
            }
            if w.confirm && !matches!(w.kind, Kind::Button { .. } | Kind::Toggle { .. }) {
                return Err(format!(
                    "{name}: confirm is only available for button and toggle"
                ));
            }
            if let Kind::Slider { min, max, step, .. } = w.kind {
                // Phrased so that NaN is rejected as well.
                let valid = max > min && step > 0.0;
                if !valid {
                    return Err(format!("{name}: needs max > min and step > 0"));
                }
            }
        }
        Ok(())
    }

    pub fn socket_path(&self) -> PathBuf {
        self.socket
            .clone()
            .unwrap_or_else(touchveil_common::default_socket)
    }
}

/// First existing config.toml (same search order as the daemon).
pub fn find() -> Option<PathBuf> {
    touchveil_common::find_config()
}

/// Reloads the config when the file has changed since it was last loaded.
pub struct Watched {
    pub path: Option<PathBuf>,
    mtime: Option<SystemTime>,
}

impl Watched {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path, mtime: None }
    }

    /// `None`: unchanged. `Some(Ok)`: reloaded. `Some(Err)`: file broken.
    pub fn poll(&mut self) -> Option<Result<Config, String>> {
        let Some(path) = &self.path else {
            if self.mtime.is_none() {
                self.mtime = Some(SystemTime::UNIX_EPOCH);
                return Some(Ok(Config::default()));
            }
            return None;
        };
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if mtime.is_some() && mtime == self.mtime {
            return None;
        }
        self.mtime = mtime;
        Some(load(path))
    }
}

fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(Color::parse("#fff").unwrap(), Color([255, 255, 255, 255]));
        assert_eq!(
            Color::parse("#10203040").unwrap(),
            Color([0x10, 0x20, 0x30, 0x40])
        );
        assert!(Color::parse("123456").is_err());
        assert!(Color::parse("#12345").is_err());
    }

    #[test]
    fn full_example_parses() {
        let cfg = Config::parse(include_str!("../../../config.example.toml")).unwrap();
        assert!(!cfg.widget.is_empty());
    }

    #[test]
    fn widget_outside_grid_is_rejected() {
        let e = Config::parse(
            "[grid]\ncolumns = 2\nrows = 2\n[[widget]]\ntype = \"button\"\nexec = \"true\"\ncol = 2\nrow = 1\nw = 2\n",
        )
        .unwrap_err();
        assert!(e.contains("outside the grid"), "{e}");
    }

    #[test]
    fn unknown_widget_key_is_rejected() {
        assert!(Config::parse(
            "[[widget]]\ntype = \"button\"\nexec = \"x\"\ncol = 1\nrow = 1\nlabl = \"x\"\n"
        )
        .is_err());
    }

    #[test]
    fn timeouts_have_defaults_and_can_be_set() {
        let cfg = Config::parse(
            "[[widget]]\ntype = \"display\"\nexec = \"x\"\ncol = 1\nrow = 1\n\
             [[widget]]\ntype = \"display\"\nexec = \"x\"\ncol = 1\nrow = 2\ntimeout_ms = 15000\n",
        )
        .unwrap();
        assert_eq!(
            cfg.widget[0].timeout(Purpose::Display),
            Duration::from_secs(5)
        );
        assert_eq!(cfg.widget[0].timeout(Purpose::Read), Duration::from_secs(2));
        assert_eq!(
            cfg.widget[1].timeout(Purpose::Display),
            Duration::from_secs(15)
        );
        assert!(Config::parse(
            "[[widget]]\ntype = \"display\"\nexec = \"x\"\ncol = 1\nrow = 1\ntimeout_ms = 0\n"
        )
        .is_err());
    }

    #[test]
    fn animation_has_default_and_upper_limit() {
        assert_eq!(Config::parse("").unwrap().theme.animation_ms, 220);
        let cfg = Config::parse("[theme]\nanimation_ms = 60000\n").unwrap();
        assert_eq!(cfg.theme.animation_ms, 2_000);
    }

    #[test]
    fn edge_hint_defaults_and_strip_width() {
        let cfg = Config::parse("").unwrap();
        assert!(cfg.overlay.edge_hint);
        assert!((cfg.gesture.strip_width - 0.04).abs() < 1e-6);
        let cfg =
            Config::parse("[overlay]\nedge_hint = false\n[gesture]\nstrip_width = 0.9\n").unwrap();
        assert!(!cfg.overlay.edge_hint);
        assert!((cfg.gesture.strip_width - 0.25).abs() < 1e-6);
    }

    #[test]
    fn usability_options() {
        let cfg = Config::parse("").unwrap();
        assert!((cfg.overlay.swipe_close - 0.25).abs() < 1e-6);
        assert_eq!(cfg.overlay.auto_close_s, 0);
        assert!(!cfg.overlay.close_after_action);

        let cfg = Config::parse(
            "[overlay]\nswipe_close = 2.0\nauto_close_s = 99999\nclose_after_action = true\n\
             [[widget]]\ntype = \"button\"\nexec = \"x\"\ncol = 1\nrow = 1\nconfirm = true\n\
             [[widget]]\ntype = \"button\"\nexec = \"x\"\ncol = 1\nrow = 2\nclose = false\n",
        )
        .unwrap();
        assert!((cfg.overlay.swipe_close - 0.9).abs() < 1e-6);
        assert_eq!(cfg.overlay.auto_close_s, 3600);
        assert!(cfg.widget[0].confirm);
        assert!(matches!(
            cfg.widget[0].kind,
            Kind::Button { close: None, .. }
        ));
        assert!(matches!(
            cfg.widget[1].kind,
            Kind::Button {
                close: Some(false),
                ..
            }
        ));

        let e = Config::parse(
            "[[widget]]\ntype = \"slider\"\nexec = \"x\"\ncol = 1\nrow = 1\nconfirm = true\n",
        )
        .unwrap_err();
        assert!(e.contains("confirm"), "{e}");
    }

    #[test]
    fn daemon_sections_are_ignored() {
        let cfg = Config::parse(
            "device = \"x\"\nrotate = 90\n[gesture]\nfingers = 3\nedge = \"top-left\"\n",
        )
        .unwrap();
        assert!(cfg.widget.is_empty());
        assert_eq!(cfg.gesture.edge, Edge::TopLeft);
    }
}
