//! Run commands without blocking the UI thread. All commands run via `sh -c`.
//!
//! Commands of widgets (except buttons) have a timeout. They run in their own process
//! group so that on timeout their children (pipelines, `&`) are killed too; a `kill` on
//! `sh` alone would leave them running.

use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long to keep waiting for the end of the output after `sh` has exited. If a
/// background process keeps the output open, it is killed afterwards.
const OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// Result of a command with a timeout.
#[derive(Debug, PartialEq)]
pub enum Output {
    /// Finished: exit code 0?, stdout without surrounding whitespace (empty when not captured).
    Done(bool, String),
    /// Timeout exceeded, process group killed.
    TimedOut,
    /// Could not be started.
    Failed,
}

fn sh(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd).stdin(Stdio::null());
    c
}

/// Starts a command in the background. A small thread waits for it to exit so that no
/// zombie processes are left behind (also for long-running programs).
pub fn spawn(cmd: &str) {
    spawn_command(sh(cmd), cmd);
}

/// Like [`spawn`], but without a shell: arguments are passed unchanged.
pub fn spawn_args(prog: &str, args: &[&OsStr]) {
    let mut c = Command::new(prog);
    c.args(args).stdin(Stdio::null());
    spawn_command(c, prog);
}

fn spawn_command(mut c: Command, what: &str) {
    match c.stdout(Stdio::null()).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("command \"{what}\": {e}"),
    }
}

/// Runs a command and reads its output, for at most `timeout`.
pub fn output(cmd: &str, timeout: Duration) -> Output {
    run(cmd, timeout, true)
}

/// Runs a command without reading its output, for at most `timeout`. Whatever it starts
/// in the background keeps running (e.g. `on = "program &"`).
pub fn status(cmd: &str, timeout: Duration) -> Output {
    run(cmd, timeout, false)
}

fn run(cmd: &str, timeout: Duration, capture: bool) -> Output {
    let deadline = Instant::now() + timeout;
    let mut c = sh(cmd);
    c.process_group(0).stdout(if capture {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("command \"{cmd}\": {e}");
            return Output::Failed;
        }
    };
    // Process group = PID, because of process_group(0).
    let group = child.id() as libc::pid_t;

    // Read the output in its own thread: a full pipe must not block the command, and
    // background processes can delay the end of the output.
    let (out_tx, out_rx) = mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            let _ = out_tx.send(buf);
        });
    }
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done_tx.send(child.wait());
    });

    let left = || deadline.saturating_duration_since(Instant::now());
    let status = match done_rx.recv_timeout(left()) {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            eprintln!("command \"{cmd}\": {e}");
            return Output::Failed;
        }
        Err(_) => {
            kill_group(group);
            eprintln!("command \"{cmd}\": aborted after {timeout:?}");
            return Output::TimedOut;
        }
    };
    let text = if capture {
        let grace = left().min(OUTPUT_GRACE);
        let buf = out_rx.recv_timeout(grace).unwrap_or_else(|_| {
            // A background process keeps the output open: kill it, then read.
            kill_group(group);
            out_rx.recv().unwrap_or_default()
        });
        String::from_utf8_lossy(&buf).trim().to_string()
    } else {
        String::new()
    };
    Output::Done(status.success(), text)
}

fn kill_group(group: libc::pid_t) {
    // Negative PID: to all processes of the group. If it is already gone, no harm done.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

/// State of a toggle from the output of `state`.
pub fn truthy(success: bool, out: &str) -> bool {
    match out.to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" | "enabled" | "active" => true,
        "false" | "0" | "off" | "no" | "disabled" | "inactive" => false,
        _ => success,
    }
}

/// First number in an output, e.g. "Volume: 42%" -> 42.
pub fn number(out: &str) -> Option<f64> {
    let start = out.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let rest = &out[start..];
    let end = rest
        .char_indices()
        .skip(1)
        .find(|&(_, c)| !(c.is_ascii_digit() || c == '.'))
        .map_or(rest.len(), |(i, _)| i);
    rest[..end].parse().ok()
}

/// Runs the commands of a slider. If new values arrive while dragging and a command is
/// running, only the newest one is run: at most one process per slider, and the final
/// value always gets through. Each slider has its own worker, so a hanging command does
/// not hold up any other slider. The worker ends with the `Coalescer`.
pub struct Coalescer {
    tx: mpsc::Sender<String>,
}

impl Coalescer {
    pub fn new(timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            while let Ok(mut cmd) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    cmd = newer;
                }
                status(&cmd, timeout);
            }
        });
        Self { tx }
    }

    pub fn run(&self, cmd: String) {
        let _ = self.tx.send(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_values() {
        assert!(truthy(false, "true"));
        assert!(!truthy(true, "false"));
        assert!(truthy(true, ""));
        assert!(!truthy(false, ""));
    }

    #[test]
    fn numbers() {
        assert_eq!(number("Volume: 42%"), Some(42.0));
        assert_eq!(number("0.35"), Some(0.35));
        assert_eq!(number("n/a"), None);
    }

    #[test]
    fn output_runs_shell() {
        let t = Duration::from_secs(5);
        assert_eq!(
            output("echo ' hi ' | tr h H", t),
            Output::Done(true, "Hi".into())
        );
        assert_eq!(output("exit 3", t), Output::Done(false, String::new()));
    }

    /// Is a process with exactly this command line still running?
    fn running(cmdline: &str) -> bool {
        Command::new("pgrep")
            .args(["-fx", cmdline])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn timeout_kills_whole_pipeline() {
        let t = Instant::now();
        let r = output("sleep 30.1 | cat", Duration::from_millis(300));
        assert_eq!(r, Output::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !running("sleep 30.1"),
            "pipeline child keeps running after the timeout"
        );
    }

    #[test]
    fn background_child_does_not_block_output() {
        let t = Instant::now();
        let r = output("sleep 30.2 & echo now", Duration::from_secs(5));
        assert_eq!(r, Output::Done(true, "now".into()));
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !running("sleep 30.2"),
            "read commands leave no background processes behind"
        );
    }

    #[test]
    fn actions_may_start_background_programs() {
        let r = status("sleep 30.3 &", Duration::from_secs(2));
        assert_eq!(r, Output::Done(true, String::new()));
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            running("sleep 30.3"),
            "actions may start programs in the background"
        );
        let _ = Command::new("pkill").args(["-fx", "sleep 30.3"]).status();
    }

    #[test]
    fn hanging_slider_does_not_block_others() {
        let f = std::env::temp_dir().join(format!("touchveil-slider-{}", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let a = Coalescer::new(Duration::from_secs(10));
        let b = Coalescer::new(Duration::from_secs(10));
        a.run("sleep 3".into());
        let t = Instant::now();
        b.run(format!("touch {}", f.display()));
        while !f.exists() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        let _ = std::fs::remove_file(f);
    }
}
