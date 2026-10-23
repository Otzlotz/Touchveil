use crate::{
    errors::{EventLoopError, LayerShikaError, RenderingError, Result},
    rendering::{
        egl::context_factory::RenderContextFactory,
        femtovg::{main_window::FemtoVGWindow, renderable_window::RenderableWindow},
        slint_integration::platform::CustomSlintPlatform,
    },
};
use crate::{
    logger,
    wayland::{
        config::{LayerSurfaceConfig, ShellSurfaceConfig, WaylandSurfaceConfig},
        globals::context::GlobalContext,
        ops::WaylandSystemOps,
        outputs::{OutputManager, OutputManagerContext},
        rendering::RenderableSet,
        surfaces::{
            app_state::AppState,
            layer_surface::{SurfaceCtx, SurfaceSetupParams},
            surface_builder::{PlatformWrapper, SurfaceStateBuilder},
            surface_state::SurfaceState,
        },
    },
};
use core::result::Result as CoreResult;
use layer_shika_domain::errors::DomainError;
use layer_shika_domain::ports::shell::ShellSystemPort;
use layer_shika_domain::value_objects::handle::SurfaceHandle;
use layer_shika_domain::value_objects::output_handle::OutputHandle;
use layer_shika_domain::value_objects::output_info::OutputInfo;
use slint::{
    LogicalPosition, PhysicalSize, WindowPosition,
    platform::{femtovg_renderer::FemtoVGRenderer, set_platform, update_timers_and_animations},
};
use slint_interpreter::ComponentInstance;
use smithay_client_toolkit::reexports::calloop::{
    EventLoop, Interest, LoopHandle, Mode, PostAction, generic::Generic,
};
use std::cell::RefCell;
use std::io::ErrorKind;
use std::rc::Rc;
use std::time::Duration;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::{
    Connection, EventQueue, Proxy,
    backend::{ObjectId, WaylandError},
    protocol::wl_surface::WlSurface,
};

/// Interval between two frames while a Slint animation is running (~60 Hz).
const ANIMATION_FRAME: Duration = Duration::from_millis(16);

struct OutputSetup {
    output_id: ObjectId,
    main_surface_id: ObjectId,
    window: Rc<FemtoVGWindow>,
    builder: SurfaceStateBuilder,
    surface_handle: SurfaceHandle,
    shell_surface_name: String,
}

struct OutputManagerParams<'a> {
    config: &'a WaylandSurfaceConfig,
    global_ctx: &'a GlobalContext,
    connection: &'a Connection,
    layer_surface_config: LayerSurfaceConfig,
    render_factory: &'a Rc<RenderContextFactory>,
}

pub struct WaylandShellSystem {
    state: AppState,
    connection: Rc<Connection>,
    event_queue: EventQueue<AppState>,
    event_loop: EventLoop<'static, AppState>,
}

/// Fetch the events of the bound `wl_outputs` so that `AppState` knows name/geometry.
fn prefetch_output_metadata(
    event_queue: &mut EventQueue<AppState>,
    app_state: &mut AppState,
) -> Result<()> {
    for _ in 0..2 {
        event_queue
            .roundtrip(app_state)
            .map_err(|e| LayerShikaError::InvalidInput {
                message: format!("wl_output roundtrip failed: {e}"),
            })?;
    }
    Ok(())
}

/// Info for the policy: filled from `AppState`, otherwise the old behaviour.
fn policy_info(app_state: &AppState, output: &WlOutput, index: usize) -> OutputInfo {
    app_state
        .get_handle_by_output_id(&output.id())
        .and_then(|h| app_state.get_output_info(h))
        .cloned()
        .unwrap_or_else(|| {
            let mut info = OutputInfo::new(OutputHandle::new());
            info.set_primary(index == 0);
            info
        })
}

impl WaylandShellSystem {
    pub fn new(config: &WaylandSurfaceConfig) -> Result<Self> {
        logger::info!("Initializing WindowingSystem");
        let (connection, mut event_queue) = Self::init_wayland_connection()?;
        let event_loop =
            EventLoop::try_new().map_err(|e| EventLoopError::Creation { source: e })?;

        let state = Self::init_state(config, &connection, &mut event_queue)?;

        Ok(Self {
            state,
            connection,
            event_queue,
            event_loop,
        })
    }

    pub fn new_multi(configs: &[ShellSurfaceConfig]) -> Result<Self> {
        if configs.is_empty() {
            return Self::new_minimal();
        }

        logger::info!(
            "Initializing WindowingSystem with {} surface configs",
            configs.len()
        );
        let (connection, mut event_queue) = Self::init_wayland_connection()?;
        let event_loop =
            EventLoop::try_new().map_err(|e| EventLoopError::Creation { source: e })?;

        let state = Self::init_state_multi(configs, &connection, &mut event_queue)?;

        Ok(Self {
            state,
            connection,
            event_queue,
            event_loop,
        })
    }

    pub fn new_minimal() -> Result<Self> {
        logger::info!("Initializing WindowingSystem in minimal mode (no layer surfaces)");
        let (connection, mut event_queue) = Self::init_wayland_connection()?;
        let event_loop =
            EventLoop::try_new().map_err(|e| EventLoopError::Creation { source: e })?;

        let state = Self::init_state_minimal(&connection, &mut event_queue)?;

        Ok(Self {
            state,
            connection,
            event_queue,
            event_loop,
        })
    }

    fn init_wayland_connection() -> Result<(Rc<Connection>, EventQueue<AppState>)> {
        let connection = Rc::new(Connection::connect_to_env()?);
        let event_queue = connection.new_event_queue();
        Ok((connection, event_queue))
    }

    fn create_layer_surface_config(config: &WaylandSurfaceConfig) -> LayerSurfaceConfig {
        LayerSurfaceConfig {
            anchor: config.anchor,
            margin: config.margin,
            exclusive_zone: config.exclusive_zone,
            keyboard_interactivity: config.keyboard_interactivity,
            height: config.height,
            width: config.width,
        }
    }

    fn create_output_setups(
        config: &WaylandSurfaceConfig,
        global_ctx: &GlobalContext,
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
        layer_surface_config: &LayerSurfaceConfig,
        app_state: &AppState,
    ) -> Result<Vec<OutputSetup>> {
        let layer_shell =
            global_ctx
                .layer_shell
                .as_ref()
                .ok_or_else(|| LayerShikaError::InvalidInput {
                    message:
                        "wlr-layer-shell protocol not available - cannot create layer surfaces"
                            .into(),
                })?;

        let mut setups = Vec::new();

        for (index, output) in global_ctx.outputs.iter().enumerate() {
            let info = policy_info(app_state, output, index);
            if !config.output_policy.should_render(&info) {
                logger::info!("Skipping output {} due to output policy", index);
                continue;
            }

            let output_id = output.id();

            let setup_params = SurfaceSetupParams {
                compositor: &global_ctx.compositor,
                output,
                layer_shell,
                fractional_scale_manager: global_ctx.fractional_scale_manager.as_ref(),
                viewporter: global_ctx.viewporter.as_ref(),
                queue_handle: &event_queue.handle(),
                layer: config.layer,
                namespace: config.namespace.clone(),
            };

            let surface_ctx = SurfaceCtx::setup(&setup_params, layer_surface_config);
            let main_surface_id = surface_ctx.surface.id();

            let render_factory =
                RenderContextFactory::new(Rc::clone(&global_ctx.render_context_manager));

            let window = Self::initialize_renderer(&surface_ctx.surface, config, &render_factory)?;

            let mut builder = SurfaceStateBuilder::new()
                .with_component_definition(config.component_definition.clone())
                .with_compilation_result(config.compilation_result.clone())
                .with_surface(Rc::clone(&surface_ctx.surface))
                .with_layer_surface(Rc::clone(&surface_ctx.layer_surface))
                .with_scale_factor(config.scale_factor)
                .with_height(config.height)
                .with_width(config.width)
                .with_exclusive_zone(config.exclusive_zone)
                .with_connection(Rc::new(connection.clone()))
                .with_window(Rc::clone(&window));

            if let Some(fs) = &surface_ctx.fractional_scale {
                builder = builder.with_fractional_scale(Rc::clone(fs));
            }

            if let Some(vp) = &surface_ctx.viewport {
                builder = builder.with_viewport(Rc::clone(vp));
            }

            setups.push(OutputSetup {
                output_id,
                main_surface_id,
                window,
                builder,
                surface_handle: config.surface_handle,
                shell_surface_name: config.surface_name.clone(),
            });
        }

        Ok(setups)
    }

    fn setup_platform(setups: &[OutputSetup]) -> Result<()> {
        let first_setup = setups
            .first()
            .ok_or_else(|| LayerShikaError::InvalidInput {
                message: "No outputs available".into(),
            })?;

        let platform = CustomSlintPlatform::new(&first_setup.window);

        for setup in setups.iter().skip(1) {
            platform.add_window(Rc::clone(&setup.window));
        }

        set_platform(Box::new(PlatformWrapper(platform)))
            .map_err(|e| LayerShikaError::PlatformSetup { source: e })?;

        Ok(())
    }

    fn create_window_states(setups: Vec<OutputSetup>, app_state: &mut AppState) -> Result<()> {
        for setup in setups {
            let per_output_surface = SurfaceState::new(setup.builder).map_err(|e| {
                LayerShikaError::WindowConfiguration {
                    message: e.to_string(),
                }
            })?;

            app_state.add_shell_surface(
                &setup.output_id,
                setup.surface_handle,
                &setup.shell_surface_name,
                setup.main_surface_id,
                per_output_surface,
            );
        }

        Ok(())
    }

    fn init_state(
        config: &WaylandSurfaceConfig,
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
    ) -> Result<AppState> {
        let global_ctx = Rc::new(GlobalContext::initialize(
            connection,
            &event_queue.handle(),
        )?);
        let layer_surface_config = Self::create_layer_surface_config(config);

        // Pointer and keyboard are only created once the seat reports them (see Dispatch<WlSeat>).
        let mut app_state = AppState::new();

        app_state.set_queue_handle(event_queue.handle());
        // Before output selection, otherwise the OutputPolicy sees no names.
        prefetch_output_metadata(event_queue, &mut app_state)?;

        let render_factory =
            RenderContextFactory::new(Rc::clone(&global_ctx.render_context_manager));

        let setups = Self::create_output_setups(
            config,
            global_ctx.as_ref(),
            connection,
            event_queue,
            &layer_surface_config,
            &app_state,
        )?;

        Self::setup_platform(&setups)?;
        Self::create_window_states(setups, &mut app_state)?;

        let output_manager = Self::create_output_manager(&OutputManagerParams {
            config,
            global_ctx: global_ctx.as_ref(),
            connection,
            layer_surface_config,
            render_factory: &render_factory,
        });

        app_state.set_output_manager(Rc::new(RefCell::new(output_manager)));
        app_state.set_global_context(Rc::clone(&global_ctx));

        Ok(app_state)
    }

    fn init_state_multi(
        configs: &[ShellSurfaceConfig],
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
    ) -> Result<AppState> {
        let global_ctx = Rc::new(GlobalContext::initialize(
            connection,
            &event_queue.handle(),
        )?);

        // Pointer and keyboard are only created once the seat reports them (see Dispatch<WlSeat>).
        let mut app_state = AppState::new();

        app_state.set_queue_handle(event_queue.handle());
        // Before output selection, otherwise the OutputPolicy sees no names.
        prefetch_output_metadata(event_queue, &mut app_state)?;

        let render_factory =
            RenderContextFactory::new(Rc::clone(&global_ctx.render_context_manager));

        let setups = Self::create_output_setups_multi(
            configs,
            global_ctx.as_ref(),
            connection,
            event_queue,
            &app_state,
        )?;

        Self::setup_platform(&setups)?;
        Self::create_window_states(setups, &mut app_state)?;

        let primary_config = configs.first().map(|c| &c.config);
        if let Some(config) = primary_config {
            let layer_surface_config = Self::create_layer_surface_config(config);
            let output_manager = Self::create_output_manager(&OutputManagerParams {
                config,
                global_ctx: global_ctx.as_ref(),
                connection,
                layer_surface_config,
                render_factory: &render_factory,
            });

            app_state.set_output_manager(Rc::new(RefCell::new(output_manager)));
        }

        app_state.set_global_context(Rc::clone(&global_ctx));

        Ok(app_state)
    }

    fn init_state_minimal(
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
    ) -> Result<AppState> {
        let global_ctx = Rc::new(GlobalContext::initialize(
            connection,
            &event_queue.handle(),
        )?);

        // Pointer and keyboard are only created once the seat reports them (see Dispatch<WlSeat>).
        let mut app_state = AppState::new();

        app_state.set_queue_handle(event_queue.handle());
        app_state.set_global_context(Rc::clone(&global_ctx));

        let platform = CustomSlintPlatform::new_empty();
        set_platform(Box::new(PlatformWrapper(platform)))
            .map_err(|e| LayerShikaError::PlatformSetup { source: e })?;

        logger::info!(
            "Minimal state initialized successfully (no layer surfaces, empty Slint platform)"
        );

        Ok(app_state)
    }

    fn create_output_setups_multi(
        configs: &[ShellSurfaceConfig],
        global_ctx: &GlobalContext,
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
        app_state: &AppState,
    ) -> Result<Vec<OutputSetup>> {
        let layer_shell =
            global_ctx
                .layer_shell
                .as_ref()
                .ok_or_else(|| LayerShikaError::InvalidInput {
                    message:
                        "wlr-layer-shell protocol not available - cannot create layer surfaces"
                            .into(),
                })?;

        let mut setups = Vec::new();

        for (output_index, output) in global_ctx.outputs.iter().enumerate() {
            let info = policy_info(app_state, output, output_index);
            let output_id = output.id();

            for shell_config in configs {
                let config = &shell_config.config;

                if !config.output_policy.should_render(&info) {
                    logger::info!(
                        "Skipping shell surface '{}' on output {} due to output policy",
                        shell_config.name,
                        output_index
                    );
                    continue;
                }

                let layer_surface_config = Self::create_layer_surface_config(config);

                let setup_params = SurfaceSetupParams {
                    compositor: &global_ctx.compositor,
                    output,
                    layer_shell,
                    fractional_scale_manager: global_ctx.fractional_scale_manager.as_ref(),
                    viewporter: global_ctx.viewporter.as_ref(),
                    queue_handle: &event_queue.handle(),
                    layer: config.layer,
                    namespace: config.namespace.clone(),
                };

                let surface_ctx = SurfaceCtx::setup(&setup_params, &layer_surface_config);
                let main_surface_id = surface_ctx.surface.id();

                let render_factory =
                    RenderContextFactory::new(Rc::clone(&global_ctx.render_context_manager));

                let window =
                    Self::initialize_renderer(&surface_ctx.surface, config, &render_factory)?;

                let mut builder = SurfaceStateBuilder::new()
                    .with_component_definition(config.component_definition.clone())
                    .with_compilation_result(config.compilation_result.clone())
                    .with_surface(Rc::clone(&surface_ctx.surface))
                    .with_layer_surface(Rc::clone(&surface_ctx.layer_surface))
                    .with_scale_factor(config.scale_factor)
                    .with_height(config.height)
                    .with_width(config.width)
                    .with_exclusive_zone(config.exclusive_zone)
                    .with_connection(Rc::new(connection.clone()))
                    .with_window(Rc::clone(&window));

                if let Some(fs) = &surface_ctx.fractional_scale {
                    builder = builder.with_fractional_scale(Rc::clone(fs));
                }

                if let Some(vp) = &surface_ctx.viewport {
                    builder = builder.with_viewport(Rc::clone(vp));
                }

                logger::info!(
                    "Created setup for shell surface '{}' on output {}",
                    shell_config.name,
                    output_index
                );

                setups.push(OutputSetup {
                    output_id: output_id.clone(),
                    main_surface_id,
                    window,
                    builder,
                    surface_handle: shell_config.config.surface_handle,
                    shell_surface_name: shell_config.name.clone(),
                });
            }
        }

        Ok(setups)
    }

    fn create_output_manager(params: &OutputManagerParams<'_>) -> OutputManager {
        let manager_context = OutputManagerContext {
            compositor: params.global_ctx.compositor.clone(),
            layer_shell: params.global_ctx.layer_shell.clone(),
            fractional_scale_manager: params.global_ctx.fractional_scale_manager.clone(),
            viewporter: params.global_ctx.viewporter.clone(),
            render_factory: Rc::clone(params.render_factory),
            connection: Rc::new(params.connection.clone()),
        };

        OutputManager::new(
            manager_context,
            params.config.clone(),
            params.layer_surface_config,
        )
    }

    pub(crate) fn initialize_renderer(
        surface: &Rc<WlSurface>,
        config: &WaylandSurfaceConfig,
        render_factory: &Rc<RenderContextFactory>,
    ) -> Result<Rc<FemtoVGWindow>> {
        let init_size = PhysicalSize::new(1, 1);

        let context = render_factory.create_context(&surface.id(), init_size)?;

        let renderer = FemtoVGRenderer::new(context)
            .map_err(|e| LayerShikaError::FemtoVGRendererCreation { source: e })?;

        let femtovg_window = FemtoVGWindow::new(renderer);
        femtovg_window.set_size(slint::WindowSize::Physical(init_size));
        femtovg_window.set_scale_factor(config.scale_factor);
        femtovg_window.set_position(WindowPosition::Logical(LogicalPosition::new(0., 0.)));

        Ok(femtovg_window)
    }

    pub fn event_loop_handle(&self) -> LoopHandle<'static, AppState> {
        self.event_loop.handle()
    }

    pub fn run(&mut self) -> Result<()> {
        logger::info!("Starting WindowingSystem main loop");

        logger::info!("Processing initial Wayland configuration events");
        // first roundtrip to receive initial output, globals, and surface configure events
        // second roundtrip handles any cascading configure events like fractional scaling and layer surface configures
        for i in 0..2 {
            let dispatched = self.event_queue.roundtrip(&mut self.state)?;
            logger::info!("Roundtrip {} dispatched {} events", i + 1, dispatched);

            self.connection
                .flush()
                .map_err(|e| LayerShikaError::WaylandProtocol { source: e })?;

            update_timers_and_animations();

            self.state.render_all_dirty()?;
        }

        logger::info!("Initial configuration complete, requesting final render");
        for surface in self.state.all_outputs() {
            RenderableWindow::request_redraw(surface.window().as_ref());
        }
        update_timers_and_animations();
        self.state.render_all_dirty()?;
        self.connection
            .flush()
            .map_err(|e| LayerShikaError::WaylandProtocol { source: e })?;

        self.setup_wayland_event_source()?;

        // Own loop instead of `run(None, ..)`: while Slint is animating, the loop must
        // not sleep (see slint::platform), otherwise an animation stalls after the
        // first frame. It then wakes up at roughly the frame rate, otherwise only on events.
        loop {
            let animating = self.animations_running();
            let timeout = animating.then_some(ANIMATION_FRAME);
            self.event_loop
                .dispatch(timeout, &mut self.state)
                .map_err(|e| EventLoopError::Execution { source: e })?;
            if animating {
                // A running animation does not mark the window as dirty by itself;
                // without this only the end result would be drawn, not the frames in between.
                for surface in self.state.all_outputs() {
                    RenderableWindow::request_redraw(surface.window().as_ref());
                }
            }
            if let Err(e) =
                Self::process_events(&self.connection, &mut self.event_queue, &mut self.state)
            {
                logger::error!("Error processing events: {e}");
            }
        }
    }

    fn animations_running(&self) -> bool {
        // The animation driver is global, any window will do.
        self.state
            .all_outputs()
            .next()
            .is_some_and(|surface| surface.window().has_active_animations())
    }

    fn setup_wayland_event_source(&self) -> Result<()> {
        let connection = Rc::clone(&self.connection);

        self.event_loop
            .handle()
            .insert_source(
                Generic::new(connection, Interest::READ, Mode::Level),
                move |_, _connection, _shared_data| Ok(PostAction::Continue),
            )
            .map_err(|e| EventLoopError::InsertSource {
                message: format!("{e:?}"),
            })?;

        Ok(())
    }

    fn process_events(
        connection: &Connection,
        event_queue: &mut EventQueue<AppState>,
        shared_data: &mut AppState,
    ) -> Result<()> {
        if let Some(guard) = event_queue.prepare_read() {
            // WouldBlock: the loop was woken by another source (timer, channel),
            // not by Wayland. Not an error, otherwise nothing gets rendered below.
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(LayerShikaError::WaylandProtocol { source: e }),
            }
        }

        // Advance the clock before the handlers so animations started there begin now.
        update_timers_and_animations();

        event_queue.dispatch_pending(shared_data)?;

        for surface in shared_data.all_outputs() {
            surface
                .window()
                .render_frame_if_dirty()
                .map_err(|e| RenderingError::Operation {
                    message: e.to_string(),
                })?;
        }

        connection
            .flush()
            .map_err(|e| LayerShikaError::WaylandProtocol { source: e })?;

        Ok(())
    }

    pub fn component_instance(&self) -> Result<&ComponentInstance> {
        self.state
            .primary_output()
            .ok_or_else(|| LayerShikaError::InvalidInput {
                message: "No outputs available".into(),
            })
            .map(SurfaceState::component_instance)
    }

    pub fn state(&self) -> Result<&SurfaceState> {
        self.state
            .primary_output()
            .ok_or_else(|| LayerShikaError::InvalidInput {
                message: "No outputs available".into(),
            })
    }

    pub fn app_state(&self) -> &AppState {
        &self.state
    }

    pub fn app_state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn spawn_surface(&mut self, config: &ShellSurfaceConfig) -> Result<Vec<OutputHandle>> {
        logger::info!("Spawning new surface '{}'", config.name);

        let mut handles = Vec::new();

        for (output_handle, _surface) in self.state.outputs_with_handles() {
            handles.push(output_handle);
        }

        logger::info!(
            "Surface '{}' would spawn on {} outputs (dynamic spawning not yet fully implemented)",
            config.name,
            handles.len()
        );

        Ok(handles)
    }

    pub fn despawn_surface(&mut self, surface_name: &str) -> Result<()> {
        logger::info!("Despawning surface '{}'", surface_name);

        let removed = self.state.remove_surfaces_by_name(surface_name);

        logger::info!(
            "Removed {} surface instances for '{}'",
            removed.len(),
            surface_name
        );

        Ok(())
    }
}

impl ShellSystemPort for WaylandShellSystem {
    fn run(&mut self) -> CoreResult<(), DomainError> {
        WaylandShellSystem::run(self).map_err(|e| DomainError::Adapter {
            source: Box::new(e),
        })
    }
}

impl WaylandSystemOps for WaylandShellSystem {
    fn run(&mut self) -> Result<()> {
        WaylandShellSystem::run(self)
    }

    fn spawn_surface(&mut self, config: &ShellSurfaceConfig) -> Result<Vec<OutputHandle>> {
        WaylandShellSystem::spawn_surface(self, config)
    }

    fn despawn_surface(&mut self, name: &str) -> Result<()> {
        WaylandShellSystem::despawn_surface(self, name)
    }

    fn app_state(&self) -> &AppState {
        WaylandShellSystem::app_state(self)
    }

    fn app_state_mut(&mut self) -> &mut AppState {
        WaylandShellSystem::app_state_mut(self)
    }

    fn event_loop_handle(&self) -> LoopHandle<'static, AppState> {
        WaylandShellSystem::event_loop_handle(self)
    }

    fn component_instance(&self) -> Result<&ComponentInstance> {
        WaylandShellSystem::component_instance(self)
    }
}
