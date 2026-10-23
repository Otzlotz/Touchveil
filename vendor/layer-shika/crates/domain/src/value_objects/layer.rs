/// Vertical stacking layer for layer-shell surfaces
///
/// Determines which layer a surface appears in, affecting visibility and stacking order.
/// Defaults to `Top` for typical panels and bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    /// Lowest layer, typically for wallpapers
    Background,
    /// Below normal windows
    Bottom,
    /// Above normal windows, default for bars/panels
    #[default]
    Top,
    /// Highest layer, above all other content
    Overlay,
}
