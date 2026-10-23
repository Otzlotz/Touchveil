//! Line-based protocol over the Unix socket between daemon and overlay.

use std::fmt;
use std::str::FromStr;

/// Daemon -> overlay, one message per line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToOverlay {
    /// Progress of the gesture, 0..1.
    Progress(f32),
    Open,
    Close,
    Down {
        slot: u32,
        x: f32,
        y: f32,
    },
    Move {
        slot: u32,
        x: f32,
        y: f32,
    },
    Up {
        slot: u32,
    },
    /// A finger rests in the start zone of the gesture (position of the first one) while
    /// the overlay is closed. Only serves as a hint of where the gesture starts.
    Hint {
        x: f32,
        y: f32,
    },
    /// No finger left in the start zone.
    HintOff,
}

/// Overlay -> daemon, one message per line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToDaemon {
    /// Open the overlay without a gesture (e.g. from the tray).
    Open,
    /// Close the overlay and release the touchscreen again.
    Close,
}

impl fmt::Display for ToOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToOverlay::Progress(p) => write!(f, "progress {p:.3}"),
            ToOverlay::Open => f.write_str("open"),
            ToOverlay::Close => f.write_str("close"),
            ToOverlay::Down { slot, x, y } => write!(f, "down {slot} {x:.4} {y:.4}"),
            ToOverlay::Move { slot, x, y } => write!(f, "move {slot} {x:.4} {y:.4}"),
            ToOverlay::Up { slot } => write!(f, "up {slot}"),
            ToOverlay::Hint { x, y } => write!(f, "hint {x:.4} {y:.4}"),
            ToOverlay::HintOff => f.write_str("hint off"),
        }
    }
}

impl FromStr for ToOverlay {
    type Err = String;

    fn from_str(line: &str) -> Result<Self, String> {
        let bad = || format!("invalid message: {line:?}");
        let mut it = line.split_whitespace();
        let mut next = || it.next().ok_or_else(bad);
        let msg = match next()? {
            "progress" => ToOverlay::Progress(next()?.parse().map_err(|_| bad())?),
            "open" => ToOverlay::Open,
            "close" => ToOverlay::Close,
            cmd @ ("down" | "move") => {
                let slot = next()?.parse().map_err(|_| bad())?;
                let x = next()?.parse().map_err(|_| bad())?;
                let y = next()?.parse().map_err(|_| bad())?;
                if cmd == "down" {
                    ToOverlay::Down { slot, x, y }
                } else {
                    ToOverlay::Move { slot, x, y }
                }
            }
            "up" => ToOverlay::Up {
                slot: next()?.parse().map_err(|_| bad())?,
            },
            "hint" => match next()? {
                "off" => ToOverlay::HintOff,
                x => ToOverlay::Hint {
                    x: x.parse().map_err(|_| bad())?,
                    y: next()?.parse().map_err(|_| bad())?,
                },
            },
            _ => return Err(bad()),
        };
        Ok(msg)
    }
}

impl fmt::Display for ToDaemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToDaemon::Open => f.write_str("open"),
            ToDaemon::Close => f.write_str("close"),
        }
    }
}

impl FromStr for ToDaemon {
    type Err = String;

    fn from_str(line: &str) -> Result<Self, String> {
        match line.trim() {
            "open" => Ok(ToDaemon::Open),
            "close" => Ok(ToDaemon::Close),
            _ => Err(format!("invalid message: {line:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_roundtrip() {
        for m in [
            ToOverlay::Progress(0.5),
            ToOverlay::Open,
            ToOverlay::Close,
            ToOverlay::Down {
                slot: 3,
                x: 0.25,
                y: 0.75,
            },
            ToOverlay::Move {
                slot: 0,
                x: 1.0,
                y: 0.0,
            },
            ToOverlay::Up { slot: 9 },
            ToOverlay::Hint { x: 1.0, y: 0.5 },
            ToOverlay::HintOff,
        ] {
            assert_eq!(m.to_string().parse::<ToOverlay>(), Ok(m));
        }
        assert_eq!("close\n".parse::<ToDaemon>(), Ok(ToDaemon::Close));
        assert_eq!(ToDaemon::Open.to_string().parse(), Ok(ToDaemon::Open));
        assert!("down 1 x 2".parse::<ToOverlay>().is_err());
        assert!("progress".parse::<ToOverlay>().is_err());
        assert!("hint 0.5".parse::<ToOverlay>().is_err());
    }
}
