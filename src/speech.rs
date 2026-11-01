//! Spoken cue names (t-m20-announce, D-092): when a tile starts, the system voice says its
//! title, if Settings ▸ Speak Cue Names is on (it's off by default).
//!
//! Speech runs the platform's own speaker as a child process on a thread of its own, so it
//! never waits on the audio or UI thread: macOS `say` (with `-a <device>`, so it goes to
//! the Preview output's device when one is chosen), Linux speech-dispatcher's `spd-say`
//! (which plays through speech-dispatcher's own output; it can't be pointed at a device).
//! A new title cuts off the one still being spoken, so a run of quick cues doesn't queue
//! up behind itself.

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

/// One thing to say, and the output device to say it on (`None`: the system's default).
struct Utterance {
    text: String,
    device: Option<String>,
}

/// The speaking thread; dropping it stops it (and whatever it's saying).
pub struct Speaker {
    tx: Sender<Utterance>,
}

/// The program that speaks here, if there is one.
pub fn program() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("say")
    } else {
        let found = std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join("spd-say").is_file()));
        found.then_some("spd-say")
    }
}

/// Whether speech can go to a chosen device (the Preview output) on this platform.
pub const CAN_ROUTE: bool = cfg!(target_os = "macos");

/// The command that says `text` on `device`.
fn command(program: &str, text: &str, device: Option<&str>) -> Command {
    let mut cmd = Command::new(program);
    if program == "say" {
        if let Some(d) = device {
            cmd.args(["-a", d]);
        }
        // `--` so a title starting with "-" isn't read as an option.
        cmd.args(["--", text]);
    } else {
        // Wait for the speech so killing the child cancels it.
        cmd.args(["--wait", "--", text]);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}

impl Speaker {
    /// A speaker, or `None` when this system has nothing to speak with.
    pub fn new() -> Option<Speaker> {
        let program = program()?;
        let (tx, rx) = channel::<Utterance>();
        std::thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                let mut current: Option<Child> = None;
                loop {
                    let next = match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(u) => Some(u),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    // Reap a finished one; cut off the old one for a new one.
                    if let Some(c) = &mut current {
                        if next.is_some() || c.try_wait().map_or(true, |s| s.is_some()) {
                            let _ = c.kill();
                            let _ = c.wait();
                            current = None;
                        }
                    }
                    if let Some(u) = next {
                        current = command(program, &u.text, u.device.as_deref()).spawn().ok();
                    }
                }
                if let Some(mut c) = current {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            })
            .ok()?;
        Some(Speaker { tx })
    }

    /// Say `text` (cutting off anything still being said), on `device` where speech can be
    /// routed. Returns at once.
    pub fn say(&self, text: &str, device: Option<&str>) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.trim().is_empty() {
            return;
        }
        let device = device.filter(|_| CAN_ROUTE).map(str::to_owned);
        self.tx.send(Utterance { text, device }).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_never_read_as_options() {
        let args = |c: Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(args(command("say", "-rf Air Horn", Some("Headphones"))), ["-a", "Headphones", "--", "-rf Air Horn"]);
        assert_eq!(args(command("say", "Applause", None)), ["--", "Applause"]);
        assert_eq!(args(command("spd-say", "-x", None)), ["--wait", "--", "-x"]);
    }
}
