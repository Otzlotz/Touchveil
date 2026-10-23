//! Touch interaction: hit testing and what touching down, dragging and lifting do.

use super::{Overlay, Target, Touch};
use crate::config::Kind;
use crate::link::send_to_daemon;
use layer_shika::slint_interpreter::Value;
use touchveil_common::ToDaemon;

impl Overlay {
    /// Normalized coordinates from the daemon -> window coordinates in logical pixels.
    pub(super) fn to_window(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.size.0, y * self.size.1)
    }

    pub(super) fn target_at(&self, x: f32, y: f32) -> Target {
        if !self.layout.panel.contains(x, y) {
            return if self.cfg.overlay.tap_outside_closes {
                Target::Outside
            } else {
                Target::None
            };
        }
        let (px, py) = self.layout.to_panel(x, y);
        if self.layout.close.is_some_and(|c| c.contains(px, py)) {
            return Target::Close;
        }
        match self.layout.widget_at(px, py) {
            // Displays are not interactive; the panel can be pushed away from there.
            Some(i) if matches!(self.cfg.widget[i].kind, Kind::Display { .. }) => {
                Target::Panel { start: (x, y) }
            }
            Some(i) if self.states[i].pressed_by.is_none() => Target::Widget(i),
            Some(_) => Target::None,
            None => Target::Panel { start: (x, y) },
        }
    }

    pub(super) fn touch_down(&mut self, slot: u32, x: f32, y: f32) {
        let target = self.target_at(x, y);
        match target {
            Target::Widget(i) => {
                self.states[i].pressed_by = Some(slot);
                if matches!(self.cfg.widget[i].kind, Kind::Slider { .. }) {
                    self.slider_set(i, self.layout.to_panel(x, y).0);
                }
                self.sync(i);
            }
            Target::Close => self.set("close-pressed", Value::Bool(true)),
            _ => {}
        }
        self.touches.insert(slot, Touch { x, y, target });
    }

    pub(super) fn touch_move(&mut self, slot: u32, x: f32, y: f32) {
        let Some(t) = self.touches.get_mut(&slot) else {
            return;
        };
        (t.x, t.y) = (x, y);
        let target = t.target;
        let (px, py) = self.layout.to_panel(x, y);
        match target {
            Target::Widget(i) => match self.cfg.widget[i].kind {
                Kind::Slider { .. } => self.slider_set(i, px),
                Kind::Button { .. } | Kind::Toggle { .. } => {
                    let inside = self.layout.widgets[i].contains(px, py);
                    if inside != self.states[i].pressed_by.is_some() {
                        self.states[i].pressed_by = inside.then_some(slot);
                        self.sync(i);
                    }
                }
                Kind::Display { .. } => {}
            },
            Target::Close => {
                let inside = self.layout.close.is_some_and(|c| c.contains(px, py));
                self.set("close-pressed", Value::Bool(inside));
            }
            // Panel follows the finger back to the edge.
            Target::Panel { start } => {
                let out = self.layout.pushed_out(start, (x, y));
                self.follow(1.0 - out as f64);
            }
            Target::Outside | Target::None => {}
        }
    }

    pub(super) fn touch_up(&mut self, slot: u32) {
        let Some(t) = self.touches.remove(&slot) else {
            return;
        };
        let (px, py) = self.layout.to_panel(t.x, t.y);
        match t.target {
            Target::Widget(i) => {
                let inside = self.states[i].pressed_by == Some(slot);
                self.states[i].pressed_by = None;
                if inside {
                    self.activate(i);
                }
                self.sync(i);
            }
            Target::Close => {
                self.set("close-pressed", Value::Bool(false));
                if self.layout.close.is_some_and(|c| c.contains(px, py)) {
                    send_to_daemon(&self.writer, ToDaemon::Close);
                }
            }
            Target::Outside if !self.layout.panel.contains(t.x, t.y) => {
                send_to_daemon(&self.writer, ToDaemon::Close)
            }
            Target::Panel { start } => {
                if self.layout.pushed_out(start, (t.x, t.y)) >= self.cfg.overlay.swipe_close {
                    send_to_daemon(&self.writer, ToDaemon::Close);
                } else {
                    self.glide_to(1.0); // snap back
                }
            }
            Target::Outside | Target::None => {}
        }
    }

    pub(super) fn release_all(&mut self) {
        self.touches.clear();
        for i in 0..self.states.len() {
            if self.states[i].pressed_by.take().is_some() {
                self.sync(i);
            }
        }
        self.set("close-pressed", Value::Bool(false));
    }
}
