use serde::Deserialize;
use std::path::PathBuf;
pub use touchveil_common::Edge;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Gesture {
    /// Edge or corner in screen coordinates (after `rotate`).
    pub edge: Edge,
    /// Width of the edge strip as a fraction of the screen size (0.01..0.25).
    pub strip_width: f32,
    /// Number of fingers required (1..=slots of the device).
    pub fingers: usize,
    /// Time window in which the finger at the edge waits for the others. 0: no timeout,
    /// the finger waits as long as it rests in the edge strip.
    pub arm_window_ms: u64,
    /// Distance for 100 % progress as a fraction of the screen size.
    pub travel: f32,
    /// From this progress (0..1) on, lifting the fingers opens the overlay.
    pub open_threshold: f32,
    /// A quick flick opens even below `open_threshold`, a quick flick back
    /// cancels.
    pub fling: bool,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            edge: Edge::Right,
            strip_width: 0.04,
            fingers: 2,
            arm_window_ms: 0,
            travel: 0.35,
            open_threshold: 0.5,
            fling: true,
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Actions {
    /// Started on open/close, e.g. ["hyprctl","dispatch","workspace","9"].
    pub on_open: Vec<String>,
    pub on_close: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    /// Part of the touch device's name (see `libinput list-devices`). If not set, the
    /// first touchscreen is picked automatically.
    pub device: Option<String>,
    /// Path of the Unix socket; default: $XDG_RUNTIME_DIR/touchveil.sock
    pub socket: Option<PathBuf>,
    /// Rotation of the touchscreen relative to the screen image (0, 90, 180, 270).
    pub rotate: u16,
    pub gesture: Gesture,
    pub actions: Actions,
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self, String> {
        let path = match path {
            Some(p) => {
                if !p.is_file() {
                    return Err(format!("{}: file not found", p.display()));
                }
                Some(p)
            }
            None => touchveil_common::find_config(),
        };

        let mut cfg = match &path {
            Some(p) => {
                eprintln!("Config: {}", p.display());
                let text =
                    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?
            }
            None => {
                eprintln!(
                    "No config.toml found (working directory: {}), searched in:",
                    std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                );
                for p in touchveil_common::config_candidates() {
                    eprintln!("  {}", p.display());
                }
                Config::default()
            }
        };

        let g = &mut cfg.gesture;
        g.strip_width = g.strip_width.clamp(0.01, 0.25);
        g.travel = g.travel.clamp(0.05, 1.0);
        g.open_threshold = g.open_threshold.clamp(0.05, 1.0);
        g.fingers = g.fingers.max(1);
        if g.arm_window_ms > 0 {
            g.arm_window_ms = g.arm_window_ms.clamp(10, 5000);
        }
        touchveil_common::check_rotation(cfg.rotate)?;
        Ok(cfg)
    }
    pub fn socket_path(&self) -> PathBuf {
        self.socket
            .clone()
            .unwrap_or_else(touchveil_common::default_socket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_example_config_parses() {
        // The example config also contains the overlay sections; they must be ignored.
        let cfg: Config = toml::from_str(include_str!("../../../config.example.toml")).unwrap();
        assert_eq!(cfg.gesture.fingers, 2);
    }
}
