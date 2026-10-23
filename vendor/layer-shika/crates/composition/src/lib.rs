#![allow(clippy::pub_use)]

mod event_loop;
mod layer_surface;
mod selection;
mod selector;
mod shell;
mod shell_config;
mod shell_runtime;
mod surface_registry;
mod system;
pub mod value_conversion;

use std::result::Result as StdResult;

pub use event_loop::{EventLoopHandle, ShellEventLoop};
use layer_shika_adapters::errors::LayerShikaError;
pub use layer_shika_adapters::platform::{slint, slint_interpreter};
pub use layer_shika_domain::entities::output_registry::OutputRegistry;
use layer_shika_domain::errors::DomainError;
pub use layer_shika_domain::value_objects::anchor::AnchorEdges;
pub use layer_shika_domain::value_objects::handle::{Handle, SurfaceHandle};
pub use layer_shika_domain::value_objects::keyboard_interactivity::KeyboardInteractivity;
pub use layer_shika_domain::value_objects::layer::Layer;
pub use layer_shika_domain::value_objects::output_handle::OutputHandle;
pub use layer_shika_domain::value_objects::output_info::{OutputGeometry, OutputInfo};
pub use layer_shika_domain::value_objects::output_policy::OutputPolicy;
pub use layer_shika_domain::value_objects::surface_instance_id::SurfaceInstanceId;
pub use layer_surface::{LayerSurfaceHandle, ShellSurfaceConfigHandler};
pub use selection::{PropertyError, Selection, SelectionResult};
pub use selector::{Output, Selector, Surface, SurfaceInfo};
pub use shell::{
    DEFAULT_COMPONENT_NAME, Shell, ShellBuilder, ShellEventContext, SurfaceConfigBuilder,
};
pub use shell_config::{CompiledUiSource, ShellConfig, SurfaceComponentConfig};
pub use shell_runtime::{DEFAULT_SURFACE_NAME, ShellRuntime};
pub use surface_registry::{SurfaceDefinition, SurfaceEntry, SurfaceMetadata, SurfaceRegistry};
pub use system::{
    CallbackContext, EventDispatchContext, RuntimeSurfaceConfigBuilder, ShellControl,
    SurfaceControlHandle, SurfaceTarget,
};
pub use value_conversion::IntoValue;

pub(crate) mod logger {
    #[cfg(all(feature = "log", feature = "tracing"))]
    compile_error!("Cannot use both logging backend at one time");

    #[cfg(feature = "log")]
    pub use log::{debug, error, info, warn};
    #[cfg(feature = "tracing")]
    pub use tracing::{debug, error, info, warn};
}

pub mod calloop {
    pub use layer_shika_adapters::platform::calloop::*;
}

/// Result type alias using layer-shika's Error
pub type Result<T> = StdResult<T, Error>;

/// Error types for layer-shika operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Adapter error: {0}")]
    Adapter(#[from] LayerShikaError),

    #[error("Domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("App has been dropped")]
    SystemDropped,
}

pub mod prelude {
    pub use layer_shika_adapters::platform::wayland::Anchor;
    pub use layer_shika_domain::prelude::{
        LogicalPosition, LogicalRect, LogicalSize, Margins, PhysicalSize, ScaleFactor,
        SurfaceConfig, SurfaceDimension, UiSource,
    };

    pub use crate::calloop::{Generic, Interest, Mode, PostAction, RegistrationToken, Timer};
    pub use crate::{
        AnchorEdges, CompiledUiSource, DEFAULT_COMPONENT_NAME, DEFAULT_SURFACE_NAME,
        EventDispatchContext, EventLoopHandle, Handle, IntoValue, KeyboardInteractivity, Layer,
        LayerSurfaceHandle, Output, OutputGeometry, OutputHandle, OutputInfo, OutputPolicy,
        OutputRegistry, PropertyError, Result, Selection, SelectionResult, Selector, Shell,
        ShellBuilder, ShellConfig, ShellControl, ShellEventContext, ShellEventLoop, ShellRuntime,
        ShellSurfaceConfigHandler, Surface, SurfaceComponentConfig, SurfaceConfigBuilder,
        SurfaceControlHandle, SurfaceDefinition, SurfaceEntry, SurfaceHandle, SurfaceInfo,
        SurfaceMetadata, SurfaceRegistry, slint, slint_interpreter,
    };
}
