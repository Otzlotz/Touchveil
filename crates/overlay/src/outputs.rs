//! Connector names of the monitors (e.g. "DP-3"), to pick the only monitor when none is
//! given and to list the available ones when the name is wrong.

use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle};

struct State(Vec<Option<String>>);

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            // Names only exist from wl_output version 4 on.
            if interface == "wl_output" && version >= 4 {
                registry.bind::<wl_output::WlOutput, _, _>(name, 4, qh, state.0.len());
                state.0.push(None);
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.0[*idx] = Some(name);
        }
    }
}

/// Names of all monitors in the compositor's order.
pub fn names() -> Result<Vec<String>, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let mut queue = conn.new_event_queue();
    conn.display().get_registry(&queue.handle(), ());
    let mut state = State(Vec::new());
    // First roundtrip: globals and bind, second: name events of the outputs.
    for _ in 0..2 {
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("Wayland: {e}"))?;
    }
    Ok(state.0.into_iter().flatten().collect())
}

/// Picks the monitor: the one given explicitly, otherwise the only one available.
pub fn choose(wanted: Option<&str>, available: &[String]) -> Result<String, String> {
    let list = available.join(", ");
    match wanted {
        Some(w) if available.iter().any(|a| a == w) => Ok(w.to_string()),
        Some(w) => Err(format!("Monitor \"{w}\" does not exist. Available: {list}")),
        None => match available {
            [only] => Ok(only.clone()),
            [] => Err("No monitor found.".into()),
            _ => Err(format!(
                "Multiple monitors ({list}): choose with [overlay] monitor = \"...\" in config.toml \
                 or with --monitor which one the overlay appears on."
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_monitor() {
        let one = vec!["eDP-1".to_string()];
        let two = vec!["eDP-1".to_string(), "DP-3".to_string()];
        assert_eq!(choose(None, &one), Ok("eDP-1".into()));
        assert_eq!(choose(Some("DP-3"), &two), Ok("DP-3".into()));
        assert!(choose(None, &two).unwrap_err().contains("eDP-1, DP-3"));
        assert!(choose(Some("HDMI-A-1"), &two)
            .unwrap_err()
            .contains("Available: eDP-1, DP-3"));
    }
}
