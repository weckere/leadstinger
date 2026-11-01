//! OSC control (D-042): a UDP listener for TouchOSC, Open Stage Control, Bitfocus
//! Companion and the like. It's the app's only network listener, so it is off until
//! turned on, listens on 127.0.0.1 unless `lan` is set, and refuses what `--safe` refuses
//! (destructive commands, quitting) as well as files, bindings and its own settings.
//!
//! Addresses (positions are 1-based in the current set; an argument of 1 or more is a
//! press, below 1 a release):
//!
//! ```text
//! /ls/tile/3/trigger [f]     tile #3, as its hotkey would (press/release)
//! /ls/key/q [f]              the tile with hotkey q (/ls/key/alt+q with Option)
//! /ls/trigger s:ref [f]      a tile by name, id or Set/Title
//! /ls/play s:ref   /ls/stop s:ref   /ls/playon s:ref (or /ls/release)   /ls/jump s:ref s:marker
//! /ls/go   /ls/stopall [f: 1 = at once]   /ls/master f(0-1)   /ls/duck f(1/0)
//! /ls/cmd s:command [s:json params]       any other allowed command
//! /ls/resync                 send the whole feedback state again now
//! ```
//!
//! Bindings with an `osc:/address` source take any other address.

use crate::control::Lender;
use rosc::{OscPacket, OscType};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

/// OSC settings, kept in the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OscSettings {
    pub enabled: bool,
    pub port: u16,
    /// Listen on every network interface, not just this machine.
    pub lan: bool,
    /// Send state changes here (`host:port`), for Companion or TouchOSC feedback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Send each playing tile's level with the feedback (`/ls/tile/N/level`, D-121).
    pub levels: bool,
}

impl Default for OscSettings {
    fn default() -> Self {
        OscSettings { enabled: false, port: 9000, lan: false, feedback: None, levels: true }
    }
}

/// How often feedback samples the tiles' levels, in seconds: ten times a second, as a
/// playing tile's remaining time changes, however fast the window ticks.
pub const LEVEL_EVERY: f64 = 0.1;

/// What the listener last did, for `osc.status` and Settings.
#[derive(Clone, Debug, Default)]
pub struct OscStatus {
    /// The address it's bound to, while listening.
    pub listening: Option<String>,
    pub error: Option<String>,
    pub received: u64,
}

/// One OSC message: its address and arguments, simplified.
#[derive(Clone, Debug, PartialEq)]
pub struct OscMessage {
    pub addr: String,
    pub args: Vec<OscArg>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OscArg {
    Num(f32),
    Str(String),
}

impl OscMessage {
    pub fn num(&self, i: usize) -> Option<f32> {
        match self.args.get(i)? {
            OscArg::Num(n) => Some(*n),
            OscArg::Str(s) => s.parse().ok(),
        }
    }

    pub fn str(&self, i: usize) -> Option<&str> {
        match self.args.get(i)? {
            OscArg::Str(s) => Some(s),
            OscArg::Num(_) => None,
        }
    }
}

/// Every message in a packet, bundles flattened.
pub fn messages(packet: OscPacket) -> Vec<OscMessage> {
    match packet {
        OscPacket::Message(m) => {
            let args = m
                .args
                .into_iter()
                .filter_map(|a| match a {
                    OscType::Float(f) => Some(OscArg::Num(f)),
                    OscType::Double(d) => Some(OscArg::Num(d as f32)),
                    OscType::Int(i) => Some(OscArg::Num(i as f32)),
                    OscType::Long(l) => Some(OscArg::Num(l as f32)),
                    OscType::Bool(b) => Some(OscArg::Num(if b { 1.0 } else { 0.0 })),
                    OscType::String(s) => Some(OscArg::Str(s)),
                    _ => None,
                })
                .collect();
            vec![OscMessage { addr: m.addr, args }]
        }
        OscPacket::Bundle(b) => b.content.into_iter().flat_map(messages).collect(),
    }
}

/// Commands OSC may not run: what `--safe` refuses, plus files, bindings and OSC's own
/// settings (a remote shouldn't widen its own reach).
pub fn refused(id: &str) -> bool {
    let spec = crate::commands::spec(id);
    spec.is_none_or(|s| s.destructive)
        || id == "app.quit"
        || id.starts_with("bind.")
        || id.starts_with("osc.")
        || matches!(id, "tile.add" | "tile.paste" | "tile.replaceAudio" | "tile.picture" | "set.import" | "set.export" | "view.screenshot" | "library.reveal")
}

/// Start the listener: it follows the library's OSC settings, rebinding when they change.
pub fn start(lender: Arc<Lender>, wake: Option<eframe::egui::Context>) {
    std::thread::Builder::new()
        .name("osc".into())
        .spawn(move || {
            let mut bound: Option<(UdpSocket, OscSettings)> = None;
            let mut buf = [0u8; rosc::decoder::MTU];
            loop {
                let settings = lender.with(|c| c.store.library.osc.clone());
                let rebind = match &bound {
                    Some((_, s)) => !settings.enabled || s.port != settings.port || s.lan != settings.lan,
                    None => settings.enabled,
                };
                if rebind {
                    bound = None;
                    let mut status = crate::osc::OscStatus::default();
                    if settings.enabled {
                        let host = if settings.lan { "0.0.0.0" } else { "127.0.0.1" };
                        match UdpSocket::bind((host, settings.port)) {
                            Ok(sock) => {
                                sock.set_read_timeout(Some(Duration::from_millis(500))).ok();
                                status.listening = Some(format!("{host}:{}", settings.port));
                                bound = Some((sock, settings.clone()));
                            }
                            Err(e) => status.error = Some(format!("Can't listen on {host}:{}: {e}", settings.port)),
                        }
                    }
                    lender.with(|c| c.osc_status = status);
                }
                let Some((sock, _)) = &bound else {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                };
                // Read for up to a second, then look at the settings again.
                let until = std::time::Instant::now() + Duration::from_secs(1);
                while std::time::Instant::now() < until {
                    let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
                    let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) else { continue };
                    let msgs = messages(packet);
                    lender.with(|c| {
                        c.osc_status.received += msgs.len() as u64;
                        for m in &msgs {
                            c.osc(m, Some(from));
                        }
                    });
                    if let Some(ctx) = &wake {
                        ctx.request_repaint();
                    }
                }
            }
        })
        .ok();
}

/// State sent to the feedback address: each value by its OSC address. Only changes go
/// out, plus everything every few seconds so a surface that reconnects catches up, or at
/// once after `/ls/resync`. An address that drops out of the state (a tile removed, a
/// smaller set) is sent once more as "" or 0, so a surface doesn't keep a stale label.
pub struct Feedback {
    sock: UdpSocket,
    pub target: String,
    addr: SocketAddr,
    last: std::collections::HashMap<String, OscArg>,
    last_full: std::time::Instant,
}

impl Feedback {
    pub fn new(target: &str) -> Option<Feedback> {
        let addr = feedback_addr(target)?;
        let local = if addr.ip().is_loopback() { "127.0.0.1:0" } else { "0.0.0.0:0" };
        let sock = UdpSocket::bind(local).ok()?;
        Some(Feedback { sock, target: target.to_owned(), addr, last: Default::default(), last_full: std::time::Instant::now() })
    }

    /// Send everything on the next [`Feedback::send`] (`/ls/resync`).
    pub fn resync(&mut self) {
        self.last_full = std::time::Instant::now() - Duration::from_secs(60);
    }

    /// Send what changed in `state` (address → value). Returns how many went out.
    pub fn send(&mut self, state: Vec<(String, OscArg)>) -> usize {
        let full = self.last_full.elapsed() > Duration::from_secs(5);
        if full {
            self.last_full = std::time::Instant::now();
        }
        let mut sent = 0;
        let gone: Vec<(String, OscArg)> = self
            .last
            .iter()
            .filter(|(a, _)| !state.iter().any(|(b, _)| b == *a))
            .map(|(a, v)| (a.clone(), if matches!(v, OscArg::Str(_)) { OscArg::Str(String::new()) } else { OscArg::Num(0.0) }))
            .collect();
        for (addr, value) in &gone {
            sent += self.send_one(addr, value);
            self.last.remove(addr);
        }
        for (addr, value) in state {
            if !full && self.last.get(&addr) == Some(&value) {
                continue;
            }
            sent += self.send_one(&addr, &value);
            self.last.insert(addr, value);
        }
        sent
    }

    fn send_one(&self, addr: &str, value: &OscArg) -> usize {
        let packet = OscPacket::Message(rosc::OscMessage { addr: addr.to_owned(), args: vec![to_osc(value)] });
        rosc::encoder::encode(&packet).ok().and_then(|bytes| self.sock.send_to(&bytes, self.addr).ok()).map_or(0, |_| 1)
    }
}

fn to_osc(value: &OscArg) -> OscType {
    match value {
        OscArg::Num(n) => OscType::Float(*n),
        OscArg::Str(s) => OscType::String(s.clone()),
    }
}

/// A countdown for a button label: "1:05" with whole seconds rounded up, so it reads
/// 0:01 in the last second and 0:00 only when the sound has ended.
pub fn countdown(remaining: f64) -> String {
    crate::logic::format_clock(remaining.max(0.0).ceil(), false)
}

/// Round for feedback, so a playing tile sends its time ten times a second and not
/// every frame.
pub fn coarse(v: f64, step: f64) -> f32 {
    ((v / step).round() * step) as f32
}

/// Where feedback goes: `host:port`, resolved.
pub fn feedback_addr(s: &str) -> Option<SocketAddr> {
    use std::net::ToSocketAddrs;
    s.to_socket_addrs().ok()?.next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscBundle, OscMessage as RMsg, OscTime};

    #[test]
    fn flattens_bundles_and_simplifies_arguments() {
        let msg = |addr: &str, args: Vec<OscType>| OscPacket::Message(RMsg { addr: addr.into(), args });
        let packet = OscPacket::Bundle(OscBundle {
            timetag: OscTime { seconds: 0, fractional: 1 },
            content: vec![msg("/ls/go", vec![]), msg("/ls/trigger", vec![OscType::String("Air Horn".into()), OscType::Int(1)])],
        });
        let bytes = rosc::encoder::encode(&packet).unwrap();
        let (_, back) = rosc::decoder::decode_udp(&bytes).unwrap();
        let ms = messages(back);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[1].str(0), Some("Air Horn"));
        assert_eq!(ms[1].num(1), Some(1.0));
        assert!(refused("tile.remove") && refused("app.quit") && refused("set.import") && refused("bind.add") && refused("nope"));
        assert!(!refused("tile.trigger") && !refused("cue.go") && !refused("master.volume"));
    }
}
