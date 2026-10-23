/// Spacing margins around a surface in pixels
#[derive(Debug, Clone, Copy, Default)]
pub struct Margins {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Margins {
    pub const fn all(px: i32) -> Self {
        Self {
            top: px,
            right: px,
            bottom: px,
            left: px,
        }
    }

    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub const fn new(top: i32, right: i32, bottom: i32, left: i32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl From<i32> for Margins {
    fn from(px: i32) -> Self {
        Self::all(px)
    }
}

impl From<(i32, i32)> for Margins {
    fn from((vertical, horizontal): (i32, i32)) -> Self {
        Self::symmetric(vertical, horizontal)
    }
}

impl From<(i32, i32, i32, i32)> for Margins {
    fn from((top, right, bottom, left): (i32, i32, i32, i32)) -> Self {
        Self::new(top, right, bottom, left)
    }
}
