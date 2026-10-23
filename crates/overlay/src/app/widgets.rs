//! Widget commands: read state, set sliders, trigger buttons and toggles.

use super::{Msg, Overlay};
use crate::config::{Kind, Purpose, CONFIRM_WINDOW};
use crate::exec;
use crate::link::send_to_daemon;
use crate::ticker::TICK;
use std::time::{Duration, Instant};
use touchveil_common::ToDaemon;

/// Smallest number of decimals that represents `step` exactly (0.25 -> 2, 0.5 -> 1).
fn decimals(step: f64) -> usize {
    (0..6)
        .find(|&d| {
            let scaled = step * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(6)
}

impl Overlay {
    // ---------- Commands ----------

    /// Reads state/value/display of a widget in the background.
    pub(super) fn read(&mut self, i: usize) {
        let w = &self.cfg.widget[i];
        let (cmd, timeout) = match &w.kind {
            Kind::Toggle { state: Some(c), .. } | Kind::Slider { value: Some(c), .. } => {
                (c.clone(), w.timeout(Purpose::Read))
            }
            Kind::Display { exec, .. } => (exec.clone(), w.timeout(Purpose::Display)),
            _ => return,
        };
        if self.states[i].busy {
            return;
        }
        self.states[i].busy = true;
        let (tx, gen) = (self.tx.clone(), self.gen);
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Read {
                gen,
                idx: i,
                res: Some(exec::output(&cmd, timeout)),
            });
        });
    }

    pub(super) fn on_read(&mut self, gen: u64, i: usize, res: Option<exec::Output>) {
        if gen != self.gen || i >= self.states.len() {
            return; // result for an old config
        }
        self.states[i].busy = false;
        if let Kind::Display { interval_ms, .. } = self.cfg.widget[i].kind {
            // Keep polling even after an error, otherwise the display would freeze.
            let every = Duration::from_millis(interval_ms.max(TICK.as_millis() as u64));
            self.states[i].next_poll = Some(Instant::now() + every);
        }
        let (ok, out) = match res {
            None => return,
            Some(exec::Output::Done(ok, out)) => (ok, out),
            // Timeout or start error: displays show it, otherwise the state stays as is.
            Some(exec::Output::TimedOut | exec::Output::Failed) => {
                if let Kind::Display { .. } = self.cfg.widget[i].kind {
                    self.states[i].text = "–".into();
                    self.sync(i);
                }
                return;
            }
        };
        match self.cfg.widget[i].kind {
            Kind::Toggle { .. } => self.states[i].on = exec::truthy(ok, &out),
            Kind::Slider { min, max, .. } => {
                if self.states[i].pressed_by.is_some() {
                    return; // finger takes precedence
                }
                if let Some(v) = exec::number(&out) {
                    self.states[i].value = v.clamp(min, max);
                    self.states[i].text = self.slider_text(i);
                }
            }
            Kind::Display { .. } => {
                self.states[i].text = if out.is_empty() && !ok {
                    "–".into()
                } else {
                    out
                };
            }
            Kind::Button { .. } => {}
        }
        self.sync(i);
    }

    pub(super) fn slider_text(&self, i: usize) -> String {
        let Kind::Slider { step, unit, .. } = &self.cfg.widget[i].kind else {
            return String::new();
        };
        format!("{:.*}{unit}", decimals(*step), self.states[i].value)
    }

    pub(super) fn slider_set(&mut self, i: usize, x: f32) {
        let Kind::Slider {
            exec,
            min,
            max,
            step,
            ..
        } = &self.cfg.widget[i].kind
        else {
            return;
        };
        let r = self.layout.widgets[i];
        let f = ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) as f64;
        let v = (min + f * (max - min) - min) / step;
        let v = (min + v.round() * step).clamp(*min, *max);
        if v == self.states[i].value {
            return;
        }
        let cmd = exec.replace("{value}", &format!("{:.*}", decimals(*step), v));
        self.states[i].value = v;
        self.states[i].text = self.slider_text(i);
        if let Some(worker) = &self.states[i].slider {
            worker.run(cmd);
        }
        self.sync(i);
    }

    /// Tap on a button or toggle.
    pub(super) fn activate(&mut self, i: usize) {
        // `confirm = true`: the first tap only asks for confirmation, the second one triggers.
        if self.cfg.widget[i].confirm {
            let now = Instant::now();
            let armed = self.states[i]
                .armed
                .is_some_and(|t| now < t + CONFIRM_WINDOW);
            self.states[i].armed = (!armed).then_some(now);
            if !armed {
                return;
            }
        }
        match self.cfg.widget[i].kind.clone() {
            Kind::Button { exec, close } => {
                exec::spawn(&exec);
                if close.unwrap_or(self.cfg.overlay.close_after_action) {
                    send_to_daemon(&self.writer, ToDaemon::Close);
                }
            }
            Kind::Toggle { on, off, state } => {
                // If the previous command is still running, the tap does not count: otherwise
                // processes pile up behind a hanging command and the state becomes unclear.
                if self.states[i].busy {
                    return;
                }
                let now_on = !self.states[i].on;
                self.states[i].on = now_on; // show immediately, then read the real state
                let cmd = if now_on { on } else { off };
                let w = &self.cfg.widget[i];
                let (act, read) = (w.timeout(Purpose::Toggle), w.timeout(Purpose::Read));
                let (tx, gen) = (self.tx.clone(), self.gen);
                self.states[i].busy = true;
                std::thread::spawn(move || {
                    exec::status(&cmd, act);
                    let res = state.map(|s| exec::output(&s, read));
                    let _ = tx.send(Msg::Read { gen, idx: i, res });
                });
            }
            // The slider already sent its value while dragging.
            Kind::Slider { .. } | Kind::Display { .. } => {}
        }
    }

    /// Discard pending confirmations (`confirm`), e.g. when closing.
    pub(super) fn disarm_all(&mut self) {
        for i in 0..self.states.len() {
            if self.states[i].armed.take().is_some() {
                self.sync(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_decimals_match_step() {
        assert_eq!(decimals(1.0), 0);
        assert_eq!(decimals(5.0), 0);
        assert_eq!(decimals(0.5), 1);
        assert_eq!(decimals(0.25), 2);
        assert_eq!(decimals(0.05), 2);
        assert_eq!(decimals(0.125), 3);
        assert_eq!(format!("{:.*}", decimals(0.25), 0.75), "0.75");
    }
}
