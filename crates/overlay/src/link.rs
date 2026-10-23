//! Connection to the daemon: reads its messages in a separate thread and sends requests
//! (open/close) back. If it drops, it reconnects every second.

use crate::app::Msg;
use layer_shika::calloop::channel::Sender;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use touchveil_common::ToDaemon;

/// Writing side of the connection, `None` while disconnected.
pub type Writer = Arc<Mutex<Option<UnixStream>>>;

/// Connects (with retries) and reads lines in a separate thread.
/// The messages go through a calloop channel into layer-shika's event loop.
/// The daemon is started by systemd (touchveil-daemon.service), not by the overlay.
pub fn spawn_reader(path: PathBuf, tx: Sender<Msg>, writer: Writer) {
    std::thread::spawn(move || {
        let mut warned = false;
        loop {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    warned = false;
                    eprintln!("connected: {}", path.display());
                    *writer.lock().unwrap() = stream.try_clone().ok();
                    if tx.send(Msg::Online).is_err() {
                        return;
                    }
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { break };
                        match line.parse() {
                            Ok(m) => {
                                if tx.send(Msg::Daemon(m)).is_err() {
                                    return;
                                }
                            }
                            Err(e) => eprintln!("{e}"),
                        }
                    }
                    eprintln!("connection closed");
                    *writer.lock().unwrap() = None;
                    let _ = tx.send(Msg::Offline);
                }
                Err(e) if !warned => {
                    eprintln!(
                        "connection to {} failed: {e} (is touchveil-daemon.service running?)",
                        path.display()
                    );
                    warned = true;
                }
                Err(_) => {}
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

pub fn send_to_daemon(writer: &Writer, msg: ToDaemon) {
    if let Some(s) = writer.lock().unwrap().as_mut() {
        let _ = writeln!(s, "{msg}");
    }
}
