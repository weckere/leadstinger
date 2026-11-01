//! MIDI input: listens to every port (or the ones `midi.enable` names) and hands each
//! note, controller and program change to the Controller through the [`Lender`], like
//! the control socket, so pads work while the window is hidden. Ports are rescanned every
//! two seconds, so a controller plugged in mid-show starts working by itself.
//!
//! CoreMIDI on macOS, the ALSA sequencer on Linux (both through `midir`). Where neither is
//! available (no /dev/snd/seq in a container, say) the app runs on without MIDI and
//! `midi.ports` says why.
//!
//! CoreMIDI keeps a copy of the device list in each process and updates it only through
//! notifications on the run loop of the thread that made the process's first MIDI client.
//! Neither the rescan thread nor `leadstinger-cli serve` runs one, so [`init`] makes that
//! first client on a thread of its own that does (D-123); without it a port plugged in after
//! launch never shows up.

use crate::bindings::{output_matches, parse_midi};
use crate::control::Lender;
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the input thread last saw, for `midi.ports` and Settings.
#[derive(Clone, Debug, Default)]
pub struct MidiStatus {
    /// Every input port on the system.
    pub ports: Vec<String>,
    /// The ones listened to.
    pub connected: Vec<String>,
    pub error: Option<String>,
}

const RESCAN: Duration = Duration::from_secs(2);

/// Start listening. `wake` (the window's egui context) repaints after each message, so a
/// pad's tile lights up at once.
pub fn start(lender: Arc<Lender>, wake: Option<eframe::egui::Context>) {
    init();
    let (tx, rx) = channel::<(String, Vec<u8>)>();
    let to_controller = lender.clone();
    // Pad lights: the Controller sends messages here, and they go to every output
    // that belongs to a connected controller.
    let (out_tx, out_rx) = channel::<Vec<u8>>();
    lender.with(|c| c.midi_out = Some(out_tx));
    let outs: Arc<Mutex<HashMap<String, MidiOutputConnection>>> = Arc::default();
    let outs_send = outs.clone();
    std::thread::Builder::new()
        .name("midi-out".into())
        .spawn(move || {
            for msg in out_rx {
                for conn in outs_send.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
                    conn.send(&msg).ok();
                }
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("midi-dispatch".into())
        .spawn(move || {
            for (port, bytes) in rx {
                if let Some(ev) = parse_midi(&port, &bytes) {
                    to_controller.with(|c| c.midi(&ev));
                    if let Some(ctx) = &wake {
                        ctx.request_repaint();
                    }
                }
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("midi-ports".into())
        .spawn(move || {
            let mut conns: HashMap<String, MidiInputConnection<()>> = HashMap::new();
            loop {
                let settings = lender.with(|c| c.store.library.midi.clone());
                let status = match MidiInput::new(crate::brand::SLUG) {
                    Err(e) => {
                        conns.clear();
                        MidiStatus { error: Some(format!("MIDI is unavailable: {e}")), ..Default::default() }
                    }
                    Ok(input) => {
                        let ports: Vec<String> = input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect();
                        conns.retain(|name, _| ports.contains(name) && settings.wants(name));
                        let mut error = None;
                        let new: Vec<&String> = ports.iter().filter(|n| settings.wants(n) && !conns.contains_key(*n)).collect();
                        for name in new {
                            match connect(name, tx.clone()) {
                                Ok(conn) => {
                                    conns.insert(name.clone(), conn);
                                }
                                Err(e) => error = Some(format!("Couldn't open {name}: {e}")),
                            }
                        }
                        let mut connected: Vec<String> = conns.keys().cloned().collect();
                        connected.sort();
                        sync_outputs(&outs, &connected, settings.feedback);
                        MidiStatus { ports, connected, error }
                    }
                };
                lender.with(|c| c.midi_status = status);
                std::thread::sleep(RESCAN);
            }
        })
        .ok();
}

/// Make CoreMIDI follow ports that come and go (see the module notes). Call it before the
/// process makes any other MIDI client; later calls do nothing. Nothing to do on Linux,
/// where every ALSA sequencer query is current.
pub fn init() {
    #[cfg(target_os = "macos")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let (ready, made) = channel();
            let spawned = std::thread::Builder::new().name("midi-runloop".into()).spawn(move || {
                // The process's first client, kept for the run: its thread's run loop is
                // the one CoreMIDI notifies.
                let first = MidiInput::new(crate::brand::SLUG);
                ready.send(()).ok();
                if first.is_err() {
                    return;
                }
                loop {
                    use core_foundation_sys::runloop::{kCFRunLoopDefaultMode, kCFRunLoopRunFinished, CFRunLoopRunInMode};
                    // SAFETY: runs this thread's own run loop; the mode is a CoreFoundation constant.
                    if unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 60.0, 0) } == kCFRunLoopRunFinished {
                        // No source to wait on: don't spin.
                        std::thread::sleep(RESCAN);
                    }
                }
            });
            if spawned.is_ok() {
                made.recv().ok();
            }
        });
    }
}

/// Keep an output open for each connected input while pad lights are on.
fn sync_outputs(outs: &Mutex<HashMap<String, MidiOutputConnection>>, inputs: &[String], on: bool) {
    let mut outs = outs.lock().unwrap_or_else(|e| e.into_inner());
    if !on || inputs.is_empty() {
        outs.clear();
        return;
    }
    let Ok(probe) = MidiOutput::new(crate::brand::SLUG) else { return };
    let names: Vec<String> = probe.ports().iter().filter_map(|p| probe.port_name(p).ok()).collect();
    let wanted: Vec<&String> = names.iter().filter(|o| inputs.iter().any(|i| output_matches(i, o))).collect();
    outs.retain(|name, _| wanted.contains(&name));
    for name in wanted {
        if outs.contains_key(name) {
            continue;
        }
        let Ok(out) = MidiOutput::new(crate::brand::SLUG) else { continue };
        let Some(port) = out.ports().into_iter().find(|p| out.port_name(p).ok().as_deref() == Some(name.as_str())) else { continue };
        if let Ok(conn) = out.connect(&port, &format!("{}-lights", crate::brand::SLUG)) {
            outs.insert(name.clone(), conn);
        }
    }
}

fn connect(name: &str, tx: std::sync::mpsc::Sender<(String, Vec<u8>)>) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new(crate::brand::SLUG).map_err(|e| e.to_string())?;
    // Clock, active sensing and SysEx would only wake the dispatcher for nothing.
    input.ignore(Ignore::All);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).ok().as_deref() == Some(name))
        .ok_or_else(|| "it went away".to_owned())?;
    let label = name.to_owned();
    input
        .connect(&port, &format!("{}-in", crate::brand::SLUG), move |_, bytes, _| {
            tx.send((label.clone(), bytes.to_vec())).ok();
        }, ())
        .map_err(|e| e.to_string())
}
