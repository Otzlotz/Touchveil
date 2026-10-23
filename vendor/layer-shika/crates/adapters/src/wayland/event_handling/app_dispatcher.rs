use crate::logger;
use crate::wayland::surfaces::app_state::AppState;
use crate::wayland::surfaces::display_metrics::DisplayMetrics;
use layer_shika_domain::value_objects::output_info::OutputGeometry;
use smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use std::os::fd::AsFd;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::GlobalListContents,
    protocol::{
        wl_compositor::WlCompositor,
        wl_keyboard::{self, WlKeyboard},
        wl_output::{self, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_region::WlRegion,
        wl_registry::{Event, WlRegistry},
        wl_seat::{self, WlSeat},
        wl_surface::WlSurface,
    },
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

impl Dispatch<ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                logger::info!(
                    "Layer surface configured with compositor size: {}x{}",
                    width,
                    height
                );
                let layer_surface_id = layer_surface.id();
                let Some(surface) = state.get_output_by_layer_surface_mut(&layer_surface_id) else {
                    logger::info!(
                        "Could not find window for layer surface {:?}",
                        layer_surface_id
                    );
                    return;
                };

                surface.handle_layer_surface_configure(layer_surface, serial, width, height);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                let layer_surface_id = layer_surface.id();
                if let Some(surface) = state.get_output_by_layer_surface_mut(&layer_surface_id) {
                    surface.handle_layer_surface_closed();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for AppState {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        let output_id = proxy.id();

        let handle = if state.output_manager().is_none() {
            Some(state.ensure_output_registered(&output_id))
        } else {
            state.get_handle_by_output_id(&output_id)
        };

        match event {
            wl_output::Event::Mode {
                flags: WEnum::Value(mode_flags),
                width,
                height,
                ..
            } => {
                let is_current = mode_flags.contains(wl_output::Mode::Current);
                let is_preferred = mode_flags.contains(wl_output::Mode::Preferred);
                logger::info!(
                    "WlOutput mode: {}x{} (current: {}, preferred: {})",
                    width,
                    height,
                    is_current,
                    is_preferred
                );
                if is_current {
                    state.set_output_mode(&output_id, width, height);
                    for surface in state.all_surfaces_for_output_mut(&output_id) {
                        surface.handle_output_mode(width, height);
                    }
                }
            }
            wl_output::Event::Mode { .. } => {
                logger::debug!("WlOutput mode event with unknown flags value");
            }
            wl_output::Event::Description { ref description } => {
                logger::info!("WlOutput description: {description:?}");
                if let Some(handle) = handle {
                    if let Some(info) = state.get_output_info_mut(handle) {
                        info.set_description(description.clone());
                    }
                }
            }
            wl_output::Event::Scale { ref factor } => {
                logger::info!("WlOutput factor scale: {factor:?}");
                if let Some(handle) = handle {
                    if let Some(info) = state.get_output_info_mut(handle) {
                        info.set_scale(*factor);
                    }
                }
            }
            wl_output::Event::Name { ref name } => {
                logger::info!("WlOutput name: {name:?}");
                if let Some(handle) = handle {
                    if let Some(info) = state.get_output_info_mut(handle) {
                        info.set_name(name.clone());
                    }
                }
            }
            wl_output::Event::Geometry {
                x,
                y,
                physical_width,
                physical_height,
                subpixel,
                make,
                model,
                transform,
            } => {
                logger::info!(
                    "WlOutput geometry: x={x}, y={y}, physical_width={physical_width}, physical_height={physical_height}, subpixel={subpixel:?}, make={make:?}, model={model:?}, transform={transform:?}"
                );
                if let Some(handle) = handle {
                    if let Some(info) = state.get_output_info_mut(handle) {
                        let mut geometry =
                            OutputGeometry::new(x, y, physical_width, physical_height);
                        if !make.is_empty() {
                            geometry = geometry.with_make(make);
                        }
                        if !model.is_empty() {
                            geometry = geometry.with_model(model);
                        }
                        info.set_geometry(geometry);
                    }
                }
            }
            wl_output::Event::Done => {
                logger::info!("WlOutput done for output {:?}", output_id);

                if let Some(manager) = state.output_manager() {
                    let manager_ref = manager.borrow();
                    if manager_ref.has_pending_output(&output_id) {
                        drop(manager_ref);

                        logger::info!(
                            "Output {:?} configuration complete, finalizing...",
                            output_id
                        );

                        let manager_ref = manager.borrow();
                        if let Err(e) = manager_ref.finalize_output(&output_id, state, qhandle) {
                            logger::info!("Failed to finalize output {:?}: {e}", output_id);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn handle_pointer_enter_event(
    state: &mut AppState,
    serial: u32,
    surface: &WlSurface,
    surface_x: f64,
    surface_y: f64,
) {
    let surface_id = surface.id();

    if let Some(key) = state.get_key_by_surface(&surface_id).cloned() {
        if let Some(layer_surface) = state.get_surface_by_key_mut(&key) {
            layer_surface.handle_pointer_enter(serial, surface, surface_x, surface_y);
        }
        state.set_active_surface_key(Some(key));
    }
}

fn handle_pointer_motion_event(state: &mut AppState, surface_x: f64, surface_y: f64) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_pointer_motion(surface_x, surface_y);
    }
}

fn handle_pointer_leave_event(state: &mut AppState) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_pointer_leave();
    }
    state.set_active_surface_key(None);
}

fn handle_pointer_button_event(
    state: &mut AppState,
    serial: u32,
    button: u32,
    button_state: WEnum<wl_pointer::ButtonState>,
) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_pointer_button(serial, button, button_state);
    }
}

fn handle_pointer_axis_source_event(state: &mut AppState, axis_source: wl_pointer::AxisSource) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_axis_source(axis_source);
    }
}

fn handle_pointer_axis_event(state: &mut AppState, time: u32, axis: wl_pointer::Axis, value: f64) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_axis(time, axis, value);
    }
}

fn handle_pointer_axis_discrete_event(state: &mut AppState, axis: wl_pointer::Axis, discrete: i32) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_axis_discrete(axis, discrete);
    }
}

fn handle_pointer_axis_stop_event(state: &mut AppState, time: u32, axis: wl_pointer::Axis) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_axis_stop(time, axis);
    }
}

fn handle_pointer_frame_event(state: &mut AppState) {
    if let Some(surface) = state.active_surface_mut() {
        surface.handle_pointer_frame();
    }
}

impl Dispatch<WlPointer, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &WlPointer,
        event: <WlPointer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => handle_pointer_enter_event(state, serial, &surface, surface_x, surface_y),
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => handle_pointer_motion_event(state, surface_x, surface_y),
            wl_pointer::Event::Leave { .. } => handle_pointer_leave_event(state),
            wl_pointer::Event::Button {
                serial,
                button,
                state: button_state,
                ..
            } => handle_pointer_button_event(state, serial, button, button_state),
            wl_pointer::Event::AxisSource {
                axis_source: WEnum::Value(axis_source),
            } => handle_pointer_axis_source_event(state, axis_source),
            wl_pointer::Event::Axis {
                time,
                axis: WEnum::Value(axis),
                value,
            } => handle_pointer_axis_event(state, time, axis, value),
            wl_pointer::Event::AxisDiscrete {
                axis: WEnum::Value(axis),
                discrete,
            } => handle_pointer_axis_discrete_event(state, axis, discrete),
            wl_pointer::Event::AxisStop {
                time,
                axis: WEnum::Value(axis),
            } => handle_pointer_axis_stop_event(state, time, axis),
            wl_pointer::Event::Frame => handle_pointer_frame_event(state),
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &WlKeyboard,
        event: <WlKeyboard as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                state.handle_keymap(fd.as_fd(), size);
            }
            wl_keyboard::Event::Enter {
                serial,
                surface,
                keys,
            } => {
                state.handle_keyboard_enter(serial, &surface, &keys);
            }
            wl_keyboard::Event::Leave { serial, surface } => {
                state.handle_keyboard_leave(serial, &surface);
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                key,
                state: WEnum::Value(key_state),
            } => {
                state.handle_key(serial, time, key, key_state);
            }
            wl_keyboard::Event::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                state.handle_modifiers(serial, mods_depressed, mods_latched, mods_locked, group);
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                state.handle_repeat_info(rate, delay);
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let scale_float = DisplayMetrics::scale_factor_from_120ths(scale);
            logger::info!("Fractional scale received: {scale_float} ({scale}x)");

            for surface in state.all_outputs_mut() {
                surface.handle_fractional_scale(proxy, scale);
            }
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for AppState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == "wl_output" {
                    logger::info!(
                        "Hot-plugged output detected! Binding wl_output with name {name}, version {version}"
                    );

                    let output = registry.bind::<WlOutput, _, _>(name, 4.min(version), qhandle, ());
                    let output_id = output.id();

                    if let Some(manager) = state.output_manager() {
                        let mut manager_ref = manager.borrow_mut();
                        let handle = manager_ref.register_output(output, qhandle);
                        logger::info!("Registered hot-plugged output with handle {handle:?}");

                        state.register_registry_name(name, output_id);
                    } else {
                        logger::info!("No output manager available yet (startup initialization)");
                    }
                }
            }
            Event::GlobalRemove { name } => {
                logger::info!("Registry global removed: name {name}");

                if let Some(output_id) = state.unregister_registry_name(name) {
                    logger::info!("Output with registry name {name} removed, cleaning up...");

                    if let Some(manager) = state.output_manager() {
                        let mut manager_ref = manager.borrow_mut();
                        manager_ref.remove_output(&output_id, state);
                    }
                }
            }
            _ => {}
        }
    }
}

macro_rules! impl_empty_dispatch_app {
    ($(($t:ty, $u:ty)),+) => {
        $(
            impl Dispatch<$t, $u> for AppState {
                fn event(
                    _state: &mut Self,
                    _proxy: &$t,
                    _event: <$t as wayland_client::Proxy>::Event,
                    _data: &$u,
                    _conn: &Connection,
                    _qhandle: &QueueHandle<Self>,
                ) {
                  logger::debug!("Implement empty dispatch event for {:?}", stringify!($t));
                }
            }
        )+
    };
}

impl_empty_dispatch_app!(
    (WlCompositor, ()),
    (WlSurface, ()),
    (WlRegion, ()),
    (ZwlrLayerShellV1, ()),
    (WpFractionalScaleManagerV1, ()),
    (WpViewporter, ()),
    (WpViewport, ())
);

impl Dispatch<WlSeat, ()> for AppState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: <WlSeat as Proxy>::Event,
        (): &(),
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            state.set_seat_capabilities(seat, caps, qhandle, conn);
        }
    }
}
