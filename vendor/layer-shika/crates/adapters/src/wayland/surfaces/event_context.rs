use std::rc::Rc;

use slint::LogicalPosition;
use slint::platform::{WindowAdapter, WindowEvent};
use wayland_client::Proxy;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_pointer;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::rendering::femtovg::main_window::FemtoVGWindow;
use crate::wayland::input::PointerInputState;
use crate::wayland::surfaces::display_metrics::SharedDisplayMetrics;

pub struct EventContext {
    main_window: Rc<FemtoVGWindow>,
    main_surface_id: ObjectId,
    display_metrics: SharedDisplayMetrics,
    pointer_state: PointerInputState,
    last_pointer_serial: u32,
    main_surface_entered: bool,
    axis_source: Option<wl_pointer::AxisSource>,
}

impl EventContext {
    #[must_use]
    pub fn new(
        main_window: Rc<FemtoVGWindow>,
        main_surface_id: ObjectId,
        display_metrics: SharedDisplayMetrics,
    ) -> Self {
        Self {
            main_window,
            main_surface_id,
            display_metrics,
            pointer_state: PointerInputState::new(),
            last_pointer_serial: 0,
            main_surface_entered: false,
            axis_source: None,
        }
    }

    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        self.display_metrics.borrow().scale_factor()
    }

    pub fn update_scale_factor(&mut self, scale_120ths: u32) -> f32 {
        self.display_metrics
            .borrow_mut()
            .update_scale_factor(scale_120ths)
    }

    pub const fn current_pointer_position(&self) -> LogicalPosition {
        self.pointer_state.current_position()
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn set_current_pointer_position(&mut self, physical_x: f64, physical_y: f64) {
        let has_fractional_scale = self.display_metrics.borrow().has_fractional_scale();
        let scale_factor = self.display_metrics.borrow().scale_factor();

        let logical_position = if has_fractional_scale {
            LogicalPosition::new(physical_x as f32, physical_y as f32)
        } else {
            LogicalPosition::new(
                (physical_x / f64::from(scale_factor)) as f32,
                (physical_y / f64::from(scale_factor)) as f32,
            )
        };
        self.pointer_state.set_current_position(logical_position);
    }

    pub const fn last_pointer_serial(&self) -> u32 {
        self.last_pointer_serial
    }

    pub fn set_last_pointer_serial(&mut self, serial: u32) {
        self.last_pointer_serial = serial;
    }

    pub fn set_entered_surface(&mut self, surface: &WlSurface) {
        self.main_surface_entered = self.main_surface_id == surface.id();
    }

    pub fn clear_entered_surface(&mut self) {
        self.main_surface_entered = false;
    }

    pub fn dispatch_to_active_window(&self, event: WindowEvent) {
        if self.main_surface_entered {
            self.main_window.window().dispatch_event(event);
        }
    }

    pub fn dispatch_to_surface(&self, surface_id: &ObjectId, event: WindowEvent) {
        if self.main_surface_id == *surface_id {
            self.main_window.window().dispatch_event(event);
        }
    }

    pub fn set_axis_source(&mut self, axis_source: wl_pointer::AxisSource) {
        self.axis_source = Some(axis_source);
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn accumulate_axis(&mut self, axis: wl_pointer::Axis, value: f64) {
        let delta = value as f32;
        match axis {
            wl_pointer::Axis::HorizontalScroll => {
                self.pointer_state.accumulate_axis_value(delta, 0.0);
            }
            wl_pointer::Axis::VerticalScroll => {
                self.pointer_state.accumulate_axis_value(0.0, delta);
            }
            _ => {}
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn accumulate_axis_discrete(&mut self, axis: wl_pointer::Axis, discrete: i32) {
        let delta = discrete as f32 * 60.0;

        match axis {
            wl_pointer::Axis::HorizontalScroll => {
                self.pointer_state.accumulate_axis_value(delta, 0.0);
            }
            wl_pointer::Axis::VerticalScroll => {
                self.pointer_state.accumulate_axis_value(0.0, delta);
            }
            _ => {}
        }
    }

    pub fn take_accumulated_axis(&mut self) -> (f32, f32) {
        let (delta_x, delta_y) = self.pointer_state.take_accumulated_axis();
        self.axis_source = None;
        (delta_x, delta_y)
    }
}
