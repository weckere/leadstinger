//! New clips on the connected QuipClipper servers, counted while the app runs
//! (t-m21-quip-badge, D-103). The status bar's badge, the QuipClipper window's "N new",
//! `status.quip_new` and `view.quip {new: true}` all read this one watcher.
//!
//! A round asks every connected server for the clips added since its seen cursor, on a
//! thread of its own, never the ui thread. It's the listing [`new_clips`] asks for, with
//! `If-None-Match`, so a library that hasn't changed costs the server a `304`. Rounds run
//! every [`OPEN_EVERY`] while the window is open, and every [`CLOSED_EVERY`] while it's
//! closed if the badge is on (`settings.quipBadge`); without a window, never. A server
//! that doesn't answer keeps its last count and is asked again later and later, up to
//! [`BACKOFF_MAX`]: why goes to `status` and the badge's hover text, never to a notice,
//! so a laptop away from its server stays quiet during a show.

use super::clips::{new_clips, Listing, NewClips};
use crate::controller::Controller;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the servers are asked while the QuipClipper window is open.
pub const OPEN_EVERY: Duration = Duration::from_secs(30);
/// How often while it's closed (with the badge on).
pub const CLOSED_EVERY: Duration = Duration::from_secs(60);
/// The longest a server that keeps failing waits between tries.
const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

/// What a server said last.
#[derive(Clone, Debug, Default)]
pub struct Server {
    pub server: String,
    /// The clips since the seen cursor (`since`); none before its first answer.
    pub new: NewClips,
    /// When it last answered.
    pub checked: Option<Instant>,
    /// Why the last try failed, if it did.
    pub error: Option<String>,
    failures: u32,
    /// Not asked again before this (it failed).
    wait_until: Option<Instant>,
    /// The watcher's `marks` when this was last marked or recorded: answers to a round
    /// started before then are stale.
    marked: u64,
}

impl Server {
    pub fn count(&self) -> usize {
        self.new.listing.clips.len()
    }
}

/// Each connected server's answer: `None` when it wasn't asked (it's waiting after a
/// failure), or the whole round's failure (the servers file can't be read).
type Answers = Result<Vec<(String, Option<Result<NewClips, String>>)>, String>;

/// The watcher, kept by the controller (`Controller::quip_watch`).
#[derive(Default)]
pub struct Watch {
    /// The connected servers, as `quip.list` orders them, with what each said last.
    servers: Vec<Server>,
    /// The round under way, and `marks` when it started.
    round: Option<(Receiver<Answers>, u64)>,
    last_round: Option<Instant>,
    /// Counts marks and records, to tell stale answers.
    marks: u64,
}

/// A server's answer as the watcher keeps it: the first look (a baseline) and clips just
/// marked as seen both leave nothing new, counted from the cursor now stored.
fn settle(new: &NewClips) -> NewClips {
    if new.since.is_some() && !new.marked {
        return NewClips { marked: false, ..new.clone() };
    }
    let cursor = new.listing.cursor.clone();
    NewClips { since: cursor.clone(), listing: Arc::new(Listing { cursor, ..Default::default() }), marked: false }
}

/// A server as a person would say it: its address without the scheme.
pub fn host(server: &str) -> &str {
    server.split_once("://").map_or(server, |(_, rest)| rest)
}

impl Watch {
    pub fn servers(&self) -> &[Server] {
        &self.servers
    }

    /// New clips on every server.
    pub fn count(&self) -> usize {
        self.servers.iter().map(Server::count).sum()
    }

    pub fn count_for(&self, server: &str) -> usize {
        self.servers.iter().find(|s| s.server == server).map_or(0, Server::count)
    }

    /// The first server, in `quip.list` order, with new clips.
    pub fn first_with_new(&self) -> Option<&str> {
        self.servers.iter().find(|s| s.count() > 0).map(|s| s.server.as_str())
    }

    /// Ask every server at the next tick (Refresh), failed ones too.
    pub fn ask_now(&mut self) {
        self.last_round = None;
        for s in &mut self.servers {
            s.wait_until = None;
        }
    }

    /// Keep what `quip.new` found, so the badge and `status` agree with it.
    pub fn record(&mut self, server: &str, new: &NewClips) {
        self.marks += 1;
        let marks = self.marks;
        let s = self.entry(server);
        s.new = settle(new);
        s.checked = Some(Instant::now());
        s.error = None;
        s.failures = 0;
        s.wait_until = None;
        s.marked = marks;
    }

    /// Count a server's new clips as seen: the clips it listed last, which are returned
    /// to show. The cursor stored is that listing's own, so a clip cut since then stays
    /// new; and if the stored cursor was moved meanwhile (by `quip.new {mark: true}` or
    /// another copy of the app), it isn't moved back. A local write: no request.
    pub fn mark_seen(&mut self, data_dir: &Path, server: &str) -> Result<Arc<Listing>, String> {
        self.marks += 1;
        let marks = self.marks;
        let Some(s) = self.servers.iter_mut().find(|s| s.server == server) else { return Ok(Arc::default()) };
        let listing = s.new.listing.clone();
        if let (Some(from), Some(to)) = (&s.new.since, &listing.cursor) {
            super::advance_seen(data_dir, server, from, to)?;
        }
        s.new = settle(&NewClips { since: None, listing: listing.clone(), marked: true });
        s.marked = marks;
        Ok(listing)
    }

    fn entry(&mut self, server: &str) -> &mut Server {
        if let Some(i) = self.servers.iter().position(|s| s.server == server) {
            return &mut self.servers[i];
        }
        self.servers.push(Server { server: server.to_owned(), ..Default::default() });
        self.servers.last_mut().expect("just pushed")
    }

    /// Start a round on its own thread.
    fn start(&mut self, data_dir: &Path) {
        self.last_round = Some(Instant::now());
        let now = Instant::now();
        let waiting: Vec<String> = self.servers.iter().filter(|s| s.wait_until.is_some_and(|t| now < t)).map(|s| s.server.clone()).collect();
        let dir = data_dir.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("quip-new".into()).spawn(move || {
            let answers = super::list(&dir).map(|servers| {
                servers
                    .into_iter()
                    .map(|(s, _)| {
                        if waiting.contains(&s) {
                            return (s, None);
                        }
                        let answer = match super::token_for(&dir, &s) {
                            Ok(Some(token)) => new_clips(&dir, &s, &token, false),
                            Ok(None) => return (s, None),
                            Err(e) => Err(e),
                        };
                        (s, Some(answer))
                    })
                    .collect()
            });
            tx.send(answers).ok();
            super::wake();
        });
        if spawned.is_ok() {
            self.round = Some((rx, self.marks));
        }
    }

    /// Take a round's answers. Servers no longer connected are dropped.
    fn take(&mut self, answers: Answers, started: u64) {
        let Ok(answers) = answers else { return };
        let now = Instant::now();
        let mut old = std::mem::take(&mut self.servers);
        for (server, answer) in answers {
            let mut s = match old.iter().position(|o| o.server == server) {
                Some(i) => old.swap_remove(i),
                None => Server { server, ..Default::default() },
            };
            match answer {
                // Marked or recorded since the round asked: what it found is stale.
                _ if s.marked > started => {}
                None => {}
                Some(Ok(new)) => {
                    s.new = settle(&new);
                    s.checked = Some(now);
                    s.error = None;
                    s.failures = 0;
                    s.wait_until = None;
                }
                Some(Err(e)) => {
                    s.failures += 1;
                    s.error = Some(e);
                    s.wait_until = Some(now + backoff(s.failures));
                }
            }
            self.servers.push(s);
        }
    }

    /// `status.quip_new`.
    pub fn json(&self, badge: bool) -> Value {
        json!({
            "count": self.count(),
            "badge": badge,
            "servers": self.servers.iter().map(|s| json!({
                "server": s.server,
                "count": s.count(),
                "checked": s.checked.map(|at| at.elapsed().as_secs()),
                "error": s.error,
            })).collect::<Vec<_>>(),
        })
    }
}

/// How long a server waits after its `failures`th failure in a row: 2, 4, 8… minutes, up
/// to [`BACKOFF_MAX`].
fn backoff(failures: u32) -> Duration {
    CLOSED_EVERY.saturating_mul(1 << failures.min(8)).min(BACKOFF_MAX)
}

/// Take a finished round, and start the next when it's due (each controller tick).
pub fn poll(c: &mut Controller) {
    if !c.has_window {
        return;
    }
    let w = &mut c.quip_watch;
    if let Some((rx, started)) = &w.round {
        let started = *started;
        match rx.try_recv() {
            Ok(answers) => {
                w.round = None;
                w.take(answers, started);
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => w.round = None,
        }
    }
    let every = if c.show_quip {
        OPEN_EVERY
    } else if c.store.library.quip_badge {
        CLOSED_EVERY
    } else {
        return;
    };
    match w.last_round.map(|at| at.elapsed()) {
        Some(since) if since < every => super::wake_after(every - since),
        _ => w.start(&c.store.data_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_server_waits_longer_each_time() {
        let mins: Vec<u64> = (1..=6).map(|n| backoff(n).as_secs() / 60).collect();
        assert_eq!(mins, [2, 4, 8, 15, 15, 15]);
    }

    #[test]
    fn a_baseline_or_a_mark_leaves_nothing_new() {
        let listing = |clips: usize, cursor: &str| Arc::new(Listing { clips: vec![Default::default(); clips], cursor: Some(cursor.into()), ..Default::default() });
        let fresh = settle(&NewClips { since: Some("v1".into()), listing: listing(2, "v2"), marked: false });
        assert_eq!((fresh.since.as_deref(), fresh.listing.clips.len()), (Some("v1"), 2));
        for (since, marked) in [(None, true), (Some("v1".into()), true)] {
            let s = settle(&NewClips { since, listing: listing(2, "v2"), marked });
            assert_eq!((s.since.as_deref(), s.listing.clips.len(), s.listing.cursor.as_deref()), (Some("v2"), 0, Some("v2")));
        }
    }
}
