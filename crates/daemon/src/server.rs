//! Main loop: waits for the touchscreen, serves it and accepts overlay connections.

use crate::config::Config;
use crate::device::{build_virtual, find_devices, not_found_error, set_nonblocking};
use crate::filter::Filter;
use crate::router::{prepare_client, Geo, Router, POS_X, POS_Y, SLOT};
use evdev::Device;
use std::io::{self, ErrorKind, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Interval between two searches while no touchscreen is present.
const DEVICE_RETRY: Duration = Duration::from_secs(2);

fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

/// `poll` with retry on EINTR. `timeout_ms` < 0 waits indefinitely.
fn poll(fds: &mut [libc::pollfd], timeout_ms: i32) -> io::Result<()> {
    loop {
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) } >= 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Waits for a matching touchscreen. Meanwhile overlay connections are accepted and
/// disconnects noticed, so the overlay is already connected when the device
/// shows up.
pub(crate) fn wait_for_device(
    filter: Option<&Filter>,
    device_cfg: Option<&str>,
    listener: &UnixListener,
    client: &mut Option<UnixStream>,
) -> io::Result<(PathBuf, Device)> {
    let mut warned = false;
    loop {
        let mut found = find_devices(filter);
        if !found.is_empty() {
            if found.len() > 1 {
                let names: Vec<&str> = found.iter().map(|(_, d)| d.name().unwrap_or("?")).collect();
                eprintln!(
                    "Multiple touch devices ({}), using the first. Pick one with device = \"...\".",
                    names.join(", ")
                );
            }
            return Ok(found.remove(0));
        }
        if !warned {
            eprintln!(
                "{} Waiting for a touchscreen …",
                not_found_error(device_cfg)
            );
            warned = true;
        }

        let deadline = Instant::now() + DEVICE_RETRY;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let mut fds = [
                pollfd(listener.as_raw_fd()),
                pollfd(client.as_ref().map_or(-1, |c| c.as_raw_fd())),
            ];
            poll(&mut fds, left.as_millis() as i32 + 1)?;
            if fds[0].revents & libc::POLLIN != 0 {
                if let Ok((s, _)) = listener.accept() {
                    *client = prepare_client(s);
                }
            }
            if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                // Without a device there is nothing to open; only detect disconnects.
                let mut buf = [0u8; 256];
                if matches!(
                    client.as_mut().map(|c| c.read(&mut buf)),
                    Some(Ok(0) | Err(_))
                ) {
                    *client = None;
                }
            }
        }
    }
}

/// Serves a touchscreen until it is disconnected (`Ok`). An error is fatal.
pub(crate) fn serve(
    cfg: &Config,
    mut dev: Device,
    listener: &UnixListener,
    client: &mut Option<UnixStream>,
) -> io::Result<()> {
    let abs = dev.get_abs_state()?;
    let (ax, ay) = (abs[POS_X as usize], abs[POS_Y as usize]);
    let nslots = (abs[SLOT as usize].maximum + 1).max(1) as usize;
    let cur = abs[SLOT as usize].value.clamp(0, nslots as i32 - 1) as usize;
    let mut cfg = cfg.clone();
    cfg.gesture.fingers = cfg.gesture.fingers.min(nslots);
    let geo = Geo {
        x: (ax.minimum, ax.maximum),
        y: (ay.minimum, ay.maximum),
        rotate: cfg.rotate,
    };

    let vdev = build_virtual(&dev)?;
    set_nonblocking(dev.as_raw_fd())?;
    dev.grab()?; // from here on only this program sees the original events

    let mut d = Router::new(cfg, geo, vdev, nslots, cur, client.take());

    let lost = loop {
        let mut fds = [
            pollfd(dev.as_raw_fd()),
            pollfd(listener.as_raw_fd()),
            pollfd(d.client.as_ref().map_or(-1, |c| c.as_raw_fd())), // -1: ignored
        ];
        let timeout = d.next_deadline().map_or(-1, |t| t.as_millis() as i32 + 1);
        poll(&mut fds, timeout)?;

        // Device unplugged: without this check poll reports the error endlessly (100 % CPU).
        if fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            break io::Error::new(ErrorKind::BrokenPipe, "device reports an error");
        }
        if fds[0].revents & libc::POLLIN != 0 {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        d.handle(ev)?;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => break e,
            }
        }
        if fds[1].revents & libc::POLLIN != 0 {
            match listener.accept() {
                Ok((s, _)) => {
                    d.set_client(s);
                    d.emit()?;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        if fds[2].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            d.read_client();
        }
        d.check_timeouts(Instant::now())?;
    };

    eprintln!("Touch device disconnected ({lost}).");
    d.close_overlay();
    *client = d.client.take();
    Ok(())
}
