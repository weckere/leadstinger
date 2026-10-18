//! The control socket (docs/CLI.md): the CLI (`leadstinger-cli`) runs any registry command in the
//! running app over a Unix domain socket in the data dir, one JSON object per line.
//!
//! ```text
//! → {"id": 7, "command": "tile.trigger", "params": {"tile": "Rimshot"}}
//! ← {"id": 7, "ok": true, "result": {...}}
//! ← {"id": 7, "ok": false, "error": {"code": "ambiguous", "message": "…", "candidates": [...]}}
//! → {"command": "events.subscribe"}   then one {"event": …} line per change
//! → {"command": "hello"}              ← {"ok": true, "result": {"app", "version", "pid"}}
//! ```
//!
//! The [`Controller`] is shared through a [`Lender`]: the window borrows it for each
//! frame, and between frames the socket threads run commands on it directly. So a
//! command doesn't wait for the window to repaint, which a hidden or occluded window
//! (another Hyprland workspace, a covered Mac window) may not do; a ticker thread keeps
//! auto-advance and the event stream going meanwhile.
//!
//! Access control is the file system: the socket is 0600 in the user's data dir (or
//! their private runtime dir). There is no network listener.

use crate::controller::{CmdError, Controller};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The socket for a data dir: `<data dir>/control.sock`, or, when that path is too long
/// for a Unix socket (about 104 bytes on macOS), a name derived from the data dir in the
/// user's runtime dir (`$XDG_RUNTIME_DIR`, else the per-user temp dir).
pub fn socket_path(data_dir: &Path) -> PathBuf {
    let direct = data_dir.join("control.sock");
    if direct.as_os_str().len() <= 100 {
        return direct;
    }
    // FNV-1a: stable across builds, unlike std's hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data_dir.as_os_str().as_encoded_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("{}-{h:016x}.sock", crate::brand::SLUG))
}

/// Take the library lock (`<data dir>/library.lock`), waiting up to `wait` for whoever
/// holds it (the app for its whole life, the offline CLI for one command). Keep the
/// returned file open as long as you use the library.
pub fn lock_library(data_dir: &Path, wait: Duration) -> io::Result<File> {
    std::fs::create_dir_all(data_dir)?;
    let file = File::options().create(true).truncate(false).write(true).open(data_dir.join("library.lock"))?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, format!("the library is in use by another {}", crate::brand::APP_NAME)))
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// The Controller, lent to one user at a time.
pub struct Lender {
    slot: Mutex<Option<Controller>>,
    returned: Condvar,
}

impl Lender {
    pub fn new(c: Controller) -> Arc<Lender> {
        Arc::new(Lender { slot: Mutex::new(Some(c)), returned: Condvar::new() })
    }

    /// Take the Controller (the window, for one frame), waiting if it's in use.
    pub fn borrow(&self) -> Controller {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(c) = slot.take() {
                return c;
            }
            slot = self.returned.wait(slot).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn give_back(&self, c: Controller) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
        self.returned.notify_all();
    }

    /// Run `f` on the Controller, waiting while the window has it.
    pub fn with<R>(&self, f: impl FnOnce(&mut Controller) -> R) -> R {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(c) = slot.as_mut() {
                return f(c);
            }
            slot = self.returned.wait(slot).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Run `f` only if the Controller is free right now.
    pub fn try_with<R>(&self, f: impl FnOnce(&mut Controller) -> R) -> Option<R> {
        let mut slot = self.slot.try_lock().ok()?;
        slot.as_mut().map(f)
    }
}

/// A running control socket. Dropping it removes the socket file.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
}

impl Server {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Is an app already answering on this data dir's socket?
pub fn app_running(data_dir: &Path) -> bool {
    Client::connect(data_dir).and_then(|mut c| c.call("hello", json!({})).map_err(|e| io::Error::other(e.message))).is_ok()
}

/// Listen on the data dir's socket. `wake` (the window's egui context) is asked to
/// repaint after each command, so effects such as a file picker show up promptly.
pub fn serve(lender: Arc<Lender>, data_dir: &Path, wake: Option<eframe::egui::Context>) -> io::Result<Server> {
    let path = socket_path(data_dir);
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("another {} is already listening", crate::brand::APP_NAME)));
        }
        std::fs::remove_file(&path)?; // left behind by a crash
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    let ticker = lender.clone();
    std::thread::Builder::new().name("control-ticker".into()).spawn(move || loop {
        // While the window renders it ticks the Controller itself; this keeps time,
        // auto-advance and events moving when it doesn't (hidden, minimised).
        std::thread::sleep(Duration::from_millis(50));
        ticker.try_with(|c| c.tick_now());
        if Arc::strong_count(&ticker) == 1 {
            break; // everyone else is gone
        }
    })?;

    std::thread::Builder::new().name("control-socket".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let lender = lender.clone();
            let wake = wake.clone();
            std::thread::spawn(move || {
                if let Err(e) = handle(stream, &lender, wake.as_ref()) {
                    if e.kind() != io::ErrorKind::BrokenPipe {
                        eprintln!("{}: control connection: {e}", crate::brand::SLUG);
                    }
                }
            });
        }
    })?;
    Ok(Server { path })
}

fn handle(stream: UnixStream, lender: &Lender, wake: Option<&eframe::egui::Context>) -> io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (id, reply) = match serde_json::from_str::<Value>(&line) {
            Err(e) => (Value::Null, Err(CmdError { code: "bad_request", message: format!("not JSON: {e}"), candidates: vec![] })),
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let command = req.get("command").and_then(Value::as_str).unwrap_or("").to_owned();
                let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
                match command.as_str() {
                    "hello" => (
                        id,
                        Ok(json!({"app": crate::brand::APP_NAME, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id()})),
                    ),
                    "events.subscribe" => {
                        let events = lender.with(|c| c.subscribe());
                        writeln!(out, "{}", json!({"id": id, "ok": true, "result": {"subscribed": true}}))?;
                        for event in events {
                            writeln!(out, "{event}")?;
                        }
                        return Ok(());
                    }
                    _ => {
                        let r = lender.with(|c| c.execute(&command, params));
                        if let Some(ctx) = wake {
                            ctx.request_repaint();
                        }
                        (id, r)
                    }
                }
            }
        };
        writeln!(out, "{}", encode(id, reply))?;
    }
    Ok(())
}

fn encode(id: Value, reply: Result<Value, CmdError>) -> Value {
    match reply {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(e) => json!({"id": id, "ok": false, "error": error_json(&e)}),
    }
}

pub fn error_json(e: &CmdError) -> Value {
    let mut v = json!({"code": e.code, "message": e.message});
    if !e.candidates.is_empty() {
        v["candidates"] = json!(e.candidates);
    }
    v
}

/// A connection to the running app.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    pub fn connect(data_dir: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(socket_path(data_dir))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        Ok(Client { reader: BufReader::new(stream.try_clone()?), writer: stream, next_id: 1 })
    }

    /// Run one command in the app.
    pub fn call(&mut self, command: &str, params: Value) -> Result<Value, CmdError> {
        let io_err = |e: io::Error| CmdError { code: "io", message: format!("lost the connection to {}: {e}", crate::brand::APP_NAME), candidates: vec![] };
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.writer, "{}", json!({"id": id, "command": command, "params": params})).map_err(io_err)?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).map_err(io_err)? == 0 {
            return Err(io_err(io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the connection")));
        }
        decode_reply(&line)
    }

    /// Subscribe to events; yields one event object per change until the app quits.
    pub fn events(mut self) -> io::Result<impl Iterator<Item = Value>> {
        self.writer.set_read_timeout(None)?;
        writeln!(self.writer, "{}", json!({"command": "events.subscribe"}))?;
        let mut first = String::new();
        self.reader.read_line(&mut first)?;
        Ok(self.reader.lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()))
    }
}

fn decode_reply(line: &str) -> Result<Value, CmdError> {
    let v: Value = serde_json::from_str(line)
        .map_err(|e| CmdError { code: "io", message: format!("bad reply from {}: {e}", crate::brand::APP_NAME), candidates: vec![] })?;
    if v["ok"].as_bool() == Some(true) {
        return Ok(v["result"].clone());
    }
    let e = &v["error"];
    // Back onto the Controller's static codes (the CLI's exit codes use them).
    let code = e["code"].as_str().and_then(|c| CODES.iter().find(|k| **k == c).copied()).unwrap_or("failed");
    Err(CmdError {
        code,
        message: e["message"].as_str().unwrap_or("the command failed").to_owned(),
        candidates: e["candidates"].as_array().cloned().unwrap_or_default(),
    })
}

const CODES: &[&str] = &[
    "failed",
    "bad_params", "not_found", "ambiguous", "unknown_command", "loading", "no_audio", "end_of_cues", "nothing_to_paste",
    "decode_failed", "not_playing", "last_set", "device_failed", "bad_request", "io", "no_window",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Engine;
    use crate::store::Store;

    fn short_temp(name: &str) -> PathBuf {
        let d = PathBuf::from("/tmp").join(format!("leadstinger-{name}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn long_data_dirs_get_a_short_socket_path() {
        let short = Path::new("/home/someone/.local/share/leadstinger");
        assert_eq!(socket_path(short), short.join("control.sock"));
        let long = PathBuf::from(format!("/tmp/{}", "x".repeat(150)));
        let p = socket_path(&long);
        assert!(p.as_os_str().len() < 104, "{}", p.display());
        assert_eq!(p, socket_path(&long), "stable");
    }

    #[test]
    fn commands_and_events_over_the_socket() {
        let dir = short_temp("ctl");
        let store = Store::open(dir.clone()).unwrap();
        let lender = Lender::new(Controller::new(store, Engine::disabled("test")));
        let server = serve(lender.clone(), &dir, None).unwrap();
        assert!(app_running(&dir));
        assert_eq!(serve(lender.clone(), &dir, None).unwrap_err().kind(), io::ErrorKind::AddrInUse);

        let mut events = Client::connect(&dir).unwrap().events().unwrap();
        let mut c = Client::connect(&dir).unwrap();
        c.call("set.create", json!({"name": "Encore"})).unwrap();
        let sets = c.call("set.list", json!({})).unwrap();
        assert_eq!(sets[1]["name"], "Encore");
        let e = c.call("tile.get", json!({"tile": "nope"})).unwrap_err();
        assert_eq!(e.code, "not_found");
        // The ticker publishes the change.
        let ev = events.next().unwrap();
        assert_eq!(ev["event"], "library.changed", "{ev}");

        // The window borrowing the Controller makes callers wait, not fail.
        let held = lender.borrow();
        let t = std::thread::spawn(move || Client::connect(&dir).unwrap().call("status", json!({})).map(|_| dir));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!t.is_finished());
        lender.give_back(held);
        let dir = t.join().unwrap().unwrap();

        let sock = server.path().to_owned();
        drop(server);
        assert!(!sock.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_library_lock_is_exclusive() {
        let dir = short_temp("lock");
        let held = lock_library(&dir, Duration::ZERO).unwrap();
        assert_eq!(lock_library(&dir, Duration::from_millis(100)).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(held);
        lock_library(&dir, Duration::ZERO).unwrap();
        std::fs::remove_dir_all(dir).ok();
    }
}
