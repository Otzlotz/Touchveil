//! Routes each finger: to the applications (virtual device), to the edge gesture or,
//! while the overlay is open, to the overlay (socket). Pure state machine without its own
//! I/O polling; the main loop lives in `server.rs`.

use crate::config::Config;
use evdev::uinput::VirtualDevice;
use evdev::{AbsoluteAxisType, EventType, InputEvent, Key};
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::{Duration, Instant};
use touchveil_common::{ToDaemon, ToOverlay};

pub(crate) const SLOT: u16 = AbsoluteAxisType::ABS_MT_SLOT.0;
const TRACKING_ID: u16 = AbsoluteAxisType::ABS_MT_TRACKING_ID.0;
pub(crate) const POS_X: u16 = AbsoluteAxisType::ABS_MT_POSITION_X.0;
pub(crate) const POS_Y: u16 = AbsoluteAxisType::ABS_MT_POSITION_Y.0;
const BTN_TOUCH: u16 = Key::BTN_TOUCH.0;
/// If the overlay hangs longer, it is disconnected and the touchscreen released.
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_millis(200);
/// The flick speed is measured over this period.
const FLING_WINDOW: Duration = Duration::from_millis(100);
/// From this speed (progress per second) on, lifting counts as a flick.
const FLING_SPEED: f32 = 1.5;
/// A flick only opens from this progress on, so a twitch at the edge is not enough.
const FLING_MIN: f32 = 0.1;

pub(crate) fn is_mt(code: u16) -> bool {
    (0x2f..=0x3d).contains(&code) // ABS_MT_SLOT .. ABS_MT_TOOL_Y
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum Role {
    #[default]
    Idle,
    Pending,   // new finger, position unknown -> buffered
    Candidate, // in the edge strip, waiting for more fingers -> buffered
    Forward,   // normal touch -> virtual device
    Gesture,   // part of the edge gesture -> swallowed
    Overlay,   // belongs to the overlay -> socket
    Dead,      // swallowed until lifted
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Normal,
    Dragging,
    Active,
}

#[derive(Clone, Default)]
struct Slot {
    role: Role,
    x: Option<i32>,
    y: Option<i32>,
    /// Start point (normalized, screen orientation) of an edge finger.
    start: (f32, f32),
    since: Option<Instant>,
    /// Touched down in the start zone of the gesture (for the edge hint in the overlay).
    edge: bool,
    dirty: bool,
    buf: Vec<InputEvent>,
}

/// Value range of the device and orientation relative to the screen.
pub(crate) struct Geo {
    pub(crate) x: (i32, i32),
    pub(crate) y: (i32, i32),
    pub(crate) rotate: u16,
}

impl Geo {
    /// Raw coordinates -> normalized (0..1) in screen orientation.
    fn norm(&self, x: i32, y: i32) -> (f32, f32) {
        let span = |(lo, hi): (i32, i32)| (hi - lo).max(1) as f32;
        let nx = ((x - self.x.0) as f32 / span(self.x)).clamp(0.0, 1.0);
        let ny = ((y - self.y.0) as f32 / span(self.y)).clamp(0.0, 1.0);
        touchveil_common::rotate(self.rotate, nx, ny)
    }
}

/// Target of the forwarded events (the uinput device in operation, a buffer in tests).
pub(crate) trait Sink {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()>;
}

impl Sink for VirtualDevice {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        VirtualDevice::emit(self, events) // appends SYN_REPORT itself
    }
}

pub(crate) struct Router<S: Sink> {
    cfg: Config,
    geo: Geo,
    vdev: S,
    slots: Vec<Slot>,
    cur: usize,
    out: Vec<InputEvent>,
    out_slot: i32,
    mode: Mode,
    progress: f32,
    /// Progress of the last [`FLING_WINDOW`] while dragging, for the flick speed.
    samples: VecDeque<(Instant, f32)>,
    /// Last reported edge hint (position of the finger in the start zone).
    hint: Option<(f32, f32)>,
    pub(crate) client: Option<UnixStream>,
    /// BTN_TOUCH state on the virtual device.
    touching: bool,
}

impl<S: Sink> Router<S> {
    pub(crate) fn new(
        cfg: Config,
        geo: Geo,
        vdev: S,
        nslots: usize,
        cur: usize,
        client: Option<UnixStream>,
    ) -> Self {
        Self {
            cfg,
            geo,
            vdev,
            slots: vec![Slot::default(); nslots],
            cur,
            out: Vec::new(),
            out_slot: -1,
            mode: Mode::Normal,
            progress: 0.0,
            samples: VecDeque::new(),
            hint: None,
            client,
            touching: false,
        }
    }

    // ---------- Output ----------

    fn push(&mut self, slot: usize, ev: InputEvent) {
        if self.out_slot != slot as i32 {
            self.out
                .push(InputEvent::new(EventType::ABSOLUTE, SLOT, slot as i32));
            self.out_slot = slot as i32;
        }
        self.out.push(ev);
    }

    fn release_ev() -> InputEvent {
        InputEvent::new(EventType::ABSOLUTE, TRACKING_ID, -1)
    }

    fn flush_buf(&mut self, i: usize) {
        let buf = std::mem::take(&mut self.slots[i].buf);
        for e in buf {
            self.push(i, e);
        }
    }

    pub(crate) fn emit(&mut self) -> io::Result<()> {
        // BTN_TOUCH follows the fingers resting on the virtual device.
        let touching = self.slots.iter().any(|s| s.role == Role::Forward);
        if touching != self.touching {
            self.out
                .push(InputEvent::new(EventType::KEY, BTN_TOUCH, touching as i32));
            self.touching = touching;
        }
        if !self.out.is_empty() {
            self.vdev.emit(&self.out)?;
            self.out.clear();
        }
        Ok(())
    }

    fn send(&mut self, msg: ToOverlay) {
        let Some(c) = self.client.as_mut() else {
            return;
        };
        // Blocking with timeout: a line arrives completely or the client is disconnected.
        // (Non-blocking, half a line could get stuck.)
        if let Err(e) = c.write_all(format!("{msg}\n").as_bytes()) {
            eprintln!("Overlay not responding ({e}), connection closed.");
            self.drop_client();
        }
    }

    fn send_touch(&mut self, down: bool, i: usize) {
        let Some((x, y)) = self.pos(i) else { return };
        let slot = i as u32;
        self.send(if down {
            ToOverlay::Down { slot, x, y }
        } else {
            ToOverlay::Move { slot, x, y }
        });
        self.slots[i].dirty = false;
    }

    /// Normalized position of a finger once X and Y are known.
    fn pos(&self, i: usize) -> Option<(f32, f32)> {
        let (x, y) = (self.slots[i].x?, self.slots[i].y?);
        Some(self.geo.norm(x, y))
    }

    fn drop_client(&mut self) {
        self.client = None;
        self.hint = None;
        self.close_overlay();
    }

    /// A new client replaces the old one. An open overlay is closed first, otherwise
    /// all touches end up at an overlay that is not even visible.
    pub(crate) fn set_client(&mut self, s: UnixStream) {
        let Some(s) = prepare_client(s) else { return };
        if self.client.is_some() {
            self.drop_client();
        } else {
            self.close_overlay();
        }
        self.client = Some(s);
    }

    fn fire(cmd: &[String]) {
        if let Some((prog, rest)) = cmd.split_first() {
            // Non-blocking; a thread waits for the exit, otherwise zombies are left behind.
            match Command::new(prog).args(rest).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => eprintln!("{prog}: {e}"),
            }
        }
    }

    // ---------- Mode changes ----------

    fn open_overlay(&mut self) {
        // Cleanly release fingers still resting on the virtual device, otherwise they get stuck.
        // Discard waiting edge fingers, otherwise they would reach the applications after their
        // time window expires even though the overlay is open (opening from the tray).
        for i in 0..self.slots.len() {
            match self.slots[i].role {
                Role::Forward => {
                    self.push(i, Self::release_ev());
                    self.slots[i].role = Role::Dead;
                }
                Role::Candidate => {
                    self.slots[i].role = Role::Dead;
                    self.slots[i].buf.clear();
                }
                _ => {}
            }
        }
        self.mode = Mode::Active;
        self.progress = 1.0;
        self.update_hint();
        self.send(ToOverlay::Open);
        Self::fire(&self.cfg.actions.on_open);
    }

    pub(crate) fn close_overlay(&mut self) {
        if self.mode != Mode::Active {
            return;
        }
        for s in &mut self.slots {
            if s.role == Role::Overlay {
                s.role = Role::Dead;
            }
        }
        self.mode = Mode::Normal;
        self.progress = 0.0;
        self.send(ToOverlay::Close);
        Self::fire(&self.cfg.actions.on_close);
    }

    /// The first lifted gesture finger ends the drag.
    fn gesture_finger_up(&mut self) {
        if self.mode != Mode::Dragging {
            return;
        }
        let g = &self.cfg.gesture;
        let speed = if g.fling { self.speed() } else { 0.0 };
        let flung = self.progress >= FLING_MIN && speed >= FLING_SPEED;
        let pulled = self.progress >= g.open_threshold && speed > -FLING_SPEED;
        let open = (flung || pulled) && self.client.is_some();
        for s in &mut self.slots {
            if s.role == Role::Gesture {
                s.role = Role::Dead; // swallow the remaining fingers until lifted
            }
        }
        if open {
            self.open_overlay();
        } else {
            self.mode = Mode::Normal;
            self.progress = 0.0;
            self.send(ToOverlay::Progress(0.0));
        }
    }

    /// Drag speed in progress per second (positive: into the screen).
    fn speed(&self) -> f32 {
        let (Some(&(t0, p0)), Some(&(t1, p1))) = (self.samples.front(), self.samples.back()) else {
            return 0.0;
        };
        let dt = t1.duration_since(t0).as_secs_f32();
        if dt < 0.01 {
            return 0.0; // too short for a reliable speed
        }
        (p1 - p0) / dt
    }

    /// After SYN_DROPPED: discard state, release virtual fingers.
    fn reset(&mut self) {
        for i in 0..self.slots.len() {
            if self.slots[i].role == Role::Forward {
                self.push(i, Self::release_ev());
            }
            self.slots[i] = Slot::default();
        }
        if self.mode == Mode::Active {
            self.close_overlay();
        }
        self.mode = Mode::Normal;
        self.progress = 0.0;
        self.update_hint();
    }

    /// Edge hint in the overlay: as long as a finger that touched down in the start zone
    /// still rests there (with some slack), even if it is already forwarded normally,
    /// and during the gesture (the overlay hides it when the panel comes in).
    /// This shows where the gesture starts and that the edge was recognized.
    fn update_hint(&mut self) {
        let g = &self.cfg.gesture;
        let slots = 0..self.slots.len();
        let want = match self.mode {
            Mode::Normal => slots
                .filter(|&i| {
                    let s = &self.slots[i];
                    s.edge && matches!(s.role, Role::Candidate | Role::Forward)
                })
                .filter_map(|i| self.pos(i))
                .find(|&(x, y)| g.edge.in_zone(x, y, g.strip_width * 2.0)),
            Mode::Dragging => slots
                .filter(|&i| self.slots[i].role == Role::Gesture)
                .find_map(|i| self.pos(i)),
            Mode::Active => None,
        };
        let changed = match (self.hint, want) {
            (Some(a), Some(b)) => (a.0 - b.0).abs() + (a.1 - b.1).abs() >= 0.005,
            (a, b) => a.is_some() != b.is_some(),
        };
        if changed {
            self.hint = want;
            self.send(match want {
                Some((x, y)) => ToOverlay::Hint { x, y },
                None => ToOverlay::HintOff,
            });
        }
    }

    // ---------- Input ----------

    pub(crate) fn handle(&mut self, ev: InputEvent) -> io::Result<()> {
        match ev.event_type() {
            EventType::ABSOLUTE => self.on_abs(ev),
            EventType::SYNCHRONIZATION => match ev.code() {
                0 => self.on_syn()?,
                3 => {
                    self.reset();
                    self.emit()?;
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn on_abs(&mut self, ev: InputEvent) {
        let code = ev.code();
        if code == SLOT {
            self.cur = ev.value().clamp(0, self.slots.len() as i32 - 1) as usize;
            return;
        }
        let i = self.cur;
        if code == TRACKING_ID {
            if ev.value() >= 0 {
                // New ID without a preceding -1: the old contact has ended.
                if self.slots[i].role != Role::Idle {
                    self.on_release(i, Self::release_ev());
                }
                self.slots[i] = Slot {
                    role: Role::Pending,
                    ..Slot::default()
                };
                self.slots[i].buf.push(ev);
            } else {
                self.on_release(i, ev);
            }
            return;
        }
        if !is_mt(code) {
            return; // ABS_X/ABS_Y, BTN_TOUCH etc. are not forwarded
        }
        let v = ev.value();
        if code == POS_X {
            self.slots[i].x = Some(v);
            self.slots[i].dirty = true;
        }
        if code == POS_Y {
            self.slots[i].y = Some(v);
            self.slots[i].dirty = true;
        }
        match self.slots[i].role {
            Role::Pending | Role::Candidate => self.slots[i].buf.push(ev),
            Role::Forward => self.push(i, ev),
            _ => {} // Gesture/Overlay/Dead/Idle: not to the virtual device
        }
    }

    fn on_release(&mut self, i: usize, ev: InputEvent) {
        match self.slots[i].role {
            // Short tap in the edge strip or similar: pass through completely as a normal touch.
            Role::Pending | Role::Candidate => {
                self.flush_buf(i);
                self.push(i, ev);
            }
            Role::Forward => self.push(i, ev),
            Role::Overlay => self.send(ToOverlay::Up { slot: i as u32 }),
            Role::Gesture => self.gesture_finger_up(),
            _ => {}
        }
        self.slots[i] = Slot::default();
    }

    fn on_syn(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let n = self.slots.len();

        // 1. Classify new fingers as soon as their position is known.
        for i in 0..n {
            if self.slots[i].role != Role::Pending {
                continue;
            }
            let Some(pos) = self.pos(i) else { continue };
            match self.mode {
                Mode::Active => {
                    self.slots[i].role = Role::Overlay;
                    self.slots[i].buf.clear();
                    self.send_touch(true, i);
                }
                Mode::Dragging => {
                    self.slots[i].role = Role::Dead;
                    self.slots[i].buf.clear();
                }
                Mode::Normal => {
                    // Without an overlay there is nothing to open: forward immediately, without
                    // swallowing the edge gesture or waiting for more fingers.
                    let g = &self.cfg.gesture;
                    if self.client.is_some() && g.edge.in_zone(pos.0, pos.1, g.strip_width) {
                        let s = &mut self.slots[i];
                        s.role = Role::Candidate;
                        s.since = Some(now);
                        s.start = pos;
                        s.edge = true;
                    } else {
                        self.slots[i].role = Role::Forward;
                        self.flush_buf(i);
                    }
                }
            }
        }

        // 1b. Edge fingers that leave the strip (with some slack) are normal touches;
        //     the buffer follows in full, no movement is lost.
        if self.mode == Mode::Normal {
            let g = &self.cfg.gesture;
            let (edge, zone) = (g.edge, g.strip_width * 2.0);
            for i in 0..n {
                if self.slots[i].role != Role::Candidate {
                    continue;
                }
                let left = self.pos(i).is_some_and(|(x, y)| !edge.in_zone(x, y, zone));
                if left {
                    self.slots[i].role = Role::Forward;
                    self.flush_buf(i);
                }
            }
        }

        // 2. Start the gesture when enough fingers are in the edge strip.
        if self.mode == Mode::Normal {
            let cands: Vec<usize> = (0..n)
                .filter(|&i| self.slots[i].role == Role::Candidate)
                .collect();
            if cands.len() >= self.cfg.gesture.fingers.min(n) {
                for i in cands {
                    let s = &mut self.slots[i];
                    s.role = Role::Gesture;
                    s.buf.clear();
                }
                self.mode = Mode::Dragging;
                self.progress = 0.0;
                self.samples.clear();
                self.send(ToOverlay::Progress(0.0));
            }
        }

        // 3. Update the drag progress.
        if self.mode == Mode::Dragging {
            let edge = self.cfg.gesture.edge;
            let moved: Vec<f32> = (0..n)
                .filter(|&i| self.slots[i].role == Role::Gesture)
                .filter_map(|i| Some(edge.inward(self.slots[i].start, self.pos(i)?)))
                .collect();
            if !moved.is_empty() {
                let avg = moved.iter().sum::<f32>() / moved.len() as f32;
                let p = (avg / self.cfg.gesture.travel).clamp(0.0, 1.0);
                self.samples.push_back((now, p));
                while self.samples.len() > 2 && self.samples[0].0 + FLING_WINDOW < now {
                    self.samples.pop_front();
                }
                if (p - self.progress).abs() >= 0.005 {
                    self.progress = p;
                    self.send(ToOverlay::Progress(p));
                }
            }
        }

        self.update_hint();

        // 4. Touch movements to the overlay.
        if self.mode == Mode::Active {
            for i in 0..n {
                if self.slots[i].role == Role::Overlay && self.slots[i].dirty {
                    self.send_touch(false, i);
                }
            }
        }

        self.emit()
    }

    /// Release waiting edge fingers whose time window has expired as normal touches.
    pub(crate) fn check_timeouts(&mut self, now: Instant) -> io::Result<()> {
        if self.cfg.gesture.arm_window_ms == 0 {
            return Ok(()); // no timeout
        }
        let window = Duration::from_millis(self.cfg.gesture.arm_window_ms);
        for i in 0..self.slots.len() {
            let expired = self.slots[i].role == Role::Candidate
                && self.slots[i]
                    .since
                    .is_some_and(|t| now.duration_since(t) >= window);
            if expired {
                self.slots[i].role = Role::Forward;
                self.flush_buf(i);
            }
        }
        self.emit()
    }

    pub(crate) fn next_deadline(&self) -> Option<Duration> {
        if self.cfg.gesture.arm_window_ms == 0 {
            return None;
        }
        let window = Duration::from_millis(self.cfg.gesture.arm_window_ms);
        let now = Instant::now();
        self.slots
            .iter()
            .filter(|s| s.role == Role::Candidate)
            .filter_map(|s| s.since)
            .map(|t| (t + window).saturating_duration_since(now))
            .min()
    }

    pub(crate) fn read_client(&mut self) {
        let mut buf = [0u8; 256];
        let Some(c) = self.client.as_mut() else {
            return;
        };
        match c.read(&mut buf) {
            Ok(0) => self.drop_client(),
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                for line in text.lines() {
                    match line.parse() {
                        Ok(ToDaemon::Close) => self.close_overlay(),
                        // Only from idle; a running gesture decides for itself.
                        Ok(ToDaemon::Open) if self.mode == Mode::Normal => self.open_overlay(),
                        Ok(ToDaemon::Open) => {}
                        Err(e) => eprintln!("Overlay: {e}"),
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => self.drop_client(),
        }
    }
}

/// Prepare the connection to the overlay. Reading only happens after POLLIN, so only
/// writing blocks, and that with a timeout.
pub(crate) fn prepare_client(s: UnixStream) -> Option<UnixStream> {
    match s
        .set_nonblocking(false)
        .and_then(|()| s.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT)))
    {
        Ok(()) => Some(s),
        Err(e) => {
            eprintln!("Overlay connection: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests;
