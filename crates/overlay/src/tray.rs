//! Tray icon (StatusNotifierItem, e.g. in the Waybar tray): open the overlay, edit the
//! config, quit. Runs in its own thread; actions go to the event loop as messages.

use crate::Msg;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use layer_shika::calloop::channel::Sender;
use std::path::PathBuf;

pub struct Tray {
    tx: Sender<Msg>,
    config: Option<PathBuf>,
    /// Connected to the daemon.
    pub connected: bool,
}

pub type Handle = ksni::blocking::Handle<Tray>;

impl Tray {
    /// Starts the tray icon. If there is no tray (yet), it appears as soon as one starts;
    /// the overlay also works entirely without it via the edge gesture.
    pub fn spawn(tx: Sender<Msg>, config: Option<PathBuf>) -> Option<Handle> {
        let tray = Tray {
            tx,
            config,
            connected: false,
        };
        match tray.assume_sni_available(true).spawn() {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("Tray icon not available: {e}");
                None
            }
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "touchveil".into()
    }

    fn title(&self) -> String {
        "Touchveil".into()
    }

    fn icon_name(&self) -> String {
        "input-touchscreen".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = if self.connected {
            "Tap to open the overlay".into()
        } else {
            "Service not reachable: systemctl --user status touchveil-daemon".into()
        };
        ksni::ToolTip {
            title: "Touchveil".into(),
            description,
            ..Default::default()
        }
    }

    /// Left click on the icon.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(Msg::TrayOpen);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open overlay".into(),
                enabled: self.connected,
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(Msg::TrayOpen);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Edit config".into(),
                enabled: self.config.is_some(),
                activate: Box::new(|t: &mut Self| {
                    if let Some(p) = &t.config {
                        crate::exec::spawn_args("xdg-open", &[p.as_os_str()]);
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
