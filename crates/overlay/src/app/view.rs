//! Rendering: write theme, layout and widget state into the Slint properties.

use super::{Overlay, WState};
use crate::config::{Color, Config, Kind, Purpose};
use crate::exec;
use crate::layout::Layout;
use layer_shika::slint::{self, ComponentHandle, Model, ModelRc, VecModel};
use layer_shika::slint_interpreter::{Struct, Value};
use touchveil_common::Edge;

pub(super) fn color(c: Color) -> Value {
    let [r, g, b, a] = c.0;
    Value::from(slint::Color::from_argb_u8(a, r, g, b))
}

pub(super) fn num(v: f32) -> Value {
    Value::Number(v as f64)
}

impl Overlay {
    pub(super) fn set(&self, name: &str, v: Value) {
        if let Err(e) = self.ui.set_property(name, v) {
            eprintln!("Property {name}: {e:?}");
        }
    }

    /// Window size in logical pixels (that is what Slint uses too).
    pub(super) fn window_size(&self) -> (f32, f32) {
        let win = self.ui.window();
        let s = win.size();
        let k = win.scale_factor();
        (s.width as f32 / k, s.height as f32 / k)
    }

    // ---------- Config ----------

    pub(super) fn reload(&mut self) {
        match self.watched.poll() {
            None => {}
            Some(Ok(cfg)) => {
                self.error.clear();
                self.apply_config(cfg);
            }
            Some(Err(e)) => {
                eprintln!("Config: {e}");
                self.error = format!("Config error, previous settings stay active:\n{e}");
                self.set("error", Value::String(self.error.as_str().into()));
            }
        }
    }

    pub(super) fn apply_config(&mut self, cfg: Config) {
        self.cfg = cfg;
        self.gen += 1;
        self.states = self
            .cfg
            .widget
            .iter()
            .map(|w| match w.kind {
                Kind::Slider { min, .. } => WState {
                    value: min,
                    slider: Some(exec::Coalescer::new(w.timeout(Purpose::Slider))),
                    ..WState::default()
                },
                _ => WState::default(),
            })
            .collect();
        for i in 0..self.states.len() {
            if let Kind::Slider { .. } = self.cfg.widget[i].kind {
                self.states[i].text = self.slider_text(i);
            }
        }

        let t = &self.cfg.theme;
        let o = &self.cfg.overlay;
        for (name, v) in [
            ("panel-color", color(t.panel)),
            ("dim", num(t.dim)),
            ("text-color", color(t.text)),
            ("muted-color", color(t.muted)),
            ("widget-color", color(t.widget)),
            ("pressed-color", color(t.pressed)),
            ("accent-color", color(t.accent)),
            ("border-color", color(t.border)),
            ("border-w", num(t.border_width)),
            ("radius", num(t.radius)),
            ("font-size", num(t.font_size)),
            ("pad", num(t.padding)),
            ("anim", Value::Number(t.animation_ms as f64)),
            ("heading", Value::String(o.title.as_str().into())),
            ("has-close", Value::Bool(o.close_button)),
            ("show-touches", Value::Bool(o.show_touches)),
            ("error", Value::String(self.error.as_str().into())),
        ] {
            self.set(name, v);
        }

        self.model.set_vec(Vec::new());
        self.size = (0.0, 0.0);
        self.relayout();
    }

    /// Recompute the layout when the window size has changed.
    pub(super) fn relayout(&mut self) {
        let size = self.window_size();
        if size == self.size || size.0 <= 1.0 {
            return;
        }
        self.size = size;
        self.layout = Layout::compute(&self.cfg, size.0, size.1);
        let l = &self.layout;
        for (name, v) in [
            ("panel-x", l.panel.x),
            ("panel-y", l.panel.y),
            ("panel-w", l.panel.w),
            ("panel-h", l.panel.h),
            ("slide-x", l.slide.0),
            ("slide-y", l.slide.1),
        ] {
            self.set(name, num(v));
        }
        self.set("header-h", num(l.header_h));
        let c = l.close.unwrap_or_default();
        for (name, v) in [
            ("close-x", c.x),
            ("close-y", c.y),
            ("close-w", c.w),
            ("close-h", c.h),
        ] {
            self.set(name, num(v));
        }
        let rows: Vec<Value> = (0..self.states.len()).map(|i| self.row(i)).collect();
        self.model.set_vec(rows);
        self.hint_geometry();
    }

    /// Position of the edge hint: a strip as wide as the start zone of the gesture,
    /// for corners a glow around the corner.
    fn hint_geometry(&self) {
        let (w, h) = self.size;
        let strip = self.cfg.gesture.strip_width;
        let corner = |cx: f32, cy: f32| (4, strip * 3.0 * w.max(h), cx, cy);
        let (dir, thick, cx, cy) = match self.cfg.gesture.edge {
            Edge::Left => (0, strip * w, 0.0, 0.0),
            Edge::Right => (1, strip * w, 0.0, 0.0),
            Edge::Top => (2, strip * h, 0.0, 0.0),
            Edge::Bottom => (3, strip * h, 0.0, 0.0),
            Edge::TopLeft => corner(0.0, 0.0),
            Edge::TopRight => corner(w, 0.0),
            Edge::BottomLeft => corner(0.0, h),
            Edge::BottomRight => corner(w, h),
        };
        self.set("hint-dir", Value::Number(f64::from(dir)));
        for (name, v) in [("hint-thick", thick), ("hint-cx", cx), ("hint-cy", cy)] {
            self.set(name, num(v));
        }
    }

    /// Show the edge hint at the finger position (normalized) or hide it. Only while the
    /// overlay is closed; while dragging, Slint fades it out as the panel comes in.
    pub(super) fn set_hint(&mut self, at: Option<(f32, f32)>) {
        let at = at.filter(|_| self.cfg.overlay.edge_hint && !self.open);
        if let Some((x, y)) = at {
            self.relayout();
            let (x, y) = self.to_window(x, y);
            self.set("hint-x", num(x));
            self.set("hint-y", num(y));
        }
        self.set("hint-on", Value::Bool(at.is_some()));
    }

    /// Data of a widget for Slint.
    pub(super) fn row(&self, i: usize) -> Value {
        let w = &self.cfg.widget[i];
        let s = &self.states[i];
        let r = self.layout.widgets.get(i).copied().unwrap_or_default();
        let (kind, value) = match w.kind {
            Kind::Button { .. } => (0, 0.0),
            Kind::Toggle { .. } => (1, 0.0),
            Kind::Slider { min, max, .. } => (2, ((s.value - min) / (max - min)).clamp(0.0, 1.0)),
            Kind::Display { .. } => (3, 0.0),
        };
        // Waiting for the second tap (`confirm`): say what to do.
        let label = if s.armed.is_some() {
            "Tap again"
        } else {
            w.label.as_str()
        };
        Value::Struct(Struct::from_iter([
            ("kind".to_string(), Value::Number(kind as f64)),
            ("x".into(), num(r.x)),
            ("y".into(), num(r.y)),
            ("w".into(), num(r.w)),
            ("h".into(), num(r.h)),
            ("label".into(), Value::String(label.into())),
            ("text".into(), Value::String(s.text.as_str().into())),
            ("value".into(), Value::Number(value)),
            ("on".into(), Value::Bool(s.on)),
            ("pressed".into(), Value::Bool(s.pressed_by.is_some())),
            ("armed".into(), Value::Bool(s.armed.is_some())),
            (
                "accent".into(),
                color(w.accent.unwrap_or(self.cfg.theme.accent)),
            ),
        ]))
    }

    /// Pass only the changed row to Slint.
    pub(super) fn sync(&self, i: usize) {
        if i < self.model.row_count() {
            self.model.set_row_data(i, self.row(i));
        }
    }

    /// A finger is moving the panel: follow without delay.
    pub(super) fn follow(&self, p: f64) {
        self.set_progress(p, true);
    }

    /// Glide to the target (opening, closing, snapping back), duration from `theme.animation_ms`.
    pub(super) fn glide_to(&self, p: f64) {
        self.set_progress(p, false);
    }

    fn set_progress(&self, p: f64, dragging: bool) {
        // `dragging` first: Slint reads the animation duration when `progress` changes.
        self.set("dragging", Value::Bool(dragging));
        self.set("progress", Value::Number(p.clamp(0.0, 1.0)));
    }

    pub(super) fn push_touches(&self) {
        if !self.cfg.overlay.show_touches {
            return;
        }
        let xy: Vec<Value> = self
            .touches
            .values()
            .flat_map(|t| [num(t.x), num(t.y)])
            .collect();
        self.set("touch-count", Value::Number(self.touches.len() as f64));
        self.set("touches", Value::Model(ModelRc::new(VecModel::from(xy))));
    }
}
