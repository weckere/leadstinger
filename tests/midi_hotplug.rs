//! End-to-end MIDI hot-plug: `leadstinger-cli serve` sees a port that appears after launch,
//! drops it when it goes, and sees it again when it comes back, through the 2 s rescan.
//! The "controller" is a child process (this test binary again) that publishes a virtual
//! source and a virtual destination, like a USB keyboard plugged in and pulled out. The
//! destination prints what it receives, which checks a tile's on-start MIDI message too.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const KEYS: &str = "LS Hotplug Keys";
const SYNTH: &str = "LS Hotplug Synth";
const HELPER: &str = "LEADSTINGER_MIDI_HELPER";

/// The child's side: publish the ports, print what the destination receives, and live
/// until killed.
#[test]
#[ignore = "run by midi_ports_come_and_go as its plugged-in controller"]
fn helper_controller() {
    if std::env::var_os(HELPER).is_none() {
        return;
    }
    use midir::os::unix::{VirtualInput, VirtualOutput};
    let _keys = midir::MidiOutput::new("LS Hotplug").unwrap().create_virtual(KEYS).unwrap();
    let _synth = midir::MidiInput::new("LS Hotplug")
        .unwrap()
        .create_virtual(SYNTH, |_, bytes, _| println!("got {bytes:?}"), ())
        .unwrap();
    println!("ready");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// A child process, killed when dropped, so a failed assertion leaves nothing running.
struct Kid(Child);

impl Drop for Kid {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Start the controller and wait until its ports exist. Its stdout lines arrive on the
/// receiver.
fn plug_in() -> (Kid, Receiver<String>) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["helper_controller", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(HELPER, "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        for line in out.lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let kid = Kid(child);
    wait_line(&lines, "ready");
    (kid, lines)
}

fn wait_line(lines: &Receiver<String>, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) if line.contains(want) => return line,
            Ok(_) => {}
            Err(e) => panic!("no {want:?} from the controller: {e}"),
        }
    }
}

fn cli(data: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .arg("--data-dir")
        .arg(data)
        .arg("--json")
        .args(args)
        .env_remove("LEADSTINGER_DATA_DIR")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().last().and_then(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null)
}

fn has(v: &Value, field: &str, name: &str) -> bool {
    v[field].as_array().is_some_and(|a| a.iter().any(|p| p.as_str().is_some_and(|p| p.contains(name))))
}

/// Wait for `midi.ports` to show (or stop showing) the keys as a port and as connected.
/// The rescan runs every 2 s; 6 s leaves room for a busy machine.
fn wait_ports(data: &Path, present: bool, what: &str) -> Duration {
    let start = Instant::now();
    loop {
        let v = cli(data, &["midi.ports"]);
        if has(&v, "ports", KEYS) == present && has(&v, "connected", KEYS) == present {
            return start.elapsed();
        }
        assert!(start.elapsed() < Duration::from_secs(6), "{what}: midi.ports still says {v}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn midi_ports_come_and_go() {
    if midir::MidiInput::new("LS Hotplug probe").is_err() {
        eprintln!("no MIDI on this machine (no ALSA sequencer?): skipped");
        return;
    }
    let root = PathBuf::from("/tmp").join(format!("leadstinger-midi-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    let data = root.join("data");
    std::fs::create_dir_all(&root).unwrap();
    let server = Kid(Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .args(["--data-dir", data.to_str().unwrap(), "serve", "--silent"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !leadstinger::control::app_running(&data) {
        assert!(Instant::now() < deadline, "serve never answered");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Let the first scan, made before anything is plugged in, happen.
    std::thread::sleep(Duration::from_millis(500));
    assert!(!has(&cli(&data, &["midi.ports"]), "ports", KEYS), "the keys are there before they're plugged in");

    let (keys, lines) = plug_in();
    eprintln!("plugged in: seen after {:?}", wait_ports(&data, true, "plugged in after launch"));

    // A tile's on-start message reaches an output that appeared after launch.
    let wav = root.join("Beep.wav");
    leadstinger::wav::write_tone(&wav, 8000, 1.0, &[440.0], 0.4).unwrap();
    let r = cli(&data, &["add", wav.to_str().unwrap()]);
    assert!(r["error"].is_null(), "add: {r}");
    let r = cli(&data, &["tile.set", "Beep", "--on-start", &format!("midi:note:60:ch1:vel100 @{SYNTH}")]);
    assert!(r["error"].is_null(), "tile.set: {r}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let r = cli(&data, &["trigger", "Beep"]);
        if r["state"] == "playing" {
            break;
        }
        assert!(Instant::now() < deadline, "never played: {r}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(wait_line(&lines, "got").contains("[144, 60, 100]"), "the note-on reached the synth");

    drop(keys);
    eprintln!("unplugged: gone after {:?}", wait_ports(&data, false, "unplugged"));

    let (keys, _lines) = plug_in();
    eprintln!("replugged: seen after {:?}", wait_ports(&data, true, "replugged"));
    drop(keys);
    drop(server);
    std::fs::remove_dir_all(root).ok();
}
