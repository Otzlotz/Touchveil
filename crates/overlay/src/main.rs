//! Overlay client (Slint + layer-shika), driven over the daemon's Unix socket.
//!
//! Protocol to the daemon: see `touchveil_common::{ToOverlay, ToDaemon}`.
//!
//! Widgets, actions and appearance come from config.toml ([overlay], [theme], [grid],
//! [[widget]]). The file is reloaded on every open if it has changed.

mod app;
mod config;
mod exec;
mod layout;
mod link;
mod outputs;
mod ticker;
mod tray;

use app::{Msg, Overlay};
use config::Config;
use layer_shika::calloop::channel;
use layer_shika::prelude::*;
use layer_shika::slint::ComponentHandle;
use link::{spawn_reader, Writer};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const SURFACE: &str = "Overlay";

fn main() -> layer_shika::Result<()> {
    let mut monitor: Option<String> = None;
    let mut socket: Option<PathBuf> = None;
    let mut cfg_path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--monitor" => monitor = args.next(),
            "--socket" => socket = args.next().map(PathBuf::from),
            "--config" => cfg_path = args.next().map(PathBuf::from),
            _ => eprintln!("unknown argument: {a}"),
        }
    }

    let cfg_path = cfg_path.or_else(config::find);
    match &cfg_path {
        Some(p) => eprintln!("Config: {}", p.display()),
        None => eprintln!("No config.toml found, overlay stays empty."),
    }
    let mut watched = config::Watched::new(cfg_path);
    // The socket path is only read at startup.
    let first = watched.poll().unwrap_or_else(|| Ok(Config::default()));
    let sock = socket.unwrap_or_else(|| {
        first
            .as_ref()
            .map_or_else(|_| touchveil_common::default_socket(), Config::socket_path)
    });

    // Only one overlay at a time: two would keep displacing each other at the daemon.
    let _lock = match touchveil_common::lock_instance(&sock.with_extension("overlay.lock")) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!("touchveil-overlay is already running (icon in the tray).");
            return Ok(());
        }
        Err(e) => {
            eprintln!("Lock file: {e}");
            std::process::exit(1);
        }
    };

    let wanted = monitor.or_else(|| first.as_ref().ok()?.overlay.monitor.clone());
    let monitor = match outputs::names().and_then(|n| outputs::choose(wanted.as_deref(), &n)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(2);
        }
    };
    eprintln!("Monitor: {monitor}");

    // Needs the output name fix in layer-shika (see Cargo.toml).
    let want = monitor.clone();
    let policy = OutputPolicy::custom(move |info| info.name() == Some(want.as_str()));

    let mut shell = Shell::from_source(include_str!("../ui/overlay.slint"))
        .surface(SURFACE)
        .layer(Layer::Overlay)
        .anchor(AnchorEdges::all())
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::None)
        .namespace("touchveil")
        .output_policy(policy)
        .build()
        .map_err(|e| {
            eprintln!("Could not create the overlay on monitor {monitor}: {e}");
            e
        })?;

    // Empty input region: the overlay gets its touches from the daemon over the socket.
    // Without this the (invisible) fullscreen surface would catch all touches and mouse
    // clicks on this monitor instead of passing them to the windows below.
    shell
        .control()
        .surface(SURFACE)
        .set_input_region(0, 0, 0, 0)?;

    // Grab the component instance before the event loop runs
    let ui = shell.with_surface(SURFACE, |c| c.clone_strong())?;

    let writer: Writer = Arc::new(Mutex::new(None));

    // Channel into the event loop: messages from the reader, ticker and command threads
    // end up on the main thread.
    let (tx, rx) = channel::channel::<Msg>();
    let mut overlay = Overlay::new(ui, writer.clone(), tx.clone(), watched, first);

    shell
        .event_loop_handle()
        .insert_source(rx, move |ev, (), _state| {
            if let channel::Event::Msg(msg) = ev {
                overlay.handle(msg);
            }
        })?;

    spawn_reader(sock, tx, writer);

    shell.run()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn slint_ui_compiles() {
        let compiler = layer_shika::slint_interpreter::Compiler::default();
        let r = spin_on(compiler.build_from_source(
            include_str!("../ui/overlay.slint").into(),
            PathBuf::from("overlay.slint"),
        ));
        let errors: Vec<String> = r.diagnostics().map(|d| d.to_string()).collect();
        assert!(r.component("Overlay").is_some(), "{errors:#?}");
    }

    /// Minimal executor: build_from_source does not wait on anything external.
    fn spin_on<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
}
