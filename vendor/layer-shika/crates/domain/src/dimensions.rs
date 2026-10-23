use crate::errors::DomainError;

/// Size in logical pixels, independent of display scaling
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalSize {
    width: f32,
    height: f32,
}

impl LogicalSize {
    pub fn new(width: f32, height: f32) -> Result<Self, DomainError> {
        if width <= 0.0 || height <= 0.0 {
            return Err(DomainError::InvalidInput {
                message: format!("Dimensions must be positive, got width={width}, height={height}"),
            });
        }
        if !width.is_finite() || !height.is_finite() {
            return Err(DomainError::InvalidInput {
                message: "Dimensions must be finite values".to_string(),
            });
        }
        Ok(Self { width, height })
    }

    pub const fn from_raw(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const fn width(&self) -> f32 {
        self.width
    }

    pub const fn height(&self) -> f32 {
        self.height
    }

    pub fn to_physical(&self, scale_factor: ScaleFactor) -> PhysicalSize {
        scale_factor.to_physical(*self)
    }

    pub fn as_tuple(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn clamp_position(
        &self,
        position: LogicalPosition,
        bounds: LogicalSize,
    ) -> LogicalPosition {
        let max_x = (bounds.width - self.width).max(0.0);
        let max_y = (bounds.height - self.height).max(0.0);

        LogicalPosition::new(
            position.x().max(0.0).min(max_x),
            position.y().max(0.0).min(max_y),
        )
    }
}

impl Default for LogicalSize {
    fn default() -> Self {
        Self {
            width: 120.0,
            height: 120.0,
        }
    }
}

/// Size in physical device pixels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalSize {
    width: u32,
    height: u32,
}

impl PhysicalSize {
    pub fn new(width: u32, height: u32) -> Result<Self, DomainError> {
        if width == 0 || height == 0 {
            return Err(DomainError::InvalidDimensions { width, height });
        }
        Ok(Self { width, height })
    }

    pub const fn from_raw(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn to_logical(&self, scale_factor: ScaleFactor) -> LogicalSize {
        scale_factor.to_logical(*self)
    }

    pub fn as_tuple(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Default for PhysicalSize {
    fn default() -> Self {
        Self {
            width: 120,
            height: 120,
        }
    }
}

/// Display scaling factor for converting between logical and physical pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactor(f32);

impl ScaleFactor {
    pub fn new(factor: f32) -> Result<Self, DomainError> {
        if factor <= 0.0 {
            return Err(DomainError::InvalidInput {
                message: format!("Scale factor must be positive, got {factor}"),
            });
        }
        if !factor.is_finite() {
            return Err(DomainError::InvalidInput {
                message: "Scale factor must be a finite value".to_string(),
            });
        }
        Ok(Self(factor))
    }

    pub const fn from_raw(factor: f32) -> Self {
        Self(factor)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn from_120ths(scale_120ths: u32) -> Self {
        Self(scale_120ths as f32 / 120.0)
    }

    pub const fn value(&self) -> f32 {
        self.0
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_physical(&self, logical: LogicalSize) -> PhysicalSize {
        let width = (logical.width * self.0).round() as u32;
        let height = (logical.height * self.0).round() as u32;
        PhysicalSize::from_raw(width.max(1), height.max(1))
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn to_logical(&self, physical: PhysicalSize) -> LogicalSize {
        let width = physical.width as f32 / self.0;
        let height = physical.height as f32 / self.0;
        LogicalSize::from_raw(width, height)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn buffer_scale(&self) -> i32 {
        self.0.round() as i32
    }

    pub fn scale_coordinate(&self, logical_coord: f32) -> f32 {
        logical_coord * self.0
    }

    pub fn unscale_coordinate(&self, physical_coord: f32) -> f32 {
        physical_coord / self.0
    }
}

impl Default for ScaleFactor {
    fn default() -> Self {
        Self(1.0)
    }
}

impl TryFrom<f32> for ScaleFactor {
    type Error = DomainError;

    fn try_from(factor: f32) -> Result<Self, Self::Error> {
        Self::new(factor)
    }
}

/// Position in logical pixels, independent of display scaling
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalPosition {
    x: f32,
    y: f32,
}

impl LogicalPosition {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn x(&self) -> f32 {
        self.x
    }

    pub const fn y(&self) -> f32 {
        self.y
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn to_physical(&self, scale_factor: ScaleFactor) -> PhysicalPosition {
        PhysicalPosition::new(
            (self.x * scale_factor.value()).round() as i32,
            (self.y * scale_factor.value()).round() as i32,
        )
    }

    pub fn as_tuple(&self) -> (f32, f32) {
        (self.x, self.y)
    }
}

impl Default for LogicalPosition {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// Rectangle in logical pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LogicalRect {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn x(&self) -> f32 {
        self.x
    }

    #[must_use]
    pub const fn y(&self) -> f32 {
        self.y
    }

    #[must_use]
    pub const fn width(&self) -> f32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> f32 {
        self.height
    }

    #[must_use]
    pub const fn origin(&self) -> LogicalPosition {
        LogicalPosition::new(self.x, self.y)
    }

    #[must_use]
    pub const fn size(&self) -> LogicalSize {
        LogicalSize::from_raw(self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalPosition {
    x: i32,
    y: i32,
}

impl PhysicalPosition {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn x(&self) -> i32 {
        self.x
    }

    pub const fn y(&self) -> i32 {
        self.y
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn to_logical(&self, scale_factor: ScaleFactor) -> LogicalPosition {
        LogicalPosition::new(
            self.x as f32 / scale_factor.value(),
            self.y as f32 / scale_factor.value(),
        )
    }

    pub fn as_tuple(&self) -> (i32, i32) {
        (self.x, self.y)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for PhysicalPosition {
    fn default() -> Self {
        Self { x: 0, y: 0 }
    }
}
