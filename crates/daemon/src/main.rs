//! touchveil-daemon: takes over the touchscreen, passes normal touches through and controls
//! the overlay with an edge gesture. Runs as a systemd user service (packaging/).

mod config;
mod device;
mod filter;
mod router;
mod server;

use config::Config;
use filter::Filter;
use std::io::{self, ErrorKind};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("touchveil-daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let mut cfg_path: Option<PathBuf> = None;
    let mut dev_override: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => cfg_path = args.next().map(PathBuf::from),
            "--device" => dev_override = args.next(),
            _ => eprintln!("unknown argument: {a}"),
        }
    }
    let mut cfg = Config::load(cfg_path).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    if let Some(d) = dev_override {
        if d.trim().is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "--device must not be empty",
            ));
        }
        cfg.device = Some(d);
    }

    // If a daemon is already running, do not take over its socket. A lock file instead of a
    // test connect, because the running daemon treats every connection as a new overlay.
    let sock_path = cfg.socket_path();
    let Some(_lock) = touchveil_common::lock_instance(&sock_path.with_extension("lock"))? else {
        eprintln!(
            "touchveil-daemon is already running ({}).",
            sock_path.display()
        );
        return Ok(());
    };

    let filter = cfg
        .device
        .as_deref()
        .map(Filter::parse)
        .transpose()
        .map_err(|e| io::Error::new(ErrorKind::InvalidInput, format!("device: {e}")))?;

    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path)?;
    listener.set_nonblocking(true)?;

    // If the touchscreen is missing or gets unplugged, wait instead of exiting: as a
    // service the daemon would otherwise be restarted endlessly.
    let mut client = None;
    loop {
        let (path, dev) = server::wait_for_device(
            filter.as_ref(),
            cfg.device.as_deref(),
            &listener,
            &mut client,
        )?;
        eprintln!(
            "Touch device: {} ({})",
            dev.name().unwrap_or("?"),
            path.display()
        );
        server::serve(&cfg, dev, &listener, &mut client)?;
    }
}
