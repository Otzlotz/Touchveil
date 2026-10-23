//! Tests of the state machine with a recording stand-in for the virtual device
//! and a socket pair as the overlay.

use super::*;

#[derive(Default)]
struct Recorder(Vec<Vec<InputEvent>>);

impl Sink for Recorder {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        self.0.push(events.to_vec());
        Ok(())
    }
}

impl Recorder {
    fn has(&self, ty: EventType, code: u16, value: i32) -> bool {
        self.0
            .iter()
            .flatten()
            .any(|e| e.event_type() == ty && e.code() == code && e.value() == value)
    }
}

fn daemon() -> (Router<Recorder>, UnixStream) {
    daemon_with(crate::config::Edge::Right, 0)
}

fn daemon_with(edge: crate::config::Edge, rotate: u16) -> (Router<Recorder>, UnixStream) {
    let mut cfg = Config::default();
    cfg.gesture.edge = edge;
    let geo = Geo {
        x: (0, 1000),
        y: (0, 1000),
        rotate,
    };
    let (a, b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    let mut d = Router::new(cfg, geo, Recorder::default(), 10, 0, None);
    d.set_client(a);
    (d, b)
}

fn abs(d: &mut Router<Recorder>, code: u16, v: i32) {
    d.handle(InputEvent::new(EventType::ABSOLUTE, code, v))
        .unwrap();
}
fn syn(d: &mut Router<Recorder>) {
    d.handle(InputEvent::new(EventType::SYNCHRONIZATION, 0, 0))
        .unwrap();
}
fn down(d: &mut Router<Recorder>, slot: i32, id: i32, x: i32, y: i32) {
    abs(d, SLOT, slot);
    abs(d, TRACKING_ID, id);
    abs(d, POS_X, x);
    abs(d, POS_Y, y);
}
fn up(d: &mut Router<Recorder>, slot: i32) {
    abs(d, SLOT, slot);
    abs(d, TRACKING_ID, -1);
}
fn received(c: &mut UnixStream) -> String {
    let mut s = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = c.read(&mut buf) {
        if n == 0 {
            break;
        }
        s.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    s
}

#[test]
fn normal_touch_is_forwarded() {
    let (mut d, _c) = daemon();
    down(&mut d, 0, 7, 500, 500);
    syn(&mut d);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 7));
    assert!(d.vdev.has(EventType::ABSOLUTE, POS_X, 500));
    assert!(d.vdev.has(EventType::KEY, BTN_TOUCH, 1));
    up(&mut d, 0);
    syn(&mut d);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, -1));
    assert!(d.vdev.has(EventType::KEY, BTN_TOUCH, 0));
}

#[test]
fn edge_finger_is_forwarded_after_arm_window() {
    let (mut d, _c) = daemon();
    d.cfg.gesture.arm_window_ms = 90; // time window only when configured
    down(&mut d, 0, 1, 990, 500);
    syn(&mut d);
    assert!(d.vdev.0.is_empty(), "edge finger must be buffered first");
    d.check_timeouts(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 1));
    assert!(d.vdev.has(EventType::KEY, BTN_TOUCH, 1));
}

#[test]
fn edge_swipe_opens_overlay_and_captures_touches() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 400);
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Dragging);
    abs(&mut d, SLOT, 0);
    abs(&mut d, POS_X, 600);
    abs(&mut d, SLOT, 1);
    abs(&mut d, POS_X, 600);
    syn(&mut d);
    up(&mut d, 0);
    up(&mut d, 1);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Active);
    let msgs = received(&mut c);
    assert!(
        msgs.contains("progress 1.000") && msgs.contains("open"),
        "{msgs}"
    );

    down(&mut d, 0, 3, 500, 500);
    syn(&mut d);
    up(&mut d, 0);
    syn(&mut d);
    let msgs = received(&mut c);
    assert!(
        msgs.contains("down 0 0.5000 0.5000") && msgs.contains("up 0"),
        "{msgs}"
    );
    assert!(d.vdev.0.is_empty(), "nothing may go to the virtual device");

    c.write_all(b"close\n").unwrap();
    d.read_client();
    assert_eq!(d.mode, Mode::Normal);
    assert!(received(&mut c).contains("close"));
}

#[test]
fn short_edge_swipe_does_not_open() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 400);
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    abs(&mut d, SLOT, 0);
    abs(&mut d, POS_X, 950);
    syn(&mut d);
    up(&mut d, 0);
    up(&mut d, 1);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Normal);
    assert!(!received(&mut c).contains("open"));
    assert!(d.vdev.0.is_empty());
}

#[test]
fn forwarded_finger_is_released_when_overlay_opens() {
    let (mut d, _c) = daemon();
    down(&mut d, 2, 9, 300, 300);
    syn(&mut d);
    d.open_overlay();
    d.emit().unwrap();
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, -1));
    assert!(d.vdev.has(EventType::KEY, BTN_TOUCH, 0));
}

#[test]
fn new_client_closes_open_overlay() {
    let (mut d, _c) = daemon();
    d.open_overlay();
    let (a, _b) = UnixStream::pair().unwrap();
    d.set_client(a);
    assert_eq!(d.mode, Mode::Normal);
}

#[test]
fn reused_slot_without_release_releases_old_contact() {
    let (mut d, _c) = daemon();
    down(&mut d, 0, 1, 500, 500);
    syn(&mut d);
    down(&mut d, 0, 2, 400, 400);
    syn(&mut d);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, -1));
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 2));
}

/// Touch down two fingers at `from`, drag to `to`, lift.
fn swipe(d: &mut Router<Recorder>, from: (i32, i32), to: (i32, i32)) {
    down(d, 0, 1, from.0, from.1);
    down(d, 1, 2, from.0, from.1 + 10);
    syn(d);
    for slot in 0..2 {
        abs(d, SLOT, slot);
        abs(d, POS_X, to.0);
        abs(d, POS_Y, to.1 + slot * 10);
    }
    syn(d);
    up(d, 0);
    up(d, 1);
    syn(d);
}

#[test]
fn corner_swipe_opens_in_both_directions() {
    // Top left, dragged vertically downwards
    let (mut d, _c) = daemon_with(crate::config::Edge::TopLeft, 0);
    swipe(&mut d, (20, 20), (20, 500));
    assert_eq!(d.mode, Mode::Active);
    // Top left, dragged horizontally to the right
    let (mut d, _c) = daemon_with(crate::config::Edge::TopLeft, 0);
    swipe(&mut d, (20, 20), (500, 20));
    assert_eq!(d.mode, Mode::Active);
    // Middle of the left edge is not a corner: forwarded normally
    let (mut d, _c) = daemon_with(crate::config::Edge::TopLeft, 0);
    swipe(&mut d, (20, 500), (500, 500));
    assert_eq!(d.mode, Mode::Normal);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 1));
}

#[test]
fn bottom_edge_respects_rotation() {
    // Device rotated by 90 degrees: screen y is raw x, the bottom edge is at raw x = max.
    let (mut d, mut c) = daemon_with(crate::config::Edge::Bottom, 90);
    swipe(&mut d, (990, 500), (500, 500));
    assert_eq!(d.mode, Mode::Active);
    assert!(received(&mut c).contains("open"));
    // Without rotation the same flick would be at the right edge, so no match for "bottom".
    let (mut d, _c) = daemon_with(crate::config::Edge::Bottom, 0);
    swipe(&mut d, (990, 500), (500, 500));
    assert_eq!(d.mode, Mode::Normal);
}

#[test]
fn closing_frees_touchscreen_again() {
    let (mut d, mut c) = daemon();
    swipe(&mut d, (990, 400), (500, 400));
    assert_eq!(d.mode, Mode::Active);
    c.write_all(b"close\n").unwrap();
    d.read_client();
    assert_eq!(d.mode, Mode::Normal);
    d.vdev.0.clear();
    down(&mut d, 0, 7, 300, 300);
    syn(&mut d);
    assert!(
        d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 7),
        "touch must reach the applications again"
    );
    assert!(!received(&mut c).contains("down"));
}

#[test]
fn without_overlay_edge_swipes_reach_apps() {
    let (mut d, _c) = daemon();
    d.client = None;
    down(&mut d, 0, 1, 990, 400);
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    // Without an overlay: forwarded immediately, no gesture, no waiting
    assert_eq!(d.mode, Mode::Normal);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 1));
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 2));
    assert_eq!(d.next_deadline(), None);
}

#[test]
fn unresponsive_overlay_is_dropped_and_touch_freed() {
    let (mut d, c) = daemon();
    d.open_overlay();
    // Overlay no longer reads: let the send buffer fill up
    for i in 0..200_000 {
        d.send(ToOverlay::Move {
            slot: 0,
            x: 0.5,
            y: i as f32,
        });
        if d.client.is_none() {
            break;
        }
    }
    assert!(d.client.is_none(), "hanging overlay must be disconnected");
    assert_eq!(d.mode, Mode::Normal);
    drop(c);
}

#[test]
fn overlay_can_be_opened_from_tray() {
    let (mut d, mut c) = daemon();
    // An edge finger is currently waiting for more fingers
    down(&mut d, 0, 1, 990, 400);
    syn(&mut d);
    c.write_all(b"open\n").unwrap();
    d.read_client();
    assert_eq!(d.mode, Mode::Active);
    assert!(received(&mut c).contains("open"));
    // The waiting edge finger must not reach the applications later
    d.check_timeouts(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(!d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 1));
}

#[test]
fn open_request_during_gesture_is_ignored() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 400);
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Dragging);
    c.write_all(b"open\n").unwrap();
    d.read_client();
    assert_eq!(d.mode, Mode::Dragging);
}

#[test]
fn edge_finger_shows_hint_until_released() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 500);
    syn(&mut d);
    assert!(received(&mut c).contains("hint 0.9900 0.5000"));

    // Even after the time window expires (normal touch) the hint stays as long as
    // the finger rests at the edge, and follows it.
    d.check_timeouts(Instant::now() + Duration::from_secs(1))
        .unwrap();
    abs(&mut d, POS_Y, 700);
    syn(&mut d);
    assert!(received(&mut c).contains("hint 0.9900 0.7000"));

    up(&mut d, 0);
    syn(&mut d);
    assert!(received(&mut c).contains("hint off"));
}

#[test]
fn hint_ends_when_finger_leaves_edge() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 500);
    syn(&mut d);
    abs(&mut d, POS_X, 500);
    syn(&mut d);
    let msgs = received(&mut c);
    assert!(
        msgs.contains("hint 0.9900") && msgs.contains("hint off"),
        "{msgs}"
    );
}

#[test]
fn hint_stays_while_dragging_and_ends_when_overlay_opens() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 990, 400);
    syn(&mut d);
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Dragging);
    for s in 0..2 {
        abs(&mut d, SLOT, s);
        abs(&mut d, POS_X, 900);
    }
    syn(&mut d);
    let msgs = received(&mut c);
    assert!(
        !msgs.contains("hint off"),
        "hint stays while dragging: {msgs}"
    );
    assert!(
        msgs.contains("hint 0.9000 0.4000"),
        "follows the finger: {msgs}"
    );

    for s in 0..2 {
        abs(&mut d, SLOT, s);
        abs(&mut d, POS_X, 600);
    }
    syn(&mut d);
    up(&mut d, 0);
    up(&mut d, 1);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Active);
    let msgs = received(&mut c);
    let off = msgs.find("hint off").expect(&msgs);
    assert!(off < msgs.find("open").expect(&msgs), "{msgs}");
}

#[test]
fn one_finger_gesture_shows_hint() {
    let (mut d, mut c) = daemon();
    d.cfg.gesture.fingers = 1;
    down(&mut d, 0, 1, 990, 500);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Dragging);
    assert!(received(&mut c).contains("hint 0.9900 0.5000"));
}

#[test]
fn edge_finger_waits_without_time_limit_until_it_leaves_the_edge() {
    let (mut d, _c) = daemon();
    down(&mut d, 0, 1, 990, 500);
    syn(&mut d);
    assert_eq!(d.next_deadline(), None);
    d.check_timeouts(Instant::now() + Duration::from_secs(60))
        .unwrap();
    assert!(
        d.vdev.0.is_empty(),
        "waits for more fingers without a timeout"
    );

    // Along the edge it stays a candidate ...
    abs(&mut d, POS_Y, 600);
    syn(&mut d);
    assert!(d.vdev.0.is_empty());
    // ... once it leaves the edge, it goes to the applications with its whole movement.
    abs(&mut d, POS_X, 800);
    syn(&mut d);
    assert!(d.vdev.has(EventType::ABSOLUTE, TRACKING_ID, 1));
    assert!(d.vdev.has(EventType::ABSOLUTE, POS_Y, 600));
    assert!(d.vdev.has(EventType::ABSOLUTE, POS_X, 800));

    // A second finger at the edge no longer starts a gesture with the first one.
    down(&mut d, 1, 2, 990, 300);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Normal);
}

#[test]
fn second_finger_may_come_late() {
    let (mut d, _c) = daemon();
    down(&mut d, 0, 1, 990, 400);
    syn(&mut d);
    std::thread::sleep(Duration::from_millis(300));
    d.check_timeouts(Instant::now()).unwrap();
    down(&mut d, 1, 2, 985, 600);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Dragging);
    assert!(d.vdev.0.is_empty(), "nothing may go to the applications");
}

#[test]
fn touch_away_from_edge_shows_no_hint() {
    let (mut d, mut c) = daemon();
    down(&mut d, 0, 1, 500, 500);
    syn(&mut d);
    up(&mut d, 0);
    syn(&mut d);
    assert!(!received(&mut c).contains("hint"));
}

/// Two fingers at the right edge, then `steps` moves of `dx` each with `pause` in between,
/// then lift. Returns whether the overlay is open.
fn flick(fling: bool, steps: i32, dx: i32, pause: Duration) -> bool {
    let (mut d, _c) = daemon();
    d.cfg.gesture.fling = fling;
    down(&mut d, 0, 1, 995, 400);
    down(&mut d, 1, 2, 995, 600);
    syn(&mut d);
    for k in 1..=steps {
        std::thread::sleep(pause);
        for s in 0..2 {
            abs(&mut d, SLOT, s);
            abs(&mut d, POS_X, 995 - k * dx);
        }
        syn(&mut d);
    }
    up(&mut d, 0);
    up(&mut d, 1);
    syn(&mut d);
    d.mode == Mode::Active
}

#[test]
fn quick_flick_opens_below_threshold() {
    // 3 x 40 = 120 units = 0.12 screen -> progress ~0.34 (< 0.5), in ~45 ms.
    // Stays above the flick threshold even with 4x longer pauses (busy CI).
    assert!(flick(true, 3, 40, Duration::from_millis(15)));
    // Without fling it stays closed.
    assert!(!flick(false, 3, 40, Duration::from_millis(15)));
}

#[test]
fn slow_drag_below_threshold_stays_closed() {
    // Same distance, but slow: below the flick speed
    assert!(!flick(true, 3, 30, Duration::from_millis(150)));
}

#[test]
fn flick_back_to_edge_cancels() {
    let (mut d, _c) = daemon();
    down(&mut d, 0, 1, 995, 400);
    down(&mut d, 1, 2, 995, 600);
    syn(&mut d);
    // drag far into the screen (above the threshold), this takes a while ...
    std::thread::sleep(Duration::from_millis(150));
    for s in 0..2 {
        abs(&mut d, SLOT, s);
        abs(&mut d, POS_X, 700);
    }
    syn(&mut d);
    // ... then flick quickly back to the edge, lift while still above the threshold
    for x in [740, 780, 810] {
        std::thread::sleep(Duration::from_millis(15));
        for s in 0..2 {
            abs(&mut d, SLOT, s);
            abs(&mut d, POS_X, x);
        }
        syn(&mut d);
    }
    assert!(
        d.progress >= 0.5,
        "still above the threshold: {}",
        d.progress
    );
    up(&mut d, 0);
    up(&mut d, 1);
    syn(&mut d);
    assert_eq!(d.mode, Mode::Normal);
}
