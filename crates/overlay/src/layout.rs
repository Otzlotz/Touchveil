//! Geometry in logical pixels: panel, header, grid cells, hit testing.
//! Slint only draws what is computed here, so that picture and hits match.

use crate::config::{Config, Widget};
use touchveil_common::Side;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// `panel` is in window coordinates, everything else relative to the panel (0,0 = its top-left corner).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Panel in the open state.
    pub panel: Rect,
    /// Offset of the panel in the closed state (off screen).
    pub slide: (f32, f32),
    pub side: Side,
    pub header_h: f32,
    pub close: Option<Rect>,
    pub widgets: Vec<Rect>,
}

/// Minimum size of the panel so that widgets stay usable.
const MIN_PANEL: f32 = 320.0;

impl Layout {
    pub fn compute(cfg: &Config, win_w: f32, win_h: f32) -> Self {
        let t = &cfg.theme;
        let pad = t.padding.max(0.0);
        let gap = t.gap.max(0.0);
        let side = cfg.gesture.edge.side();
        let size = |total: f32| (total * cfg.overlay.size).max(MIN_PANEL.min(total));
        let (panel, slide) = match side {
            Side::Right => {
                let w = size(win_w);
                (
                    Rect {
                        x: win_w - w,
                        y: 0.0,
                        w,
                        h: win_h,
                    },
                    (w, 0.0),
                )
            }
            Side::Left => {
                let w = size(win_w);
                (
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w,
                        h: win_h,
                    },
                    (-w, 0.0),
                )
            }
            Side::Top => {
                let h = size(win_h);
                (
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: win_w,
                        h,
                    },
                    (0.0, -h),
                )
            }
            Side::Bottom => {
                let h = size(win_h);
                (
                    Rect {
                        x: 0.0,
                        y: win_h - h,
                        w: win_w,
                        h,
                    },
                    (0.0, h),
                )
            }
        };
        let (panel_w, panel_h) = (panel.w, panel.h);

        let has_header = cfg.overlay.close_button || !cfg.overlay.title.is_empty();
        let header_h = if has_header {
            (t.font_size * 3.0).max(64.0)
        } else {
            0.0
        };
        let close = cfg.overlay.close_button.then(|| {
            let h = header_h;
            let w = (h * 2.0).min(panel_w - 2.0 * pad);
            Rect {
                x: panel_w - pad - w,
                y: pad,
                w,
                h,
            }
        });

        let top = pad + if has_header { header_h + gap } else { 0.0 };
        let area = Rect {
            x: pad,
            y: top,
            w: (panel_w - 2.0 * pad).max(0.0),
            h: (panel_h - top - pad).max(0.0),
        };
        let (cols, rows) = (cfg.grid.columns as f32, cfg.grid.rows as f32);
        let cell_w = ((area.w - gap * (cols - 1.0)) / cols).max(0.0);
        let cell_h = ((area.h - gap * (rows - 1.0)) / rows).max(0.0);

        let widgets = cfg
            .widget
            .iter()
            .map(|w: &Widget| {
                let (c, r) = ((w.col - 1) as f32, (w.row - 1) as f32);
                let (sw, sh) = (w.w as f32, w.h as f32);
                Rect {
                    x: area.x + c * (cell_w + gap),
                    y: area.y + r * (cell_h + gap),
                    w: sw * cell_w + (sw - 1.0) * gap,
                    h: sh * cell_h + (sh - 1.0) * gap,
                }
            })
            .collect();

        Self {
            panel,
            slide,
            side,
            header_h,
            close,
            widgets,
        }
    }

    /// Window coordinates -> panel coordinates (open panel).
    pub fn to_panel(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.panel.x, y - self.panel.y)
    }

    /// How far a finger has pushed the panel from `start` to `cur` back towards the edge,
    /// as a fraction of the panel size (0 = not at all, 1 = all the way out).
    pub fn pushed_out(&self, start: (f32, f32), cur: (f32, f32)) -> f32 {
        let (dx, dy) = (cur.0 - start.0, cur.1 - start.1);
        let (d, extent) = match self.side {
            Side::Right => (dx, self.panel.w),
            Side::Left => (-dx, self.panel.w),
            Side::Top => (-dy, self.panel.h),
            Side::Bottom => (dy, self.panel.h),
        };
        (d / extent.max(1.0)).clamp(0.0, 1.0)
    }

    /// Widget under a point (panel coordinates). Ones defined later lie on top.
    pub fn widget_at(&self, x: f32, y: f32) -> Option<usize> {
        self.widgets.iter().rposition(|r| r.contains(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(widgets: &str) -> Config {
        Config::parse(&format!(
            "[overlay]\nsize = 0.5\nclose_button = false\ntitle = \"\"\n[theme]\npadding = 10\ngap = 10\n[grid]\ncolumns = 2\nrows = 2\n{widgets}"
        ))
        .unwrap()
    }

    #[test]
    fn spans_cover_cells_and_gaps() {
        let c = cfg(
            "[[widget]]\ntype=\"button\"\nexec=\"x\"\ncol=1\nrow=1\nw=2\n\
             [[widget]]\ntype=\"button\"\nexec=\"x\"\ncol=2\nrow=2\n",
        );
        // Panel 500 wide: area 480x580, cells 235x285
        let l = Layout::compute(&c, 1000.0, 600.0);
        assert_eq!(
            l.panel,
            Rect {
                x: 500.0,
                y: 0.0,
                w: 500.0,
                h: 600.0
            }
        );
        assert_eq!(
            l.widgets[0],
            Rect {
                x: 10.0,
                y: 10.0,
                w: 480.0,
                h: 285.0
            }
        );
        assert_eq!(
            l.widgets[1],
            Rect {
                x: 255.0,
                y: 305.0,
                w: 235.0,
                h: 285.0
            }
        );
        assert_eq!(l.widget_at(400.0, 100.0), Some(0));
        assert_eq!(l.widget_at(300.0, 400.0), Some(1));
        assert_eq!(l.widget_at(100.0, 400.0), None);
    }

    #[test]
    fn narrow_screen_gets_minimum_panel_width() {
        let l = Layout::compute(&cfg(""), 400.0, 300.0);
        assert_eq!(
            l.panel,
            Rect {
                x: 80.0,
                y: 0.0,
                w: 320.0,
                h: 300.0
            }
        );
    }

    #[test]
    fn panel_follows_chosen_side() {
        let mut c = cfg("[[widget]]\ntype=\"button\"\nexec=\"x\"\ncol=1\nrow=1\n");
        c.gesture.edge = touchveil_common::Edge::Bottom;
        let l = Layout::compute(&c, 1000.0, 1000.0);
        assert_eq!(
            l.panel,
            Rect {
                x: 0.0,
                y: 500.0,
                w: 1000.0,
                h: 500.0
            }
        );
        assert_eq!(l.slide, (0.0, 500.0));
        // Grid fills the panel: cell 485 x 235
        assert_eq!(
            l.widgets[0],
            Rect {
                x: 10.0,
                y: 10.0,
                w: 485.0,
                h: 235.0
            }
        );
        // Pushing down closes, pushing up does not
        assert_eq!(l.pushed_out((500.0, 600.0), (500.0, 850.0)), 0.5);
        assert_eq!(l.pushed_out((500.0, 600.0), (500.0, 500.0)), 0.0);

        c.gesture.edge = touchveil_common::Edge::TopLeft;
        let l = Layout::compute(&c, 1000.0, 1000.0);
        assert_eq!(
            l.panel,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 500.0,
                h: 1000.0
            }
        );
        assert_eq!(l.slide, (-500.0, 0.0));
    }
}
