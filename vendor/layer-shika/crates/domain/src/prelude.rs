#![allow(clippy::pub_use)]

pub use crate::config::SurfaceConfig;
pub use crate::dimensions::{
    LogicalPosition, LogicalRect, LogicalSize, PhysicalPosition, PhysicalSize, ScaleFactor,
};
pub use crate::entities::output_registry::OutputRegistry;
pub use crate::errors::{DomainError, Result};
pub use crate::surface_dimensions::SurfaceDimensions;
pub use crate::value_objects::anchor::AnchorEdges;
pub use crate::value_objects::dimensions::SurfaceDimension;
pub use crate::value_objects::handle::{Handle, OutputHandle, SurfaceHandle};
pub use crate::value_objects::keyboard_interactivity::KeyboardInteractivity;
pub use crate::value_objects::layer::Layer;
pub use crate::value_objects::margins::Margins;
pub use crate::value_objects::output_info::{OutputGeometry, OutputInfo};
pub use crate::value_objects::output_policy::OutputPolicy;
pub use crate::value_objects::ui_source::UiSource;
