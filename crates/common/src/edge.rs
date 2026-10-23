//! Edges and corners where the gesture starts, and the rotation of the touchscreen.
//! Coordinates are normalized (0..1), (0, 0) is top left.

use serde::Deserialize;

/// Edge or corner where the gesture starts.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    Left,
    #[default]
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Side the panel slides in from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    Left,
    #[default]
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// For corners, the panel slides in from the horizontal side of the corner.
    pub fn side(self) -> Side {
        match self {
            Edge::Left | Edge::TopLeft | Edge::BottomLeft => Side::Left,
            Edge::Right | Edge::TopRight | Edge::BottomRight => Side::Right,
            Edge::Top => Side::Top,
            Edge::Bottom => Side::Bottom,
        }
    }

    /// Is (x, y) in the start zone? `strip` is the width of the edge strip;
    /// corners are squares of twice the strip width so they are easy to hit.
    pub fn in_zone(self, x: f32, y: f32, strip: f32) -> bool {
        let c = strip * 2.0;
        match self {
            Edge::Left => x <= strip,
            Edge::Right => x >= 1.0 - strip,
            Edge::Top => y <= strip,
            Edge::Bottom => y >= 1.0 - strip,
            Edge::TopLeft => x <= c && y <= c,
            Edge::TopRight => x >= 1.0 - c && y <= c,
            Edge::BottomLeft => x <= c && y >= 1.0 - c,
            Edge::BottomRight => x >= 1.0 - c && y >= 1.0 - c,
        }
    }

    /// Distance away from the edge into the screen, from `start` to `cur`. For corners the
    /// larger of the two directions counts, so you can drag horizontally or vertically.
    pub fn inward(self, start: (f32, f32), cur: (f32, f32)) -> f32 {
        let right = cur.0 - start.0;
        let down = cur.1 - start.1;
        match self {
            Edge::Left => right,
            Edge::Right => -right,
            Edge::Top => down,
            Edge::Bottom => -down,
            Edge::TopLeft => right.max(down),
            Edge::TopRight => (-right).max(down),
            Edge::BottomLeft => right.max(-down),
            Edge::BottomRight => (-right).max(-down),
        }
    }
}

/// Rotates normalized raw coordinates of the touch device to the screen orientation.
/// `deg` is 0, 90, 180 or 270 (other values leave the coordinates unchanged).
pub fn rotate(deg: u16, x: f32, y: f32) -> (f32, f32) {
    match deg {
        90 => (1.0 - y, x),
        180 => (1.0 - x, 1.0 - y),
        270 => (y, 1.0 - x),
        _ => (x, y),
    }
}

/// Checks a `rotate` value from the config.
pub fn check_rotation(deg: u16) -> Result<u16, String> {
    match deg {
        0 | 90 | 180 | 270 => Ok(deg),
        _ => Err(format!("rotate = {deg}: allowed are 0, 90, 180, 270")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_corners() {
        assert!(Edge::Right.in_zone(0.99, 0.5, 0.04));
        assert!(!Edge::Right.in_zone(0.9, 0.5, 0.04));
        assert!(Edge::Top.in_zone(0.5, 0.01, 0.04));
        assert!(Edge::BottomLeft.in_zone(0.05, 0.95, 0.04));
        assert!(!Edge::BottomLeft.in_zone(0.05, 0.5, 0.04));
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(near(Edge::Bottom.inward((0.5, 1.0), (0.5, 0.7)), 0.3));
        // Corner: dragging horizontally or vertically counts
        assert!(near(Edge::TopRight.inward((1.0, 0.0), (0.8, 0.1)), 0.2));
        assert!(near(Edge::TopRight.inward((1.0, 0.0), (0.95, 0.4)), 0.4));
        assert_eq!(Edge::TopRight.side(), Side::Right);
        assert_eq!(Edge::Top.side(), Side::Top);
    }

    #[test]
    fn rotation_maps_corners() {
        assert_eq!(rotate(0, 0.2, 0.3), (0.2, 0.3));
        assert_eq!(rotate(90, 0.0, 0.0), (1.0, 0.0));
        assert_eq!(rotate(180, 0.0, 0.0), (1.0, 1.0));
        assert_eq!(rotate(270, 0.0, 0.0), (0.0, 1.0));
        assert!(check_rotation(45).is_err());
    }

    #[test]
    fn edge_names_in_config() {
        #[derive(Deserialize)]
        struct C {
            edge: Edge,
        }
        let c: C = toml::from_str("edge = \"bottom-right\"").unwrap();
        assert_eq!(c.edge, Edge::BottomRight);
        assert!(toml::from_str::<C>("edge = \"middle\"").is_err());
    }
}
