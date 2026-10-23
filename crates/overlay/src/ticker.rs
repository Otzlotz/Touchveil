//! Clock for displays, only while the overlay is open.

use crate::app::Msg;
use layer_shika::calloop::channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Interval at which displays are checked (only while the overlay is open).
pub const TICK: Duration = Duration::from_millis(200);

/// Sends `Msg::Tick` while the overlay is open and otherwise sleeps without CPU load.
pub struct Ticker {
    running: Arc<AtomicBool>,
    thread: std::thread::Thread,
}

impl Ticker {
    pub fn new(tx: Sender<Msg>) -> Self {
        let running = Arc::new(AtomicBool::new(false));
        let r = running.clone();
        let handle = std::thread::spawn(move || loop {
            if r.load(Ordering::Relaxed) {
                std::thread::sleep(TICK);
                if r.load(Ordering::Relaxed) && tx.send(Msg::Tick).is_err() {
                    return;
                }
            } else {
                std::thread::park();
            }
        });
        Self {
            running,
            thread: handle.thread().clone(),
        }
    }
    pub fn start(&self) {
        self.running.store(true, Ordering::Relaxed);
        self.thread.unpark();
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
