use crate::dimensions::ScaleFactor;
use crate::errors::{DomainError, Result};
use crate::value_objects::anchor::AnchorEdges;
use crate::value_objects::dimensions::SurfaceDimension;
use crate::value_objects::keyboard_interactivity::KeyboardInteractivity;
use crate::value_objects::layer::Layer;
use crate::value_objects::margins::Margins;
use crate::value_objects::output_policy::OutputPolicy;

/// Complete configuration for a layer-shell surface
///
/// Contains all positioning, sizing, and behavioral properties for a surface.
/// Use with `ShellConfig` for declarative configuration or build via `ShellBuilder`.
///
/// # Wayland Protocol Requirements
///
/// According to the wlr-layer-shell protocol, dimensions and anchors must be coordinated:
/// - If width is 0, the surface must be anchored to both left and right edges
/// - If height is 0, the surface must be anchored to both top and bottom edges
///
/// Use `validate()` to check protocol compliance before use.
#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    pub dimensions: SurfaceDimension,
    pub margin: Margins,
    pub exclusive_zone: i32,
    pub scale_factor: ScaleFactor,
    pub namespace: String,
    pub layer: Layer,
    pub anchor: AnchorEdges,
    pub keyboard_interactivity: KeyboardInteractivity,
    pub output_policy: OutputPolicy,
}

impl SurfaceConfig {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dimensions: SurfaceDimension::default(),
            margin: Margins::default(),
            exclusive_zone: -1,
            namespace: "layer-shika".to_owned(),
            scale_factor: ScaleFactor::default(),
            layer: Layer::default(),
            anchor: AnchorEdges::default(),
            keyboard_interactivity: KeyboardInteractivity::default(),
            output_policy: OutputPolicy::default(),
        }
    }

    /// Validates the surface configuration according to Wayland layer-shell protocol requirements.
    ///
    /// According to the protocol:
    /// - If width is 0, the surface must be anchored to both left and right edges
    /// - If height is 0, the surface must be anchored to both top and bottom edges
    pub fn validate(&self) -> Result<()> {
        if self.dimensions.width() == 0 && !(self.anchor.has_left() && self.anchor.has_right()) {
            return Err(DomainError::Configuration {
                message: "Width is 0 but surface is not anchored to both left and right edges. \
                          According to wlr-layer-shell protocol, you must set your anchor to \
                          opposite edges in the dimensions you omit."
                    .to_string(),
            });
        }

        if self.dimensions.height() == 0 && !(self.anchor.has_top() && self.anchor.has_bottom()) {
            return Err(DomainError::Configuration {
                message: "Height is 0 but surface is not anchored to both top and bottom edges. \
                          According to wlr-layer-shell protocol, you must set your anchor to \
                          opposite edges in the dimensions you omit."
                    .to_string(),
            });
        }

        Ok(())
    }
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self::new()
    }
}
