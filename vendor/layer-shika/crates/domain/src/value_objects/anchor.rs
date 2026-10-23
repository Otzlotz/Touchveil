/// Represents which edges of the output a layer surface should be anchored to.
///
/// Use predefined helpers like `top_bar()`, `bottom_bar()`, or build custom configurations
/// with `empty()` combined with `with_top()`, `with_bottom()`, `with_left()`, `with_right()`.
///
/// # Examples
/// ```
/// use layer_shika_domain::value_objects::anchor::AnchorEdges;
///
/// let top_bar = AnchorEdges::top_bar();
/// let custom = AnchorEdges::empty().with_top().with_left();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorEdges(u8);

impl AnchorEdges {
    const TOP: u8 = 1 << 0;
    const BOTTOM: u8 = 1 << 1;
    const LEFT: u8 = 1 << 2;
    const RIGHT: u8 = 1 << 3;

    #[must_use]
    pub const fn new(bits: u8) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn all() -> Self {
        Self(Self::TOP | Self::BOTTOM | Self::LEFT | Self::RIGHT)
    }

    #[must_use]
    pub const fn top_bar() -> Self {
        Self(Self::TOP | Self::LEFT | Self::RIGHT)
    }

    #[must_use]
    pub const fn bottom_bar() -> Self {
        Self(Self::BOTTOM | Self::LEFT | Self::RIGHT)
    }

    #[must_use]
    pub const fn left_bar() -> Self {
        Self(Self::LEFT | Self::TOP | Self::BOTTOM)
    }

    #[must_use]
    pub const fn right_bar() -> Self {
        Self(Self::RIGHT | Self::TOP | Self::BOTTOM)
    }

    #[must_use]
    pub const fn with_top(mut self) -> Self {
        self.0 |= Self::TOP;
        self
    }

    #[must_use]
    pub const fn with_bottom(mut self) -> Self {
        self.0 |= Self::BOTTOM;
        self
    }

    #[must_use]
    pub const fn with_left(mut self) -> Self {
        self.0 |= Self::LEFT;
        self
    }

    #[must_use]
    pub const fn with_right(mut self) -> Self {
        self.0 |= Self::RIGHT;
        self
    }

    #[must_use]
    pub const fn has_top(&self) -> bool {
        self.0 & Self::TOP != 0
    }

    #[must_use]
    pub const fn has_bottom(&self) -> bool {
        self.0 & Self::BOTTOM != 0
    }

    #[must_use]
    pub const fn has_left(&self) -> bool {
        self.0 & Self::LEFT != 0
    }

    #[must_use]
    pub const fn has_right(&self) -> bool {
        self.0 & Self::RIGHT != 0
    }
}

impl Default for AnchorEdges {
    fn default() -> Self {
        Self::top_bar()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Bottom,
    Left,
    Right,
}
