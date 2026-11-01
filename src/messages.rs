//! Messages (D-055): OSC or MIDI messages a tile sends when it starts and when it stops,
//! to cue lights, video or another app. Each message is one line of text, stored on the tile
//! as given and checked when it's set:
//!
//! ```text
//! osc://127.0.0.1:53000/cue/5/start          an OSC message, no arguments
//! osc://localhost:8000/light/scene 3 0.5 "Act one"   int, float and string arguments
//! midi:note:60:ch1:vel100 @Lights            note on (velocity 127 unless given)
//! midi:noteoff:60:ch1                        note off
//! midi:cc:20:ch16:val127                     controller (value 127 unless given)
//! midi:program:5:ch2                         program change
//! ```
//!
//! A MIDI message goes to every output whose name contains the text after `@` (any case), or
//! to every output without one. OSC messages follow D-042: they go to this machine only,
//! unless OSC's "from other machines too" setting (`osc.enable --lan true`) is on.
//!
//! Sending happens on its own thread: name lookups and opening MIDI ports never hold up
//! the window or the controller, and nothing here touches the audio thread.

use crate::audio::VoiceStatus;
use crate::logic::PlayState;
use rosc::{OscPacket, OscType};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use uuid::Uuid;

/// One parsed message.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Osc { host: String, port: u16, address: String, args: Vec<OscType> },
    Midi { port: Option<String>, bytes: Vec<u8> },
}

/// Parse one message (see the module docs for the forms).
pub fn parse(s: &str) -> Result<Action, String> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("osc://") {
        parse_osc(rest)
    } else if let Some(rest) = s.strip_prefix("midi:") {
        parse_midi(rest)
    } else {
        Err(format!("{s:?}: a message starts with osc:// or midi:"))
    }
}

fn parse_osc(rest: &str) -> Result<Action, String> {
    let (target, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let slash = target.find('/').ok_or_else(|| format!("osc://{target}: no address (osc://host:port/address)"))?;
    let (hostport, address) = target.split_at(slash);
    let (host, port) = hostport.rsplit_once(':').ok_or_else(|| format!("osc://{hostport}: no port (osc://host:port/address)"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port: u16 = port.parse().ok().filter(|p| *p > 0).ok_or_else(|| format!("osc://{hostport}: the port is 1-65535"))?;
    if host.is_empty() {
        return Err(format!("osc://{hostport}: no host"));
    }
    if address.len() < 2 || address.contains(char::is_whitespace) {
        return Err(format!("osc://{target}: the address is a path such as /cue/1/go"));
    }
    Ok(Action::Osc { host: host.to_owned(), port, address: address.to_owned(), args: osc_args(args)? })
}

/// Whitespace-separated arguments: whole numbers are ints, other numbers floats, the
/// rest strings; "double quotes" keep spaces in a string.
fn osc_args(s: &str) -> Result<Vec<OscType>, String> {
    let mut out = Vec::new();
    let mut chars = s.trim().chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' {
            chars.next();
            let word: String = chars.by_ref().take_while(|&c| c != '"').collect();
            out.push(OscType::String(word));
            continue;
        }
        let word: String = std::iter::from_fn(|| chars.next_if(|c| !c.is_whitespace())).collect();
        out.push(if let Ok(i) = word.parse::<i32>() {
            OscType::Int(i)
        } else if let Some(f) = word.parse::<f32>().ok().filter(|f| f.is_finite()) {
            OscType::Float(f)
        } else {
            OscType::String(word)
        });
    }
    if s.matches('"').count() % 2 == 1 {
        return Err(format!("{s:?}: a quote isn't closed"));
    }
    Ok(out)
}

fn parse_midi(rest: &str) -> Result<Action, String> {
    let (msg, port) = match rest.split_once('@') {
        Some((m, p)) => (m.trim(), Some(p.trim().to_owned()).filter(|p| !p.is_empty())),
        None => (rest.trim(), None),
    };
    let bad = || format!("midi:{msg}: expected midi:note:60, midi:noteoff:60, midi:cc:20 or midi:program:5, then :chN and :velN or :valN");
    let mut parts = msg.split(':');
    let kind = parts.next().unwrap_or_default().to_ascii_lowercase();
    let num: u8 = parts.next().and_then(|n| n.parse().ok()).filter(|n| *n <= 127).ok_or_else(bad)?;
    let (mut ch, mut value) = (1u8, 127u8);
    for p in parts {
        let p = p.to_ascii_lowercase();
        if let Some(c) = p.strip_prefix("ch") {
            ch = c.parse().ok().filter(|c| (1..=16).contains(c)).ok_or_else(|| format!("midi:{msg}: the channel is ch1-ch16"))?;
        } else if let Some(v) = p.strip_prefix("vel").or_else(|| p.strip_prefix("val")) {
            value = v.parse().ok().filter(|v| *v <= 127).ok_or_else(|| format!("midi:{msg}: the value is 0-127"))?;
        } else {
            return Err(bad());
        }
    }
    let c = ch - 1;
    let bytes = match kind.as_str() {
        "note" => vec![0x90 | c, num, value],
        "noteoff" => vec![0x80 | c, num, 0],
        "cc" => vec![0xB0 | c, num, value],
        "program" | "pc" => vec![0xC0 | c, num],
        _ => return Err(bad()),
    };
    Ok(Action::Midi { port, bytes })
}

/// Check every message in a list, naming the first bad one.
pub fn validate(list: &[String]) -> Result<(), String> {
    list.iter().try_for_each(|s| parse(s).map(|_| ()))
}

/// A literal address that isn't this machine (a host name is checked when it's sent).
pub fn is_remote_literal(a: &Action) -> bool {
    matches!(a, Action::Osc { host, .. } if host.parse::<std::net::IpAddr>().is_ok_and(|ip| !ip.is_loopback()))
}

/// A message on its way, with whether OSC may leave this machine.
struct Job {
    action: Action,
    lan: bool,
}

/// What the controller keeps: the sending thread (started on the first message), the tiles
/// it saw sounding last tick, and the ones playing as a preview (they send nothing).
#[derive(Default)]
pub struct Tracker {
    worker: Option<(Sender<Job>, Receiver<String>)>,
    seen: HashMap<Uuid, PlayState>,
    pub previews: HashSet<Uuid>,
    /// How many messages went to the thread.
    pub queued: u64,
}

impl Tracker {
    /// Compare this tick's voices with the last: tiles that started and tiles that
    /// stopped. A tile starts when its voice appears (or plays again while fading out)
    /// and stops when its voice is gone; pausing and resuming are neither.
    pub fn transitions(&mut self, statuses: &HashMap<Uuid, VoiceStatus>) -> (Vec<Uuid>, Vec<Uuid>) {
        let started = statuses
            .iter()
            .filter(|(id, s)| match self.seen.get(id) {
                None => true,
                Some(PlayState::Stopping) => s.state == PlayState::Playing,
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect();
        let stopped = self.seen.keys().filter(|id| !statuses.contains_key(id)).copied().collect();
        self.seen = statuses.iter().map(|(id, s)| (*id, s.state)).collect();
        (started, stopped)
    }

    pub fn send(&mut self, action: Action, lan: bool) {
        let (tx, _) = self.worker.get_or_insert_with(start);
        if tx.send(Job { action, lan }).is_ok() {
            self.queued += 1;
        }
    }

    /// Errors from the sending thread since the last call.
    pub fn errors(&self) -> Vec<String> {
        self.worker.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default()
    }
}

fn start() -> (Sender<Job>, Receiver<String>) {
    let (tx, rx) = channel::<Job>();
    let (etx, erx) = channel::<String>();
    std::thread::Builder::new()
        .name("messages".into())
        .spawn(move || {
            let mut udp: HashMap<&'static str, UdpSocket> = HashMap::new();
            let mut midi = MidiOuts::default();
            for job in rx {
                let result = match job.action {
                    Action::Osc { host, port, address, args } => send_osc(&mut udp, &host, port, address, args, job.lan),
                    Action::Midi { port, bytes } => midi.send(port.as_deref(), &bytes),
                };
                if let Err(e) = result {
                    etx.send(e).ok();
                }
            }
        })
        .ok();
    (tx, erx)
}

fn send_osc(udp: &mut HashMap<&'static str, UdpSocket>, host: &str, port: u16, address: String, args: Vec<OscType>, lan: bool) -> Result<(), String> {
    // IPv4 first: "localhost" is often ::1 first, and most OSC apps listen on IPv4 only.
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().map(Iterator::collect).unwrap_or_default();
    let to = *addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).ok_or_else(|| format!("OSC message: can't find {host}"))?;
    let local = to.ip().is_loopback();
    if !local && !lan {
        return Err(format!("OSC message to {host}:{port} refused: messages stay on this machine unless OSC may use the network (osc.enable --lan true)"));
    }
    let bind = match (to.is_ipv4(), local) {
        (true, true) => "127.0.0.1:0",
        (true, false) => "0.0.0.0:0",
        (false, true) => "[::1]:0",
        (false, false) => "[::]:0",
    };
    if !udp.contains_key(bind) {
        udp.insert(bind, UdpSocket::bind(bind).map_err(|e| format!("OSC message: {e}"))?);
    }
    let sock = &udp[bind];
    let bytes = rosc::encoder::encode(&OscPacket::Message(rosc::OscMessage { addr: address, args })).map_err(|e| format!("OSC message: {e}"))?;
    sock.send_to(&bytes, to).map(|_| ()).map_err(|e| format!("OSC message to {host}:{port}: {e}"))
}

/// MIDI outputs opened for messages, by port name, kept open between them.
#[derive(Default)]
struct MidiOuts {
    open: HashMap<String, midir::MidiOutputConnection>,
}

impl MidiOuts {
    fn send(&mut self, port: Option<&str>, bytes: &[u8]) -> Result<(), String> {
        // So an output plugged in after launch is in the list (macOS).
        crate::midi::init();
        let probe = midir::MidiOutput::new(crate::brand::SLUG).map_err(|e| format!("MIDI message: MIDI is unavailable: {e}"))?;
        let names: Vec<String> = probe.ports().iter().filter_map(|p| probe.port_name(p).ok()).collect();
        let wanted: Vec<&String> = names.iter().filter(|n| port.is_none_or(|p| n.to_lowercase().contains(&p.to_lowercase()))).collect();
        if wanted.is_empty() {
            return Err(match port {
                Some(p) => format!("MIDI message: no output called {p:?}"),
                None => "MIDI message: there is no MIDI output".to_owned(),
            });
        }
        self.open.retain(|name, _| names.contains(name));
        let mut errors = Vec::new();
        for name in wanted {
            if !self.open.contains_key(name) {
                let out = midir::MidiOutput::new(crate::brand::SLUG).map_err(|e| e.to_string())?;
                let Some(p) = out.ports().into_iter().find(|p| out.port_name(p).ok().as_deref() == Some(name.as_str())) else { continue };
                match out.connect(&p, &format!("{}-messages", crate::brand::SLUG)) {
                    Ok(conn) => {
                        self.open.insert(name.clone(), conn);
                    }
                    Err(e) => errors.push(format!("{name}: {e}")),
                }
            }
            if let Some(conn) = self.open.get_mut(name) {
                if let Err(e) = conn.send(bytes) {
                    errors.push(format!("{name}: {e}"));
                    self.open.remove(name);
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("MIDI message: {}", errors.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_osc_and_midi_messages() {
        let a = parse(r#"osc://127.0.0.1:53000/cue/5/start 3 0.5 "Act one" go"#).unwrap();
        assert_eq!(
            a,
            Action::Osc {
                host: "127.0.0.1".into(),
                port: 53000,
                address: "/cue/5/start".into(),
                args: vec![OscType::Int(3), OscType::Float(0.5), OscType::String("Act one".into()), OscType::String("go".into())],
            }
        );
        assert!(matches!(parse("osc://[::1]:9000/x").unwrap(), Action::Osc { host, .. } if host == "::1"));
        assert_eq!(parse("midi:note:60:ch2:vel100 @Lights").unwrap(), Action::Midi { port: Some("Lights".into()), bytes: vec![0x91, 60, 100] });
        assert_eq!(parse("midi:note:60").unwrap(), Action::Midi { port: None, bytes: vec![0x90, 60, 127] });
        assert_eq!(parse("midi:noteoff:60:ch16").unwrap(), Action::Midi { port: None, bytes: vec![0x8F, 60, 0] });
        assert_eq!(parse("midi:cc:20:val0").unwrap(), Action::Midi { port: None, bytes: vec![0xB0, 20, 0] });
        assert_eq!(parse("midi:program:5:ch3").unwrap(), Action::Midi { port: None, bytes: vec![0xC2, 5] });
        for bad in ["osc://host/x", "osc://host:0/x", "osc://:9000/x", "osc://h:9000", "osc://h:9000/ ", "osc://h:1/x \"open", "midi:note:200", "midi:cc:1:ch17", "midi:bend:1", "http://x", ""] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(is_remote_literal(&parse("osc://192.168.1.20:8000/x").unwrap()));
        assert!(!is_remote_literal(&parse("osc://localhost:8000/x").unwrap()));
        assert!(!is_remote_literal(&parse("osc://127.0.0.1:8000/x").unwrap()));
    }

    #[test]
    fn transitions_skip_pause_and_resume() {
        let st = |state| VoiceStatus { state, position: 0.0, length: 1.0, speed: 1.0, vamping: false, played_on: false, from: 0.0 };
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut t = Tracker::default();
        let (s, e) = t.transitions(&HashMap::from([(a, st(PlayState::Playing))]));
        assert_eq!((s, e), (vec![a], vec![]));
        let (s, e) = t.transitions(&HashMap::from([(a, st(PlayState::Paused)), (b, st(PlayState::Playing))]));
        assert_eq!((s, e), (vec![b], vec![]));
        let (s, e) = t.transitions(&HashMap::from([(a, st(PlayState::Playing)), (b, st(PlayState::Stopping))]));
        assert_eq!((s, e), (vec![], vec![]), "resume and fade-out are neither");
        let (s, e) = t.transitions(&HashMap::from([(b, st(PlayState::Playing))]));
        assert_eq!((s, e), (vec![b], vec![a]), "played again while fading out; a is gone");
    }

    #[test]
    fn osc_messages_stay_local_unless_lan() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        let port = rx.local_addr().unwrap().port();
        let mut t = Tracker::default();
        t.send(parse(&format!("osc://127.0.0.1:{port}/light/go 7")).unwrap(), false);
        let mut buf = [0u8; 512];
        let (n, _) = rx.recv_from(&mut buf).unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buf[..n]).unwrap();
        assert_eq!(packet, OscPacket::Message(rosc::OscMessage { addr: "/light/go".into(), args: vec![OscType::Int(7)] }));
        // TEST-NET-1 (RFC 5737): never routed, so nothing leaves even if it were allowed.
        t.send(parse("osc://192.0.2.1:9000/x").unwrap(), false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let errors = loop {
            let e = t.errors();
            if !e.is_empty() || std::time::Instant::now() > deadline {
                break e;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(errors.iter().any(|e| e.contains("refused")), "{errors:?}");
    }
}
