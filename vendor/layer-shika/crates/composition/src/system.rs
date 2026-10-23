use std::rc::Rc;

use layer_shika_adapters::platform::calloop::channel;
use layer_shika_adapters::platform::slint_interpreter::{CompilationResult, ComponentInstance};
use layer_shika_adapters::{AppState, SurfaceState};
use layer_shika_domain::config::SurfaceConfig;
use layer_shika_domain::entities::output_registry::OutputRegistry;
use layer_shika_domain::errors::DomainError;
use layer_shika_domain::prelude::{
    AnchorEdges, KeyboardInteractivity, Layer, Margins, OutputPolicy, ScaleFactor,
};
use layer_shika_domain::value_objects::dimensions::SurfaceDimension;
use layer_shika_domain::value_objects::handle::SurfaceHandle;
use layer_shika_domain::value_objects::output_handle::OutputHandle;
use layer_shika_domain::value_objects::output_info::OutputInfo;
use layer_shika_domain::value_objects::surface_instance_id::SurfaceInstanceId;
use smithay_client_toolkit::reexports::client::protocol::wl_region::WlRegion;

use crate::event_loop::FromAppState;
use crate::layer_surface::LayerSurfaceHandle;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum SurfaceTarget {
    ByInstance(SurfaceInstanceId),
    ByHandle(SurfaceHandle),
    ByName(String),
    ByNameAndOutput { name: String, output: OutputHandle },
}

pub enum SurfaceCommand {
    Resize {
        target: SurfaceTarget,
        width: u32,
        height: u32,
    },
    SetAnchor {
        target: SurfaceTarget,
        anchor: AnchorEdges,
    },
    SetExclusiveZone {
        target: SurfaceTarget,
        zone: i32,
    },
    SetMargins {
        target: SurfaceTarget,
        margins: Margins,
    },
    SetLayer {
        target: SurfaceTarget,
        layer: Layer,
    },
    SetOutputPolicy {
        target: SurfaceTarget,
        policy: OutputPolicy,
    },
    SetInputRegion {
        target: SurfaceTarget,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    SetScaleFactor {
        target: SurfaceTarget,
        factor: ScaleFactor,
    },
    SetKeyboardInteractivity {
        target: SurfaceTarget,
        mode: KeyboardInteractivity,
    },
    ApplyConfig {
        target: SurfaceTarget,
        config: SurfaceConfig,
    },
}

pub enum ShellCommand {
    Surface(SurfaceCommand),
    Render,
}

/// Context provided to callback handlers
///
/// # Callback Signatures
///
/// - Basic: `Fn(CallbackContext) -> impl IntoValue`
/// - With args: `Fn(&[Value], CallbackContext) -> impl IntoValue`
pub struct CallbackContext {
    instance_id: SurfaceInstanceId,
    surface_name: String,
    control: ShellControl,
}

impl CallbackContext {
    pub fn new(
        instance_id: SurfaceInstanceId,
        surface_name: String,
        control: ShellControl,
    ) -> Self {
        Self {
            instance_id,
            surface_name,
            control,
        }
    }

    /// Returns the surface instance identifier
    pub const fn instance_id(&self) -> &SurfaceInstanceId {
        &self.instance_id
    }

    /// Returns the surface handle
    pub const fn surface_handle(&self) -> SurfaceHandle {
        self.instance_id.surface()
    }

    /// Returns the output handle
    pub const fn output_handle(&self) -> OutputHandle {
        self.instance_id.output()
    }

    /// Returns the surface name
    pub fn surface_name(&self) -> &str {
        &self.surface_name
    }

    /// Returns a reference to the shell control handle
    pub const fn control(&self) -> &ShellControl {
        &self.control
    }

    /// Returns a control handle for this specific surface instance
    pub fn this_instance(&self) -> SurfaceControlHandle {
        self.control.surface_instance(&self.instance_id)
    }

    /// Returns a control handle for all instances of this surface
    pub fn all_surface_instances(&self) -> SurfaceControlHandle {
        self.control.surface_by_handle(self.surface_handle())
    }

    /// Returns a control handle for all surfaces with this name
    pub fn all_named(&self) -> SurfaceControlHandle {
        self.control.surface_by_name(&self.surface_name)
    }

    /// Returns a control handle for all surfaces with this name on the current output
    pub fn all_named_on_this_output(&self) -> SurfaceControlHandle {
        self.control
            .surface_by_name_and_output(&self.surface_name, self.output_handle())
    }
}

/// Handle for runtime control of shell operations
///
/// Cloneable and can be sent across threads for triggering shell operations.
#[derive(Clone)]
pub struct ShellControl {
    sender: channel::Sender<ShellCommand>,
}

impl ShellControl {
    pub fn new(sender: channel::Sender<ShellCommand>) -> Self {
        Self { sender }
    }

    /// Requests a redraw of all surfaces
    pub fn request_redraw(&self) -> Result<()> {
        self.sender
            .send(ShellCommand::Render)
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Returns a control handle for a specific surface instance
    pub fn surface_instance(&self, id: &SurfaceInstanceId) -> SurfaceControlHandle {
        SurfaceControlHandle {
            target: SurfaceTarget::ByInstance(*id),
            sender: self.sender.clone(),
        }
    }

    /// Returns a control handle for all instances of a surface by handle
    pub fn surface_by_handle(&self, handle: SurfaceHandle) -> SurfaceControlHandle {
        SurfaceControlHandle {
            target: SurfaceTarget::ByHandle(handle),
            sender: self.sender.clone(),
        }
    }

    /// Returns a control handle for all surfaces with the given name
    pub fn surface_by_name(&self, name: impl Into<String>) -> SurfaceControlHandle {
        SurfaceControlHandle {
            target: SurfaceTarget::ByName(name.into()),
            sender: self.sender.clone(),
        }
    }

    /// Returns a control handle for surfaces with the given name on a specific output
    pub fn surface_by_name_and_output(
        &self,
        name: impl Into<String>,
        output: OutputHandle,
    ) -> SurfaceControlHandle {
        SurfaceControlHandle {
            target: SurfaceTarget::ByNameAndOutput {
                name: name.into(),
                output,
            },
            sender: self.sender.clone(),
        }
    }

    /// Alias for `surface_by_name`
    pub fn surface(&self, name: impl Into<String>) -> SurfaceControlHandle {
        self.surface_by_name(name)
    }
}

/// Handle for runtime control of a specific surface
///
/// Operations apply to all matching instances. Changes are queued and applied asynchronously.
/// Obtained via `ShellControl::surface()`.
pub struct SurfaceControlHandle {
    target: SurfaceTarget,
    sender: channel::Sender<ShellCommand>,
}

impl SurfaceControlHandle {
    /// Resizes the surface to the specified dimensions
    pub fn resize(&self, width: u32, height: u32) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::Resize {
                target: self.target.clone(),
                width,
                height,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the surface width
    pub fn set_width(&self, width: u32) -> Result<()> {
        self.resize(width, 0)
    }

    /// Sets the surface height
    pub fn set_height(&self, height: u32) -> Result<()> {
        self.resize(0, height)
    }

    /// Sets the anchor edges for the surface
    pub fn set_anchor(&self, anchor: AnchorEdges) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetAnchor {
                target: self.target.clone(),
                anchor,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the exclusive zone for the surface
    pub fn set_exclusive_zone(&self, zone: i32) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetExclusiveZone {
                target: self.target.clone(),
                zone,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the margins for the surface
    pub fn set_margins(&self, margins: impl Into<Margins>) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetMargins {
                target: self.target.clone(),
                margins: margins.into(),
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the layer for the surface
    pub fn set_layer(&self, layer: Layer) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetLayer {
                target: self.target.clone(),
                layer,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the output policy for the surface
    pub fn set_output_policy(&self, policy: OutputPolicy) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetOutputPolicy {
                target: self.target.clone(),
                policy,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the input region for the surface
    pub fn set_input_region(&self, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetInputRegion {
                target: self.target.clone(),
                x,
                y,
                width,
                height,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the scale factor for the surface
    pub fn set_scale_factor(&self, factor: ScaleFactor) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::SetScaleFactor {
                target: self.target.clone(),
                factor,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Sets the keyboard interactivity mode for the surface
    pub fn set_keyboard_interactivity(&self, mode: KeyboardInteractivity) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(
                SurfaceCommand::SetKeyboardInteractivity {
                    target: self.target.clone(),
                    mode,
                },
            ))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Applies a complete surface configuration
    pub fn apply_config(&self, config: SurfaceConfig) -> Result<()> {
        self.sender
            .send(ShellCommand::Surface(SurfaceCommand::ApplyConfig {
                target: self.target.clone(),
                config,
            }))
            .map_err(|_| Error::Domain(DomainError::ChannelClosed))
    }

    /// Returns a builder for configuring multiple properties at once
    pub fn configure(self) -> RuntimeSurfaceConfigBuilder {
        RuntimeSurfaceConfigBuilder {
            handle: self,
            config: SurfaceConfig::new(),
        }
    }
}

/// Builder for applying multiple configuration changes to a surface at once
///
/// Created via `SurfaceControlHandle::configure()`. Chain configuration methods
/// and call `.apply()` to commit all changes atomically.
/// Builder for applying multiple configuration changes to a surface at once
///
/// All changes are committed together in one compositor round-trip for efficiency.
pub struct RuntimeSurfaceConfigBuilder {
    handle: SurfaceControlHandle,
    config: SurfaceConfig,
}

impl RuntimeSurfaceConfigBuilder {
    /// Sets the surface size
    #[must_use]
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.config.dimensions = SurfaceDimension::from_raw(width, height);
        self
    }

    /// Sets the surface width
    #[must_use]
    pub fn width(mut self, width: u32) -> Self {
        self.config.dimensions = SurfaceDimension::from_raw(width, self.config.dimensions.height());
        self
    }

    /// Sets the surface height
    #[must_use]
    pub fn height(mut self, height: u32) -> Self {
        self.config.dimensions = SurfaceDimension::from_raw(self.config.dimensions.width(), height);
        self
    }

    /// Sets the layer
    #[must_use]
    pub const fn layer(mut self, layer: Layer) -> Self {
        self.config.layer = layer;
        self
    }

    /// Sets the margins
    #[must_use]
    pub fn margins(mut self, margins: impl Into<Margins>) -> Self {
        self.config.margin = margins.into();
        self
    }

    /// Sets the anchor edges
    #[must_use]
    pub const fn anchor(mut self, anchor: AnchorEdges) -> Self {
        self.config.anchor = anchor;
        self
    }

    /// Sets the exclusive zone
    #[must_use]
    pub const fn exclusive_zone(mut self, zone: i32) -> Self {
        self.config.exclusive_zone = zone;
        self
    }

    /// Sets the namespace
    #[must_use]
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.namespace = namespace.into();
        self
    }

    /// Sets the keyboard interactivity mode
    #[must_use]
    pub const fn keyboard_interactivity(mut self, mode: KeyboardInteractivity) -> Self {
        self.config.keyboard_interactivity = mode;
        self
    }

    /// Sets the output policy
    #[must_use]
    pub fn output_policy(mut self, policy: OutputPolicy) -> Self {
        self.config.output_policy = policy;
        self
    }

    /// Sets the scale factor
    #[must_use]
    pub fn scale_factor(mut self, sf: impl TryInto<ScaleFactor, Error = DomainError>) -> Self {
        self.config.scale_factor = sf.try_into().unwrap_or_default();
        self
    }

    /// Applies the configured changes to the surface
    pub fn apply(self) -> Result<()> {
        self.handle.apply_config(self.config)
    }
}

pub struct EventDispatchContext<'a> {
    app_state: &'a mut AppState,
}

impl<'a> FromAppState<'a> for EventDispatchContext<'a> {
    fn from_app_state(app_state: &'a mut AppState) -> Self {
        Self { app_state }
    }
}

impl EventDispatchContext<'_> {
    pub(crate) fn surface_by_instance_mut(
        &mut self,
        surface_handle: SurfaceHandle,
        output_handle: OutputHandle,
    ) -> Option<&mut SurfaceState> {
        self.app_state
            .get_surface_by_instance_mut(surface_handle, output_handle)
    }

    pub(crate) fn surfaces_by_handle_mut(
        &mut self,
        handle: SurfaceHandle,
    ) -> Vec<&mut SurfaceState> {
        self.app_state.surfaces_by_handle_mut(handle)
    }

    pub(crate) fn surfaces_by_name_mut(&mut self, name: &str) -> Vec<&mut SurfaceState> {
        self.app_state.surfaces_by_name_mut(name)
    }

    pub(crate) fn surfaces_by_name_and_output_mut(
        &mut self,
        name: &str,
        output: OutputHandle,
    ) -> Vec<&mut SurfaceState> {
        self.app_state.surfaces_by_name_and_output_mut(name, output)
    }

    pub fn with_surface<F, R>(&self, name: &str, f: F) -> Result<R>
    where
        F: FnOnce(&ComponentInstance) -> R,
    {
        let component = self.get_surface_component(name).ok_or_else(|| {
            Error::Domain(DomainError::SurfaceNotFound {
                message: format!("Surface '{}' not found", name),
            })
        })?;
        Ok(f(component))
    }

    pub fn with_output<F, R>(&self, handle: OutputHandle, f: F) -> Result<R>
    where
        F: FnOnce(&ComponentInstance) -> R,
    {
        let component = self.get_output_component(handle).ok_or_else(|| {
            Error::Domain(DomainError::OutputNotFound {
                message: format!("Output with handle {:?} not found", handle),
            })
        })?;
        Ok(f(component))
    }

    fn get_surface_component(&self, name: &str) -> Option<&ComponentInstance> {
        self.app_state
            .surfaces_by_name(name)
            .first()
            .map(|s| s.component_instance())
    }

    /// Returns the primary component instance
    #[must_use]
    pub fn component_instance(&self) -> Option<&ComponentInstance> {
        self.app_state
            .primary_output()
            .map(SurfaceState::component_instance)
    }

    /// Returns all component instances across all outputs
    pub fn all_component_instances(&self) -> impl Iterator<Item = &ComponentInstance> {
        self.app_state
            .all_outputs()
            .map(SurfaceState::component_instance)
    }

    /// Returns the output registry
    pub const fn output_registry(&self) -> &OutputRegistry {
        self.app_state.output_registry()
    }

    /// Returns the primary output handle
    #[must_use]
    pub fn primary_output_handle(&self) -> Option<OutputHandle> {
        self.app_state.primary_output_handle()
    }

    /// Returns the active output handle
    #[must_use]
    pub fn active_output_handle(&self) -> Option<OutputHandle> {
        self.app_state.active_output_handle()
    }

    /// Returns all outputs with their handles and components
    pub fn outputs(&self) -> impl Iterator<Item = (OutputHandle, &ComponentInstance)> {
        self.app_state
            .outputs_with_handles()
            .map(|(handle, surface)| (handle, surface.component_instance()))
    }

    /// Returns the component for a specific output
    pub fn get_output_component(&self, handle: OutputHandle) -> Option<&ComponentInstance> {
        self.app_state
            .get_output_by_handle(handle)
            .map(SurfaceState::component_instance)
    }

    /// Returns information about a specific output
    pub fn get_output_info(&self, handle: OutputHandle) -> Option<&OutputInfo> {
        self.app_state.get_output_info(handle)
    }

    /// Returns information about all outputs
    pub fn all_output_info(&self) -> impl Iterator<Item = &OutputInfo> {
        self.app_state.all_output_info()
    }

    /// Returns all outputs with their info and components
    pub fn outputs_with_info(&self) -> impl Iterator<Item = (&OutputInfo, &ComponentInstance)> {
        self.app_state
            .outputs_with_info()
            .map(|(info, surface)| (info, surface.component_instance()))
    }

    /// Renders a new frame for all dirty surfaces
    pub fn render_frame_if_dirty(&mut self) -> Result<()> {
        for surface in self.app_state.all_outputs() {
            surface.render_frame_if_dirty()?;
        }
        Ok(())
    }

    /// Returns the compilation result if available
    #[must_use]
    pub fn compilation_result(&self) -> Option<Rc<CompilationResult>> {
        self.app_state
            .primary_output()
            .and_then(SurfaceState::compilation_result)
    }

    pub fn configure_surface<F>(&mut self, name: &str, f: F) -> Result<()>
    where
        F: FnOnce(&ComponentInstance, LayerSurfaceHandle<'_>),
    {
        let surfaces = self.app_state.surfaces_by_name(name);
        let surface = surfaces.first().ok_or_else(|| {
            Error::Domain(DomainError::SurfaceNotFound {
                message: format!("Surface '{}' not found", name),
            })
        })?;

        let handle = LayerSurfaceHandle::from_window_state(surface);
        let component = surface.component_instance();
        f(component, handle);
        Ok(())
    }

    pub fn configure_all_surfaces<F>(&mut self, mut f: F)
    where
        F: FnMut(&ComponentInstance, LayerSurfaceHandle<'_>),
    {
        for surface in self.app_state.all_outputs() {
            let handle = LayerSurfaceHandle::from_window_state(surface);
            let component = surface.component_instance();
            f(component, handle);
        }
    }

    pub fn create_region(&self) -> Option<WlRegion> {
        self.app_state.create_region()
    }
}
