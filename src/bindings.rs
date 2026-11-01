//! Bindings: external triggers (MIDI now; global keys and OSC later) mapped to registry
//! commands (D-041). A binding is a *source* plus an invocation, so anything the CLI can
//! do can sit on a pad, a key or a fader.
//!
//! This module is pure: the model, matching, MIDI message parsing and the CLI's source
//! shorthand. The MIDI ports themselves are in [`crate::midi`]; the Controller runs what
//! a binding asks for.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// What sets a binding off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A system-wide key combination, e.g. `Ctrl+Alt+1`.
    Key(String),
    Midi(MidiSource),
    /// An OSC address, e.g. `/show/intro`.
    Osc(String),
}

impl Default for Source {
    fn default() -> Self {
        Source::Midi(MidiSource::default())
    }
}

/// A MIDI note, controller (CC) or program change, optionally on one channel or port.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiSource {
    /// Part of the port's name; none matches every port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// 1–16; none matches every channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MidiKind {
    Note(u8),
    Cc(u8),
    Program(u8),
}

/// One incoming MIDI message the app cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiEvent {
    pub port: String,
    /// 1–16.
    pub channel: u8,
    pub kind: MidiKind,
    /// Velocity (notes) or the controller's value (CC), 0–127.
    pub value: u8,
    /// Note on with a velocity, a CC at 64 or more, or a program change.
    pub on: bool,
}

/// Parse one MIDI message. Anything but note on/off, CC and program change is `None`.
pub fn parse_midi(port: &str, bytes: &[u8]) -> Option<MidiEvent> {
    let status = *bytes.first()?;
    let channel = (status & 0x0F) + 1;
    let data = |i: usize| bytes.get(i).map(|b| b & 0x7F);
    let (kind, value, on) = match status & 0xF0 {
        0x90 => {
            let v = data(2)?;
            (MidiKind::Note(data(1)?), v, v > 0)
        }
        0x80 => (MidiKind::Note(data(1)?), 0, false),
        0xB0 => {
            let v = data(2)?;
            (MidiKind::Cc(data(1)?), v, v >= 64)
        }
        0xC0 => (MidiKind::Program(data(1)?), 127, true),
        _ => return None,
    };
    Some(MidiEvent { port: port.to_owned(), channel, kind, value, on })
}

impl MidiSource {
    pub fn kind(&self) -> Option<MidiKind> {
        match (self.note, self.cc, self.program) {
            (Some(n), None, None) => Some(MidiKind::Note(n)),
            (None, Some(c), None) => Some(MidiKind::Cc(c)),
            (None, None, Some(p)) => Some(MidiKind::Program(p)),
            _ => None,
        }
    }

    /// The source a learned event makes: its message and channel, on any port.
    pub fn from_event(ev: &MidiEvent) -> MidiSource {
        let mut s = MidiSource { channel: Some(ev.channel), ..Default::default() };
        match ev.kind {
            MidiKind::Note(n) => s.note = Some(n),
            MidiKind::Cc(c) => s.cc = Some(c),
            MidiKind::Program(p) => s.program = Some(p),
        }
        s
    }

    pub fn matches(&self, ev: &MidiEvent) -> bool {
        self.kind() == Some(ev.kind)
            && self.channel.is_none_or(|c| c == ev.channel)
            && self.port.as_deref().is_none_or(|p| ev.port.to_lowercase().contains(&p.to_lowercase()))
    }
}

impl Source {
    /// Parse the CLI's shorthand or a JSON object:
    /// `midi:note:36`, `midi:cc:7:ch10`, `midi:program:2`, `key:Ctrl+Alt+1`, `osc:/show/intro`,
    /// or `{"midi": {"note": 36, "channel": 10}}`.
    pub fn parse(v: &Value) -> Result<Source, String> {
        if let Some(s) = v.as_str() {
            return Source::parse_str(s);
        }
        let src: Source = serde_json::from_value(v.clone()).map_err(|e| format!("source: {e}"))?;
        src.validate()?;
        Ok(src)
    }

    fn parse_str(s: &str) -> Result<Source, String> {
        let bad = || format!("source {s:?}: use midi:note:36, midi:cc:7:ch10, midi:program:2, key:Ctrl+Alt+1 or osc:/address");
        let (kind, rest) = s.split_once(':').ok_or_else(bad)?;
        let src = match kind.to_ascii_lowercase().as_str() {
            "key" => Source::Key(rest.to_owned()),
            "osc" => Source::Osc(rest.to_owned()),
            "midi" => {
                let mut parts = rest.split(':');
                let what = parts.next().unwrap_or_default().to_ascii_lowercase();
                let n: u8 = parts.next().and_then(|n| n.parse().ok()).ok_or_else(bad)?;
                let mut m = MidiSource::default();
                match what.as_str() {
                    "note" => m.note = Some(n),
                    "cc" => m.cc = Some(n),
                    "program" | "pc" => m.program = Some(n),
                    _ => return Err(bad()),
                }
                for extra in parts {
                    match extra.strip_prefix("ch").and_then(|c| c.parse().ok()) {
                        Some(c) => m.channel = Some(c),
                        None => m.port = Some(extra.to_owned()),
                    }
                }
                Source::Midi(m)
            }
            _ => return Err(bad()),
        };
        src.validate()?;
        Ok(src)
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Source::Midi(m) => {
                if m.kind().is_none() {
                    return Err("a MIDI source needs exactly one of note, cc or program".into());
                }
                if [m.note, m.cc, m.program].iter().flatten().any(|n| *n > 127) {
                    return Err("MIDI notes, controllers and programs are 0–127".into());
                }
                if m.channel.is_some_and(|c| !(1..=16).contains(&c)) {
                    return Err("MIDI channels are 1–16".into());
                }
                Ok(())
            }
            Source::Key(k) if k.trim().is_empty() => Err("a key source needs a key, e.g. key:Ctrl+Alt+1".into()),
            Source::Osc(a) if !a.starts_with('/') => Err("an OSC address starts with /".into()),
            _ => Ok(()),
        }
    }

    /// Short, human: `Note 36 · ch 10`, `CC 7`, `Ctrl+Alt+1`, `OSC /show/intro`.
    pub fn describe(&self) -> String {
        match self {
            Source::Key(k) => k.clone(),
            Source::Osc(a) => format!("OSC {a}"),
            Source::Midi(m) => {
                let mut s = match m.kind() {
                    Some(MidiKind::Note(n)) => format!("Note {n} ({})", note_name(n)),
                    Some(MidiKind::Cc(c)) => format!("CC {c}"),
                    Some(MidiKind::Program(p)) => format!("Program {p}"),
                    None => "MIDI".into(),
                };
                if let Some(c) = m.channel {
                    s.push_str(&format!(" · ch {c}"));
                }
                if let Some(p) = &m.port {
                    s.push_str(&format!(" · {p}"));
                }
                s
            }
        }
    }
}

/// `C4` for 60 (middle C, as most controllers label it).
pub fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[n as usize % 12], n as i32 / 12 - 1)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub id: Uuid,
    pub source: Source,
    /// A registry command id.
    pub command: String,
    pub params: Map<String, Value>,
    /// For a fader or knob: the parameter its 0–1 position fills (e.g. `volume`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
}

impl Default for Binding {
    fn default() -> Self {
        Binding { id: Uuid::new_v4(), source: Source::default(), command: String::new(), params: Map::new(), value: None, enabled: true }
    }
}

/// How a source fired.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fire {
    Press,
    Release,
    /// A fader or knob position, 0–1.
    Value(f32),
}

/// Commands that take a press and a release, and the parameter that carries which.
fn press_release(command: &str) -> Option<(&'static str, Value, Value)> {
    match command {
        "tile.trigger" => Some(("event", json!("press"), json!("release"))),
        "duck.hold" => Some(("on", json!(true), json!(false))),
        _ => None,
    }
}

impl Binding {
    /// The command to run when the source fires this way, if any. Hold-style commands
    /// (`tile.trigger`, `duck.hold`) get press and release; a binding with a `value`
    /// takes fader positions; everything else runs on press.
    pub fn invocation(&self, fire: Fire) -> Option<(String, Value)> {
        let mut params = self.params.clone();
        match (fire, &self.value, press_release(&self.command)) {
            (Fire::Value(v), Some(key), _) => {
                params.insert(key.clone(), json!((v as f64 * 1000.0).round() / 1000.0));
            }
            (Fire::Value(_), None, _) => return None,
            (_, Some(_), _) => return None,
            (fire, None, Some((key, press, release))) => {
                params.insert(key.into(), if fire == Fire::Press { press } else { release });
            }
            (Fire::Press, None, None) => {}
            (Fire::Release, None, None) => return None,
        }
        Some((self.command.clone(), Value::Object(params)))
    }

    /// For `bind.list`.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "source": self.source,
            "what": self.source.describe(),
            "command": self.command,
            "params": self.params,
            "value": self.value,
            "enabled": self.enabled,
        })
    }
}

/// MIDI input settings, kept in the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiSettings {
    pub enabled: bool,
    /// Parts of port names to listen to; empty listens to every port.
    pub ports: Vec<String>,
    /// Without a binding of its own, note `auto_map + k` plays tile #k+1 in the current set.
    pub auto_map: Option<u8>,
    /// Light pads: send each bound note back to the controller, with `lit` velocity while
    /// its tile plays and `unlit` otherwise (many pads read velocity as a colour).
    pub feedback: bool,
    pub lit: u8,
    pub unlit: u8,
    /// The channel for auto-mapped pads' lights (1-16).
    pub feedback_channel: u8,
    /// Auto-mapped pads per page (`midi.page`); 0 maps every note from `auto_map` up.
    pub bank: u8,
    /// Pad colours by tile colour (src/pads.rs); none uses `lit` and `unlit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<crate::pads::Palette>,
}

/// Port names for the outputs that light an input's pads: the same name, or the same
/// device before ALSA's `:` (`nanoPAD2:nanoPAD2 MIDI 1 20:0`).
pub fn output_matches(input: &str, output: &str) -> bool {
    let device = |s: &str| s.split(':').next().unwrap_or(s).trim().to_lowercase();
    input == output || device(input) == device(output)
}

impl Default for MidiSettings {
    fn default() -> Self {
        MidiSettings { enabled: true, ports: Vec::new(), auto_map: None, feedback: false, lit: 127, unlit: 0, feedback_channel: 1, bank: 0, palette: None }
    }
}

impl MidiSettings {
    pub fn wants(&self, port: &str) -> bool {
        self.enabled && (self.ports.is_empty() || self.ports.iter().any(|p| port.to_lowercase().contains(&p.to_lowercase())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_midi_messages() {
        let on = parse_midi("Pads", &[0x99, 36, 100]).unwrap();
        assert_eq!((on.channel, on.kind, on.value, on.on), (10, MidiKind::Note(36), 100, true));
        assert!(!parse_midi("Pads", &[0x99, 36, 0]).unwrap().on, "note on at velocity 0 is a note off");
        assert!(!parse_midi("Pads", &[0x80, 36, 64]).unwrap().on);
        let cc = parse_midi("Pads", &[0xB0, 7, 100]).unwrap();
        assert_eq!((cc.channel, cc.kind, cc.on), (1, MidiKind::Cc(7), true));
        assert_eq!(parse_midi("Pads", &[0xC2, 5]).unwrap().kind, MidiKind::Program(5));
        assert_eq!(parse_midi("Pads", &[0xF8]), None, "clock is ignored");
        assert_eq!(parse_midi("Pads", &[0x90, 36]), None, "short message");
    }

    #[test]
    fn sources_parse_match_and_describe() {
        let s = Source::parse(&json!("midi:note:36:ch10")).unwrap();
        assert_eq!(s, Source::Midi(MidiSource { note: Some(36), channel: Some(10), ..Default::default() }));
        assert_eq!(s.describe(), "Note 36 (C2) · ch 10");
        assert_eq!(Source::parse(&json!({"midi": {"cc": 7}})).unwrap().describe(), "CC 7");
        assert_eq!(Source::parse(&json!("key:Ctrl+Alt+1")).unwrap(), Source::Key("Ctrl+Alt+1".into()));
        assert!(Source::parse(&json!("midi:note:200")).is_err());
        assert!(Source::parse(&json!("midi:note:3:ch17")).is_err());
        assert!(Source::parse(&json!({"midi": {"note": 1, "cc": 2}})).is_err());
        assert!(Source::parse(&json!("osc:nope")).is_err());
        assert!(Source::parse(&json!("pedal:1")).is_err());

        let Source::Midi(m) = Source::parse(&json!("midi:note:36:nanoPAD")).unwrap() else { panic!() };
        let ev = parse_midi("nanoPAD2 PAD", &[0x99, 36, 90]).unwrap();
        assert!(m.matches(&ev));
        assert!(!m.matches(&parse_midi("LPD8", &[0x99, 36, 90]).unwrap()), "other port");
        assert!(!m.matches(&parse_midi("nanoPAD2", &[0x99, 37, 90]).unwrap()), "other note");
        assert_eq!(MidiSource::from_event(&ev), MidiSource { note: Some(36), channel: Some(10), ..Default::default() });
    }

    #[test]
    fn bindings_turn_fires_into_commands() {
        let pad = Binding { command: "tile.trigger".into(), params: json!({"tile": "Air Horn"}).as_object().unwrap().clone(), ..Default::default() };
        assert_eq!(pad.invocation(Fire::Press).unwrap().1, json!({"tile": "Air Horn", "event": "press"}));
        assert_eq!(pad.invocation(Fire::Release).unwrap().1, json!({"tile": "Air Horn", "event": "release"}));
        assert_eq!(pad.invocation(Fire::Value(0.5)), None);

        let go = Binding { command: "cue.go".into(), ..Default::default() };
        assert_eq!(go.invocation(Fire::Press).unwrap(), ("cue.go".into(), json!({})));
        assert_eq!(go.invocation(Fire::Release), None, "plain commands run on press only");

        let talk = Binding { command: "duck.hold".into(), ..Default::default() };
        assert_eq!(talk.invocation(Fire::Release).unwrap().1, json!({"on": false}));

        let fader = Binding { command: "master.volume".into(), value: Some("volume".into()), ..Default::default() };
        assert_eq!(fader.invocation(Fire::Value(100.0 / 127.0)).unwrap().1, json!({"volume": 0.787}));
        assert_eq!(fader.invocation(Fire::Press), None);
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(36), "C2");
        assert_eq!(note_name(0), "C-1");
        assert!(MidiSettings::default().wants("anything"));
        let only = MidiSettings { ports: vec!["nano".into()], ..Default::default() };
        assert!(only.wants("nanoKONTROL2") && !only.wants("LPD8"));
        assert!(output_matches("Launchpad Mini", "Launchpad Mini"));
        assert!(output_matches("nanoPAD2:nanoPAD2 MIDI 1 20:0", "nanoPAD2:nanoPAD2 MIDI 1 20:0"));
        assert!(output_matches("LPD8:LPD8 MIDI 1 24:0", "LPD8:LPD8 MIDI 1 24:1"));
        assert!(!output_matches("LPD8", "nanoPAD2"));
    }
}
