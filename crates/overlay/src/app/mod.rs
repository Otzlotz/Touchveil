//! State of the overlay and the handling of all messages in the event loop.
//!
//! Split by task: `view` (rendering), `touch` (interaction) and `widgets` (the widgets'
//! commands). The fields are private; the submodules can still see them.

mod touch;
mod view;
mod widgets;

use crate::config::{self, Config, Kind, CONFIRM_WINDOW};
use crate::exec;
use crate::layout::Layout;
use crate::link::{send_to_daemon, Writer};
use crate::ticker::Ticker;
use crate::tray;
use layer_shika::calloop::channel::Sender;
use layer_shika::slint::{ModelRc, VecModel};
use layer_shika::slint_interpreter::{ComponentInstance, Value};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;
use touchveil_common::{ToDaemon, ToOverlay};

/// Messages to the event loop.
#[derive(Debug)]
pub enum Msg {
    Daemon(ToOverlay),
    /// Connected to the daemon.
    Online,
    /// Connection to the daemon lost.
    Offline,
    /// Open the overlay from the tray.
    TrayOpen,
    /// Clock tick for displays.
    Tick,
    /// Result of a read command (state/value/display) for widget `idx` of config generation `gen`.
    Read {
        gen: u64,
        idx: usize,
        /// `None`: nothing to apply (toggle without `state`), only reset `busy`.
        res: Option<exec::Output>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    None,
    Widget(usize),
    Close,
    /// Next to the panel: a tap closes.
    Outside,
    /// Empty area in the panel: pushing back closes.
    Panel {
        start: (f32, f32),
    },
}

struct Touch {
    x: f32,
    y: f32,
    target: Target,
}

/// Runtime state of a widget.
#[derive(Default)]
struct WState {
    pressed_by: Option<u32>,
    on: bool,
    value: f64,
    text: String,
    /// A command of the widget is currently running.
    busy: bool,
    next_poll: Option<Instant>,
    /// A slider's own command worker.
    slider: Option<exec::Coalescer>,
    /// `confirm = true`: first tap happened at this time, waiting for the second.
    armed: Option<Instant>,
}

pub struct Overlay {
    ui: ComponentInstance,
    writer: Writer,
    tx: Sender<Msg>,
    watched: config::Watched,
    cfg: Config,
    gen: u64,
    error: String,
    layout: Layout,
    size: (f32, f32),
    open: bool,
    states: Vec<WState>,
    model: Rc<VecModel<Value>>,
    touches: BTreeMap<u32, Touch>,
    /// Last touch while the overlay was open (for `auto_close_s`).
    last_touch: Instant,
    ticker: Ticker,
    tray: Option<tray::Handle>,
}

impl Overlay {
    /// Creates the overlay and applies the config read at startup.
    pub fn new(
        ui: ComponentInstance,
        writer: Writer,
        tx: Sender<Msg>,
        watched: config::Watched,
        first: Result<Config, String>,
    ) -> Self {
        let model = Rc::new(VecModel::<Value>::default());
        if let Err(e) = ui.set_property("widgets", Value::Model(ModelRc::from(model.clone()))) {
            eprintln!("Property widgets: {e:?}");
        }
        let mut overlay = Overlay {
            ui,
            writer,
            ticker: Ticker::new(tx.clone()),
            tray: tray::Tray::spawn(tx.clone(), watched.path.clone()),
            tx,
            watched,
            cfg: Config::default(),
            gen: 0,
            error: String::new(),
            layout: Layout::default(),
            size: (0.0, 0.0),
            open: false,
            states: Vec::new(),
            model,
            touches: BTreeMap::new(),
            last_touch: Instant::now(),
        };
        if let Err(e) = &first {
            eprintln!("Config: {e}");
            overlay.error = format!("Config error:\n{e}");
        }
        overlay.apply_config(first.unwrap_or_default());
        overlay
    }

    // ---------- Messages ----------

    pub(super) fn close(&mut self) {
        self.open = false;
        self.ticker.stop();
        self.release_all();
        self.disarm_all();
        self.glide_to(0.0);
        self.push_touches();
    }

    pub(super) fn set_tray_connected(&self, connected: bool) {
        if let Some(t) = &self.tray {
            t.update(|t| t.connected = connected);
        }
    }

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Daemon(ToOverlay::Progress(p)) => {
                self.relayout();
                // The daemon sends 0 when the gesture is cancelled: glide back.
                if p > 0.0 {
                    self.follow(p as f64);
                } else {
                    self.glide_to(0.0);
                }
            }
            Msg::Daemon(ToOverlay::Hint { x, y }) => self.set_hint(Some((x, y))),
            Msg::Daemon(ToOverlay::HintOff) => self.set_hint(None),
            Msg::Daemon(ToOverlay::Open) => {
                self.reload();
                self.relayout();
                self.set_hint(None);
                self.open = true;
                self.glide_to(1.0);
                for i in 0..self.states.len() {
                    self.read(i);
                }
                self.last_touch = Instant::now();
                // The clock only runs when something needs it.
                let o = &self.cfg.overlay;
                let needs_tick = o.auto_close_s > 0
                    || self
                        .cfg
                        .widget
                        .iter()
                        .any(|w| w.confirm || matches!(w.kind, Kind::Display { .. }));
                if needs_tick {
                    self.ticker.start();
                }
            }
            Msg::Online => self.set_tray_connected(true),
            Msg::Offline => {
                self.set_tray_connected(false);
                self.set_hint(None);
                self.close();
            }
            Msg::TrayOpen if !self.open => send_to_daemon(&self.writer, ToDaemon::Open),
            Msg::TrayOpen => {}
            Msg::Daemon(ToOverlay::Close) => self.close(),
            Msg::Daemon(ToOverlay::Down { slot, x, y }) if self.open => {
                self.last_touch = Instant::now();
                let (x, y) = self.to_window(x, y);
                self.touch_down(slot, x, y);
                self.push_touches();
            }
            Msg::Daemon(ToOverlay::Move { slot, x, y }) if self.open => {
                self.last_touch = Instant::now();
                let (x, y) = self.to_window(x, y);
                self.touch_move(slot, x, y);
                self.push_touches();
            }
            Msg::Daemon(ToOverlay::Up { slot }) => {
                self.last_touch = Instant::now();
                self.touch_up(slot);
                self.push_touches();
            }
            Msg::Daemon(ToOverlay::Down { .. } | ToOverlay::Move { .. }) => {}
            Msg::Tick if self.open => {
                let now = Instant::now();
                for i in 0..self.states.len() {
                    let due = matches!(self.cfg.widget[i].kind, Kind::Display { .. })
                        && self.states[i].next_poll.is_some_and(|t| now >= t);
                    if due {
                        self.read(i);
                    }
                    let expired = self.states[i]
                        .armed
                        .is_some_and(|t| now >= t + CONFIRM_WINDOW);
                    if expired {
                        self.states[i].armed = None;
                        self.sync(i);
                    }
                }
                let idle = u64::from(self.cfg.overlay.auto_close_s);
                if idle > 0
                    && self.touches.is_empty()
                    && now.duration_since(self.last_touch).as_secs() >= idle
                {
                    self.last_touch = now; // do not send again until the daemon closes
                    send_to_daemon(&self.writer, ToDaemon::Close);
                }
            }
            Msg::Tick => {}
            Msg::Read { gen, idx, res } => self.on_read(gen, idx, res),
        }
    }
}
