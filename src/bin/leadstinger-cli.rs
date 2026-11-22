//! `leadstinger-cli`: every command of the app from a shell (docs/CLI.md).
//!
//! It runs commands in the running app over the control socket, or, when the app is
//! closed, opens the library itself (taking the library lock) for the commands that
//! don't need the app.

use serde_json::{json, Map, Value};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use leadstinger::audio::Engine;
use leadstinger::brand::{APP_NAME, CLI_NAME};
use leadstinger::commands::{spec, Kind, Spec, COMMANDS};
use leadstinger::control::{self, Client};
use leadstinger::controller::{CmdError, Controller};
use leadstinger::logic::format_clock;
use leadstinger::policy::{local_paths, Policy};
use leadstinger::store::Store;

fn usage() -> String {
    format!(
        "\
{CLI_NAME}: run any {APP_NAME} command (docs/CLI.md).

USAGE:
  {CLI_NAME} <command> [<main argument>…] [--param value]… [--params JSON]
  {CLI_NAME} commands [--filter TEXT] [--json | --markdown | --python]
  {CLI_NAME} help <command>
  {CLI_NAME} run [--dry-run] <script.jsonl | ->   one batch: one undo step, nothing kept if a step fails
  {CLI_NAME} watch
  {CLI_NAME} connect                            the socket protocol on stdin/stdout, app or library
  {CLI_NAME} mcp [--allow-destructive]          an MCP server on stdio: commands, query, run, screenshot
  {CLI_NAME} serve [--silent] [--device NAME]   headless: play and serve the socket, no window

Shortcuts: status, list, sets, trigger, play, stop, stop-all, go, add.
Options: --json  --offline  --data-dir DIR  --safe  --allow-dir DIR  --keep-going  --dry-run
         --ssh (forced-command mode: the command comes from $SSH_ORIGINAL_COMMAND and
         always runs --safe --json)  --version  --help"
    )
}

/// Exit codes (docs/CLI.md).
const FAILED: u8 = 1;
const USAGE_ERR: u8 = 2;
const NOT_RUNNING: u8 = 3;
const AMBIGUOUS: u8 = 4;
const REFUSED: u8 = 5;

/// Parameters that are always text, even when they look like a number ("1984").
const TEXT_KEYS: &[&str] = &[
    "tile", "set", "name", "title", "path", "query", "text", "hotkey", "layout", "theme", "appearance", "color", "mode",
    "event", "pictureLayer", "pictureFit", "folder", "format", "url", "token", "link",
];

#[derive(Clone)]
struct Opts {
    /// Forced-command mode for an SSH key (docs/CLI.md "Remote agents").
    ssh: bool,
    json: bool,
    offline: bool,
    safe: bool,
    keep_going: bool,
    /// `run`: report what the script would change, and change nothing.
    dry_run: bool,
    data_dir: PathBuf,
    allow_dirs: Vec<PathBuf>,
}

struct Fail {
    code: u8,
    error: CmdError,
}

impl Fail {
    fn new(code: u8, error_code: &'static str, message: impl Into<String>) -> Fail {
        Fail { code, error: CmdError { code: error_code, message: message.into(), candidates: vec![] } }
    }

    fn usage(message: impl Into<String>) -> Fail {
        Fail::new(USAGE_ERR, "usage", message)
    }

    fn from_cmd(e: CmdError) -> Fail {
        let code = match e.code {
            "ambiguous" => AMBIGUOUS,
            "no_window" => USAGE_ERR,
            "refused" => REFUSED,
            "not_running" => NOT_RUNNING,
            _ => FAILED,
        };
        Fail { code, error: e }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = Opts {
        ssh: false,
        json: !std::io::stdout().is_terminal(),
        offline: false,
        safe: false,
        keep_going: false,
        dry_run: false,
        data_dir: leadstinger::brand::data_dir(),
        allow_dirs: Vec::new(),
    };
    // Global options may appear anywhere; the rest is the command line proper.
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => opts.json = true,
            "--offline" => opts.offline = true,
            "--safe" => opts.safe = true,
            "--keep-going" => opts.keep_going = true,
            "--dry-run" => opts.dry_run = true,
            "--ssh" => opts.ssh = true,
            "--data-dir" | "--allow-dir" => {
                let Some(v) = it.next() else { return report(&opts, Fail::usage(format!("{a} needs a folder"))) };
                if a == "--data-dir" {
                    opts.data_dir = absolute(&v);
                } else {
                    opts.allow_dirs.push(absolute(&v).canonicalize().unwrap_or_else(|_| absolute(&v)));
                }
            }
            "--version" | "-V" => {
                println!("{CLI_NAME} {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" if rest.is_empty() => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            _ => rest.push(a),
        }
    }
    if opts.ssh {
        match remote_command(&mut opts, rest) {
            Ok(words) => rest = words,
            Err(f) => return report(&opts, f),
        }
    }
    let result = match rest.first().map(String::as_str) {
        None => {
            println!("{}", usage());
            Ok(())
        }
        Some("help") => match rest.get(1) {
            Some(name) => help(name),
            None => {
                println!("{}", usage());
                Ok(())
            }
        },
        Some("commands") => list_commands(&opts, &rest[1..]),
        Some("run") => run_script(&opts, rest.get(1).map(String::as_str)),
        Some("watch") => watch(&opts),
        Some("connect") => connect(&opts),
        Some("mcp") => mcp(&opts, &rest[1..]),
        Some("serve") => serve(&opts, &rest[1..]),
        Some(_) => one_command(&opts, &rest),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => report(&opts, f),
    }
}

/// `--ssh`: the key's forced command is `leadstinger-cli --ssh [--data-dir D] [--allow-dir A]`
/// and the caller's words arrive in `$SSH_ORIGINAL_COMMAND`. They always run `--safe` and
/// `--json`, and may not change the library, the allowed folders or go offline, read a
/// script file on this machine, or pass anything else on the forced command line.
fn remote_command(opts: &mut Opts, local: Vec<String>) -> Result<Vec<String>, Fail> {
    opts.safe = true;
    opts.json = true;
    if !local.is_empty() {
        return Err(Fail::usage("--ssh takes its command from $SSH_ORIGINAL_COMMAND, not the command line"));
    }
    let raw = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
    let mut words = split_words(&raw).map_err(Fail::usage)?;
    if words.first().is_some_and(|w| w == CLI_NAME) {
        words.remove(0);
    }
    let mut out = Vec::new();
    for w in words {
        match w.as_str() {
            "--json" | "--safe" => {}
            "--keep-going" => opts.keep_going = true,
            "--dry-run" => opts.dry_run = true,
            "--data-dir" | "--allow-dir" | "--offline" | "--ssh" => {
                return Err(Fail::new(REFUSED, "refused", format!("{w} can't be set from a remote command")));
            }
            _ => out.push(w),
        }
    }
    if out.first().is_some_and(|w| w == "serve") {
        return Err(Fail::new(REFUSED, "refused", "serve can't be started from a remote command"));
    }
    if out.first().is_some_and(|w| w == "run") && out.get(1).is_some_and(|f| f != "-") {
        return Err(Fail::new(REFUSED, "refused", "over SSH, `run` reads its script from stdin: use `run -`"));
    }
    Ok(out)
}

/// Split a command line the way a POSIX shell would for plain words: whitespace
/// separates; '…' is literal; "…" allows \" \\ \$ \`; a backslash outside quotes escapes
/// the next character. No expansion of any kind happens.
fn split_words(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("unterminated ' quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => cur.push(e),
                            Some(e) => {
                                cur.push('\\');
                                cur.push(e);
                            }
                            None => return Err("unterminated \" quote".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("unterminated \" quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(e) => cur.push(e),
                    None => return Err("trailing backslash".into()),
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

fn report(opts: &Opts, f: Fail) -> ExitCode {
    if opts.json && f.error.code != "usage" {
        println!("{}", json!({"error": control::error_json(&f.error)}));
    } else {
        eprintln!("{CLI_NAME}: {}", f.error.message);
        for c in &f.error.candidates {
            let title = c["title"].as_str().or(c["name"].as_str()).unwrap_or("?");
            let id = c["tile"].as_str().or(c["set"].as_str()).unwrap_or("");
            match c["set"].as_str().filter(|_| c["title"].is_string()) {
                Some(set) => eprintln!("  {set}/{title}  ({id})"),
                None => eprintln!("  {title}  ({id})"),
            }
        }
        if f.code == USAGE_ERR && f.error.code == "usage" {
            eprintln!("\nRun `{CLI_NAME} help` for usage, `{CLI_NAME} commands` for the list.");
        }
    }
    ExitCode::from(f.code)
}

fn absolute(p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

fn alias(name: &str) -> &str {
    match name {
        "status" => "status",
        "list" => "tile.list",
        "sets" => "set.list",
        "trigger" => "tile.trigger",
        "play" => "tile.play",
        "stop" => "tile.stop",
        "stop-all" => "transport.stopAll",
        "go" => "cue.go",
        "add" => "tile.add",
        other => other,
    }
}

/// `fade-in` → `fadeIn`.
fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '-' || c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn typed(key: &str, raw: &str) -> Result<Value, Fail> {
    if raw == "null" {
        return Ok(Value::Null);
    }
    if TEXT_KEYS.contains(&key) {
        return Ok(Value::String(raw.to_owned()));
    }
    // An object or a list must be valid JSON: sent on as text, the app would ignore it.
    if raw.starts_with(['[', '{']) {
        return serde_json::from_str(raw).map_err(|e| Fail::usage(format!("--{key}: not valid JSON ({e}): {raw}")));
    }
    let looks_json = raw == "true" || raw == "false" || raw.starts_with('"') || raw.parse::<f64>().is_ok();
    if looks_json {
        if let Ok(v) = serde_json::from_str(raw) {
            return Ok(v);
        }
    }
    Ok(Value::String(raw.to_owned()))
}

/// Whether a command's parameter summary names `key` (as a whole word).
fn mentions(spec: &Spec, key: &str) -> bool {
    spec.params
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|w| w == key)
}

/// The parameter a `--flag` names: as typed (`--cues_source`), camel-cased
/// (`--fade-in` → `fadeIn`) or snake-cased (`--cues-source` → `cues_source`), whichever
/// the command's summary names, else the summary's word that matches ignoring case and
/// separators (`--oneshot` → `oneShot`). A flag it doesn't name is a usage error, never dropped.
fn flag_key(spec: &Spec, flag: &str) -> Result<String, Fail> {
    let candidates = [flag.to_owned(), camel(flag), flag.replace('-', "_")];
    if let Some(k) = candidates.into_iter().find(|k| mentions(spec, k)) {
        return Ok(k);
    }
    // `--oneshot`, `--ONESHOT`: the summary's own spelling, ignoring case and separators.
    let loose = |w: &str| w.replace(['-', '_'], "").to_ascii_lowercase();
    let want = loose(flag);
    let found = spec.params.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).find(|w| !w.is_empty() && loose(w) == want);
    found.map(str::to_owned).ok_or_else(|| {
        Fail::usage(format!(
            "{} has no option --{flag} (see `{CLI_NAME} help {}`; --params '{{…}}' sends a JSON object as it is)",
            spec.id, spec.id
        ))
    })
}

/// Turn `<command> [main…] [--k v]…` into an id and a parameter object.
fn parse(args: &[String]) -> Result<(&'static Spec, Value), Fail> {
    let name = alias(&args[0]);
    let spec = spec(name).ok_or_else(|| {
        let near: Vec<&str> = COMMANDS.iter().map(|s| s.id).filter(|id| id.contains(name) || name.contains(id)).take(5).collect();
        let hint = if near.is_empty() { String::new() } else { format!(" (did you mean {}?)", near.join(", ")) };
        Fail::usage(format!("no command {name:?}{hint}"))
    })?;
    let mut params = Map::new();
    let mut positional = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--params" {
            let raw = args.get(i + 1).ok_or_else(|| Fail::usage("--params needs a JSON object"))?;
            match serde_json::from_str::<Value>(raw) {
                Ok(Value::Object(m)) => params.extend(m),
                _ => return Err(Fail::usage("--params must be a JSON object")),
            }
            i += 2;
        } else if let Some(k) = a.strip_prefix("--") {
            let (k, inline) = match k.split_once('=') {
                Some((k, v)) => (k, Some(v.to_owned())),
                None => (k, None),
            };
            let key = flag_key(spec, k)?;
            let value = match inline {
                Some(v) => typed(&key, &v)?,
                None => match args.get(i + 1).filter(|n| !n.starts_with("--")) {
                    Some(v) => {
                        i += 1;
                        typed(&key, v)?
                    }
                    None => Value::Bool(true),
                },
            };
            params.insert(key, value);
            i += 1;
        } else {
            positional.push(a.clone());
            i += 1;
        }
    }
    if !positional.is_empty() {
        let main = spec.main.ok_or_else(|| Fail::usage(format!("{} takes no main argument; use --name value", spec.id)))?;
        let v = if main == "paths" {
            json!(positional)
        } else if positional.len() == 1 {
            typed(main, &positional[0])?
        } else if positional.len() == 2 && main != "tile" && spec.params.starts_with("{tile") {
            // `tile.picture <tile> <path>`, `tile.replaceAudio <tile> <path>`
            params.insert("tile".into(), json!(positional[0]));
            typed(main, &positional[1])?
        } else {
            return Err(Fail::usage(format!("{} takes one main argument ({main}); quote names with spaces", spec.id)));
        };
        params.insert(main.to_owned(), v);
    }
    // `quip.connect --token -`: off the command line, where other users' `ps` would show it.
    if spec.id == "quip.connect" && params.get("token").and_then(Value::as_str) == Some("-") {
        let mut token = String::new();
        std::io::stdin().read_line(&mut token).map_err(|e| Fail::usage(format!("reading the token from stdin: {e}")))?;
        params.insert("token".into(), json!(token.trim()));
    }
    // Paths are relative to the caller, not to the app.
    for &key in local_paths(spec).iter().filter(|k| **k != "paths") {
        if let Some(Value::String(p)) = params.get(key).filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
            params.insert(key.into(), json!(absolute(p)));
        }
    }
    match params.get("paths") {
        Some(Value::String(p)) => {
            params.insert("paths".into(), json!([absolute(p)]));
        }
        Some(Value::Array(a)) => {
            let abs: Vec<Value> = a.iter().map(|p| p.as_str().map_or(p.clone(), |s| json!(absolute(s)))).collect();
            params.insert("paths".into(), Value::Array(abs));
        }
        _ => {}
    }
    Ok((spec, Value::Object(params)))
}

/// `--safe`: no destructive commands, and files only inside `--allow-dir` folders.
/// The policy `--safe` and `--allow-dir` ask for.
fn policy(opts: &Opts) -> Policy {
    if opts.safe { Policy::safe(opts.allow_dirs.clone()) } else { Policy::open() }
}

/// Refuse here what the policy refuses (the app checks again on the socket).
fn check_safe(opts: &Opts, spec: &Spec, params: &Value) -> Result<(), Fail> {
    policy(opts).check(spec, params).map_err(Fail::from_cmd)
}

/// Where commands run: the app over its socket, or the library opened here.
enum Target {
    App(Client),
    Library { ctl: Box<Controller>, _lock: std::fs::File },
}

impl Target {
    fn open(opts: &Opts, need_app: bool) -> Result<Target, Fail> {
        if !opts.offline {
            if let Ok(mut c) = Client::connect(&opts.data_dir) {
                if opts.safe {
                    // The app holds this connection to the policy from now on.
                    c.call("policy", policy(opts).to_json()).map_err(Fail::from_cmd)?;
                }
                return Ok(Target::App(c));
            }
        }
        if need_app {
            return Err(Fail::new(
                NOT_RUNNING,
                "not_running",
                format!("{APP_NAME} isn't running, and this command needs it. Start the app, then try again."),
            ));
        }
        let lock = control::lock_library(&opts.data_dir, Duration::from_secs(5)).map_err(|e| {
            Fail::new(FAILED, "busy", format!("can't open the library in {}: {e}", opts.data_dir.display()))
        })?;
        let store = Store::open_with(opts.data_dir.clone(), false)
            .map_err(|e| Fail::new(FAILED, "failed", format!("can't open the library: {e:#}")))?;
        let mut ctl = Controller::new(store, Engine::disabled(CLI_NAME));
        ctl.has_window = false;
        Ok(Target::Library { ctl: Box::new(ctl), _lock: lock })
    }

    /// Run a command; a render (`tile.render`, `set.render`) is waited for, with its
    /// progress on a terminal, and gives what it wrote.
    fn call(&mut self, spec: &Spec, params: Value) -> Result<Value, Fail> {
        let started = self.call_once(spec, params)?;
        self.finish(spec, started)
    }

    /// Wait for what a command started (an export, a transcription, a pairing…).
    fn finish(&mut self, spec: &Spec, started: Value) -> Result<Value, Fail> {
        match (spec.id, started["job"].as_u64()) {
            ("tile.render" | "set.render", Some(job)) => self.wait_render(job, started),
            ("tile.transcribe" | "set.transcribe" | "tile.alignWords", Some(job)) => self.wait_transcribe(job, started),
            ("quip.pair", _) if matches!(started["status"].as_str(), Some("starting" | "waiting")) => self.wait_pair(started),
            ("quip.discover", _) if started["looking"] == json!(true) => self.wait_discover(),
            ("app.checkForUpdates", _) => self.wait_updates(started),
            _ => Ok(started),
        }
    }

    /// Check for Updates answers at once; wait for GitHub's answer. A failed check fails
    /// the command.
    fn wait_updates(&mut self, mut v: Value) -> Result<Value, Fail> {
        let check = spec("app.checkForUpdates").expect("registered");
        while v["checking"] == json!(true) {
            std::thread::sleep(Duration::from_millis(100));
            v = self.call_once(check, json!({"again": false}))?;
        }
        match v["status"].as_str() {
            Some("failed") => Err(Fail::new(FAILED, "failed", format!("Couldn't check for updates: {}", v["error"].as_str().unwrap_or("unknown error")))),
            _ => Ok(v),
        }
    }

    /// `quip.pair`: show the code QuipClipper shows too, and wait for Allow or Deny there.
    fn wait_pair(&mut self, started: Value) -> Result<Value, Fail> {
        let pairing = spec("quip.pairing").expect("registered");
        let server = started["server"].clone();
        let mut shown = false;
        loop {
            let v = self.call_once(pairing, json!({"url": server}))?;
            let p = v.get(0).cloned().unwrap_or_else(|| json!({"status": "cancelled"}));
            match p["status"].as_str() {
                Some("waiting") if !shown => {
                    eprintln!(
                        "Allow {APP_NAME} in QuipClipper ({}): it shows a request with the code {}",
                        p["approve_url"].as_str().unwrap_or_default(),
                        p["code"].as_str().unwrap_or_default()
                    );
                    shown = true;
                }
                Some("starting" | "waiting") => {}
                Some("connected") => return Ok(p),
                status => {
                    let code = if status == Some("denied") { "denied" } else if status == Some("expired") { "expired" } else { "failed" };
                    return Err(Fail::new(FAILED, code, p["message"].as_str().unwrap_or("The pairing didn't finish").to_owned()));
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// `quip.discover`: wait the few seconds a look takes.
    fn wait_discover(&mut self) -> Result<Value, Fail> {
        let discover = spec("quip.discover").expect("registered");
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let v = self.call_once(discover, json!({"again": false}))?;
            if v["looking"] != json!(true) {
                return Ok(v);
            }
        }
    }

    /// A transcription: wait for every tile, showing which one and how far on a
    /// terminal; fails when no tile could be done.
    fn wait_transcribe(&mut self, job: u64, started: Value) -> Result<Value, Fail> {
        let status = spec("transcribe.status").expect("registered");
        let tty = std::io::stderr().is_terminal();
        let j = loop {
            let v = self.call_once(status, json!({"job": job}))?;
            let j = v["jobs"][0].clone();
            if j["finished"].as_bool() != Some(false) {
                break j;
            }
            if tty {
                let cur = &j["current"];
                let pct = cur["fraction"].as_f64().map(|f| format!(" {:.0}%", f * 100.0)).unwrap_or_default();
                eprint!("\r\x1b[2KTranscribing {}{pct}", cur["title"].as_str().unwrap_or_default());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if tty {
            eprint!("\r\x1b[2K");
        }
        if j["cancelled"] == json!(true) {
            return Err(Fail::new(FAILED, "cancelled", "The transcription was cancelled"));
        }
        let errors: Vec<String> = j["errors"].as_array().map(|a| a.iter().map(|e| format!("“{}”: {}", e["title"].as_str().unwrap_or_default(), e["error"].as_str().unwrap_or_default())).collect()).unwrap_or_default();
        if j["results"].as_array().is_none_or(|r| r.is_empty()) && !errors.is_empty() {
            return Err(Fail::new(FAILED, "failed", format!("Couldn't transcribe {}", errors.join("; "))));
        }
        Ok(json!({"job": job, "engine": started["engine"], "results": j["results"], "errors": j["errors"], "skipped": started["skipped"]}))
    }

    fn wait_render(&mut self, job: u64, started: Value) -> Result<Value, Fail> {
        let status = spec("render.status").expect("registered");
        let tty = std::io::stderr().is_terminal();
        let r = loop {
            let v = self.call_once(status, json!({"job": job}))?;
            let r = v["renders"][0].clone();
            if r["finished"].as_bool() != Some(false) {
                break r;
            }
            if tty {
                let pct = r["fraction"].as_f64().unwrap_or(0.0) * 100.0;
                eprint!("\r\x1b[2KExporting {} {pct:.0}%", r["current"].as_str().unwrap_or_default());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if tty {
            eprint!("\r\x1b[2K");
        }
        let errors: Vec<&str> = r["errors"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !errors.is_empty() {
            return Err(Fail::new(FAILED, "failed", format!("Couldn't export {}", errors.join("; "))));
        }
        if r["cancelled"] == json!(true) {
            return Err(Fail::new(FAILED, "cancelled", "The export was cancelled"));
        }
        Ok(json!({"job": job, "format": started["format"], "files": r["written"], "skipped": started["skipped"]}))
    }

    fn call_once(&mut self, spec: &Spec, params: Value) -> Result<Value, Fail> {
        match self {
            Target::App(c) => c.call(spec.id, params).map_err(Fail::from_cmd),
            Target::Library { ctl, .. } => offline_call(ctl, spec.id, params, &Policy::open()).map_err(Fail::from_cmd),
        }
    }
}

/// A command run on the library opened here (the app isn't running): refused when it, or
/// a step of its batch, needs the app; saved afterwards.
fn offline_call(ctl: &mut Controller, id: &str, params: Value, policy: &Policy) -> Result<Value, CmdError> {
    let not_running = |id: &str| CmdError { code: "not_running", message: format!("{id} needs the running app"), candidates: vec![] };
    if let Some(s) = spec(id) {
        policy.check(s, &params)?;
        if s.kind == Kind::Live {
            return Err(not_running(id));
        }
    }
    if id == "batch.run" {
        let live = params["steps"].as_array().into_iter().flatten().filter_map(|st| st["command"].as_str()).find(|c| spec(c).is_some_and(|s| s.kind == Kind::Live));
        if let Some(c) = live {
            return Err(not_running(c));
        }
    }
    let r = ctl.execute_with(id, params, policy);
    ctl.effects.clear();
    // Files added are copied (or converted) in the background: finish first.
    ctl.store.finish_imports();
    if ctl.store.has_unsaved() {
        ctl.store.save_now().map_err(|e| CmdError { code: "failed", message: format!("couldn't save the library: {e:#}"), candidates: vec![] })?;
    }
    r
}

/// `connect`: the control socket's protocol (docs/CLI.md "Programs") on stdin and stdout,
/// for programs and clients such as clients/python: to the running app, or, when it isn't
/// running, to the library opened here (holding its lock until stdin closes; commands
/// that need the app answer `not_running`). Under `--safe` the policy is set first and
/// can't be widened.
fn connect(opts: &Opts) -> Result<(), Fail> {
    use std::io::{BufRead, BufReader, Write};
    let policy = policy(opts);
    if !opts.offline {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(control::socket_path(&opts.data_dir)) {
            let mut to_app = stream.try_clone().map_err(|e| Fail::new(FAILED, "io", e.to_string()))?;
            let mut from_app = BufReader::new(stream);
            if opts.safe {
                writeln!(to_app, "{}", json!({"command": "policy", "params": policy.to_json()})).map_err(|e| Fail::new(FAILED, "io", e.to_string()))?;
                let mut line = String::new();
                from_app.read_line(&mut line).map_err(|e| Fail::new(FAILED, "io", e.to_string()))?;
                control::error_from_reply(&line).map_err(Fail::from_cmd)?;
            }
            // Replies (and events after events.subscribe) go straight out, a line at a time.
            let out = std::thread::spawn(move || {
                let mut stdout = std::io::stdout();
                for line in from_app.lines().map_while(Result::ok) {
                    if writeln!(stdout, "{line}").and_then(|()| stdout.flush()).is_err() {
                        break;
                    }
                }
            });
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if writeln!(to_app, "{line}").is_err() {
                    break;
                }
            }
            to_app.shutdown(std::net::Shutdown::Write).ok();
            out.join().ok();
            return Ok(());
        }
    }
    let Target::Library { mut ctl, _lock } = Target::open(&Opts { offline: true, ..opts.clone() }, false)? else { unreachable!("offline") };
    let mut policy = policy;
    let mut stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let (id, reply) = match serde_json::from_str::<Value>(&line) {
            Err(e) => (Value::Null, Err(CmdError { code: "bad_request", message: format!("not JSON: {e}"), candidates: vec![] })),
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let command = req.get("command").and_then(Value::as_str).unwrap_or("").to_owned();
                let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
                let reply = match command.as_str() {
                    "hello" => Ok(json!({"app": APP_NAME, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(), "offline": true})),
                    "policy" => Policy::from_json(&params).map(|asked| {
                        policy = policy.narrow(&asked);
                        policy.to_json()
                    }),
                    "events.subscribe" => Err(CmdError { code: "not_running", message: format!("{APP_NAME} isn't running; events need the app"), candidates: vec![] }),
                    _ => offline_call(&mut ctl, &command, params, &policy),
                };
                (id, reply)
            }
        };
        if writeln!(stdout, "{}", control::encode(id, reply)).and_then(|()| stdout.flush()).is_err() {
            break;
        }
    }
    Ok(())
}

/// `mcp`: an MCP server on stdio (docs/CLI.md "MCP"). Each call reaches the running app,
/// or the library when it's closed (locked only for that call, so the app can start while
/// a client is connected); `--safe` and `--allow-dir` hold for every call.
fn mcp(opts: &Opts, args: &[String]) -> Result<(), Fail> {
    let mut allow_destructive = false;
    for a in args {
        match a.as_str() {
            "--allow-destructive" if opts.ssh => return Err(Fail::new(REFUSED, "refused", "--allow-destructive can't be set from a remote command")),
            "--allow-destructive" => allow_destructive = true,
            other => return Err(Fail::usage(format!("mcp: unknown option {other}"))),
        }
    }
    let shots = std::env::temp_dir().join(format!("{}-mcp-{}", leadstinger::brand::SLUG, std::process::id()));
    let backend = McpBackend { opts: opts.clone(), shots: shots.clone(), taken: 0 };
    let stdin = std::io::stdin();
    let r = leadstinger::mcp::Server::new(backend, allow_destructive).serve(stdin.lock(), std::io::stdout().lock());
    std::fs::remove_dir_all(&shots).ok();
    r.map_err(|e| Fail::new(FAILED, "io", e.to_string()))
}

struct McpBackend {
    opts: Opts,
    /// Where screenshots are written for a moment before they're sent.
    shots: PathBuf,
    taken: u32,
}

impl leadstinger::mcp::Backend for McpBackend {
    fn call(&mut self, command: &str, params: Value) -> Result<Value, CmdError> {
        let policy = policy(&self.opts);
        let live = spec(command).is_some_and(|s| s.kind == Kind::Live);
        if let Some(s) = spec(command) {
            policy.check(s, &params)?;
        }
        match Target::open(&self.opts, live).map_err(|f| f.error)? {
            Target::App(mut c) => c.call(command, params),
            Target::Library { mut ctl, _lock } => offline_call(&mut ctl, command, params, &policy),
        }
    }

    fn screenshot(&mut self, params: Value) -> Result<Vec<u8>, CmdError> {
        let fail = |code: &'static str, message: String| CmdError { code, message, candidates: vec![] };
        let mut c = Client::connect(&self.opts.data_dir)
            .map_err(|_| fail("not_running", format!("{APP_NAME} isn't running; a screenshot needs the app and its window")))?;
        std::fs::create_dir_all(&self.shots).map_err(|e| fail("io", e.to_string()))?;
        let dir = self.shots.canonicalize().map_err(|e| fail("io", e.to_string()))?;
        if self.opts.safe {
            // The connection's policy, plus this server's own folder for the picture.
            let mut dirs = self.opts.allow_dirs.clone();
            dirs.push(dir.clone());
            c.call("policy", Policy::safe(dirs).to_json())?;
        }
        self.taken += 1;
        let path = dir.join(format!("shot-{}.png", self.taken));
        let mut p = match params {
            Value::Object(m) => Value::Object(m),
            _ => json!({}),
        };
        p["path"] = json!(path);
        c.call("view.screenshot", p)?;
        // The window writes it once it has settled: up to a second when it's hidden.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut last = 0;
        loop {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > 0 && size == last {
                break;
            }
            if std::time::Instant::now() > deadline {
                return Err(fail("failed", "the window didn't write the screenshot in time".into()));
            }
            last = size;
            std::thread::sleep(Duration::from_millis(150));
        }
        let png = std::fs::read(&path).map_err(|e| fail("io", e.to_string()));
        std::fs::remove_file(&path).ok();
        png
    }
}

/// `run --dry-run`: what the script would have changed.
fn print_changes(opts: &Opts, changes: &Value) {
    if opts.json {
        println!("{}", json!({"dryRun": true, "changes": changes}));
        return;
    }
    println!("A dry run: nothing was changed. The script would:");
    let mut any = false;
    for (kind, name_key) in [("sets", "name"), ("tiles", "title")] {
        for (what, verb) in [("added", "add"), ("removed", "remove"), ("changed", "change")] {
            for item in changes[kind][what].as_array().map(Vec::as_slice).unwrap_or_default() {
                any = true;
                let fields = item["fields"].as_array().map(|f| f.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "));
                let fields = fields.map(|f| format!(" ({f})")).unwrap_or_default();
                println!("  {verb} {} “{}”{fields}", &kind[..kind.len() - 1], item[name_key].as_str().unwrap_or_default());
            }
        }
    }
    if !any {
        println!("  change no sets or tiles");
    }
}

fn one_command(opts: &Opts, args: &[String]) -> Result<(), Fail> {
    let (spec, params) = parse(args)?;
    check_safe(opts, spec, &params)?;
    let mut target = Target::open(opts, spec.kind == Kind::Live)?;
    let result = target.call(spec, params)?;
    print_result(opts, spec.id, &result);
    Ok(())
}

fn print_result(opts: &Opts, id: &str, v: &Value) {
    if opts.json {
        println!("{v}");
        return;
    }
    match id {
        "status" => print_status(v),
        "tile.list" => print_tiles(v),
        "set.list" => print_sets(v),
        "app.checkForUpdates" => print_update(v),
        _ if v.as_object().is_some_and(|m| m.is_empty()) => {}
        _ => println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()),
    }
}

fn print_update(v: &Value) {
    let current = v["current"].as_str().unwrap_or("?");
    match v["status"].as_str() {
        Some("newer") => {
            println!("{APP_NAME} {} is out (you have {current}).", v["latest"].as_str().unwrap_or("?"));
            println!("Download it from {}", v["url"].as_str().unwrap_or("?"));
            if let Some(notes) = v["notes"].as_str().filter(|n| !n.is_empty()) {
                println!("\n{notes}");
            }
        }
        _ => println!("You have the latest version ({current})."),
    }
}

fn clock(v: &Value) -> String {
    v.as_f64().map(|s| format_clock(s, true)).unwrap_or_else(|| "–".into())
}

fn print_status(v: &Value) {
    let playing = v["playing"].as_array().cloned().unwrap_or_default();
    if playing.is_empty() {
        println!("Nothing playing.");
    }
    for p in &playing {
        println!(
            "{} {}  {} / {}  (−{} left){}",
            if p["state"] == "paused" { "⏸" } else { "▶" },
            p["title"].as_str().unwrap_or("?"),
            clock(&p["position"]),
            clock(&p["length"]),
            clock(&p["remaining"]),
            p["set"].as_str().map(|s| format!("  [{s}]")).unwrap_or_default()
        );
    }
    let master = v["master_db"].as_f64().map_or("−∞ dB".to_owned(), |d| format!("{d:.1} dB"));
    println!("Set: {}  ·  layout {}  ·  master {master}", v["set"].as_str().unwrap_or("?"), v["layout"].as_str().unwrap_or("?"));
    if let Some(n) = v["next_cue"].as_object() {
        println!("Next cue: #{} {}", n["position"], n["title"].as_str().unwrap_or(""));
    }
    let dev = v["device"].as_str().or(v["device_error"].as_str()).unwrap_or("no output");
    println!("Output: {dev}{}", if v["locked"] == true { "  ·  set locked" } else { "" });
}

fn print_tiles(v: &Value) {
    let tiles = v.as_array().cloned().unwrap_or_default();
    if tiles.is_empty() {
        println!("No tiles.");
        return;
    }
    let width = tiles.iter().filter_map(|t| t["title"].as_str()).map(|t| t.chars().count()).max().unwrap_or(5).clamp(5, 40);
    let mut last_set = "";
    for t in &tiles {
        let set = t["set"].as_str().unwrap_or("");
        if set != last_set {
            println!("{set}");
            last_set = set;
        }
        let state = match t["state"].as_str() {
            Some("stopped") | None => "",
            Some(s) => s,
        };
        // The pad options that are on (D-145).
        let options: Vec<&str> = [("loop", "loop"), ("solo", "solo"), ("pausable", "pausable"), ("one_shot", "one-shot"), ("hold", "hold"), ("disabled", "disabled")]
            .into_iter()
            .filter(|(k, _)| t[*k] == true)
            .map(|(_, word)| word)
            .collect();
        println!(
            "  #{:<3} {:<3} {:<width$}  {:>7}  {:<14} {}",
            t["position"],
            t["key"].as_str().map(|k| k.to_uppercase()).unwrap_or_else(|| "–".into()),
            t["title"].as_str().unwrap_or(""),
            clock(&t["duration"]),
            options.join(" "),
            state
        );
    }
}

fn print_sets(v: &Value) {
    for s in v.as_array().cloned().unwrap_or_default() {
        println!(
            "{} #{} {}  ({} tiles, {}{})",
            if s["current"] == true { "*" } else { " " },
            s["position"],
            s["name"].as_str().unwrap_or(""),
            s["tiles"],
            s["layout"].as_str().unwrap_or(""),
            if s["auto_advance"] == true { ", auto-advance" } else { "" }
        );
    }
}

fn list_commands(opts: &Opts, args: &[String]) -> Result<(), Fail> {
    let mut filter = None;
    let mut markdown = false;
    let mut python = false;
    let mut json = opts.json;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--filter" => filter = Some(it.next().ok_or_else(|| Fail::usage("--filter needs text"))?.to_lowercase()),
            "--markdown" => markdown = true,
            "--python" => python = true,
            "--json" => json = true,
            other => return Err(Fail::usage(format!("commands: unknown option {other}"))),
        }
    }
    if markdown {
        print!("{}", leadstinger::commands::markdown());
        return Ok(());
    }
    if python {
        print!("{}", leadstinger::pyclient::generate());
        return Ok(());
    }
    let shown: Vec<&Spec> = COMMANDS
        .iter()
        .filter(|s| filter.as_ref().is_none_or(|f| s.id.to_lowercase().contains(f) || s.label.to_lowercase().contains(f)))
        .collect();
    if json {
        let v: Vec<Value> = shown
            .iter()
            .map(|s| {
                let summary = leadstinger::schema::read(s.params).ok();
                json!({
                    "id": s.id, "label": s.label, "main": s.main, "params": s.params,
                    "kind": format!("{:?}", s.kind).to_lowercase(), "destructive": s.destructive, "undo": s.undo,
                    "shortcut": (!s.shortcut_text().is_empty()).then(|| s.shortcut_text()),
                    "inputSchema": leadstinger::schema::input_schema(s),
                    "note": summary.as_ref().and_then(|r| r.note.clone()),
                    "returns": summary.and_then(|r| r.returns),
                })
            })
            .collect();
        println!("{}", Value::Array(v));
        return Ok(());
    }
    for (area, specs) in leadstinger::commands::by_area() {
        let specs: Vec<&&Spec> = specs.iter().filter(|s| shown.iter().any(|x| x.id == s.id)).collect();
        if specs.is_empty() {
            continue;
        }
        println!("{area}");
        for s in specs {
            let tag = if s.kind == Kind::Live { " (app)" } else { "" };
            println!("  {:<22} {}{}{}", s.id, s.label.trim_end_matches('…'), tag, if s.destructive { " *destructive*" } else { "" });
        }
    }
    println!("\n`{CLI_NAME} help <command>` shows its parameters. (app) = needs the running app.");
    Ok(())
}

fn help(name: &str) -> Result<(), Fail> {
    let s = spec(alias(name)).ok_or_else(|| Fail::usage(format!("no command {name:?}")))?;
    println!("{}  {}", s.id, s.label.trim_end_matches('…'));
    println!("  parameters: {}", s.params);
    if let Some(m) = s.main {
        println!("  main argument: {m}   e.g. {CLI_NAME} {} <{m}>", s.id);
    }
    println!("  needs: {}{}", s.kind.describe(), if s.destructive { "; destructive (refused by --safe)" } else { "" });
    let keys = s.shortcut_text();
    if !keys.is_empty() {
        println!("  in the app: {keys}");
    }
    Ok(())
}

/// One `{"command": id, "params": {...}}` per line (or a JSON array of them); one
/// result line per step. Stops at the first failure unless `--keep-going`.
fn run_script(opts: &Opts, file: Option<&str>) -> Result<(), Fail> {
    let file = file.ok_or_else(|| Fail::usage("run needs a script file, or - for stdin"))?;
    let mut text = String::new();
    if file == "-" {
        std::io::stdin().read_to_string(&mut text).map_err(|e| Fail::new(FAILED, "io", e.to_string()))?;
    } else {
        text = std::fs::read_to_string(file).map_err(|e| Fail::new(FAILED, "io", format!("{file}: {e}")))?;
    }
    let steps: Vec<Value> = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Array(a)) => a,
        _ => text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
            .map(|l| serde_json::from_str(l).map_err(|e| Fail::usage(format!("not JSON: {l}: {e}"))))
            .collect::<Result<_, _>>()?,
    };
    let mut parsed = Vec::new();
    for (n, step) in steps.iter().enumerate() {
        let id = step["command"].as_str().ok_or_else(|| Fail::usage(format!("step {}: no \"command\"", n + 1)))?;
        let s = spec(alias(id)).ok_or_else(|| Fail::usage(format!("step {}: no command {id:?}", n + 1)))?;
        let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
        check_safe(opts, s, &params)?;
        parsed.push((s, params));
    }
    let need_app = parsed.iter().any(|(s, _)| s.kind == Kind::Live);
    let mut target = Target::open(opts, need_app)?;
    // One batch: one undo step, and unless --keep-going nothing is kept when a step fails.
    let steps: Vec<Value> = parsed.iter().map(|(s, p)| json!({"command": s.id, "params": p})).collect();
    let batch = spec("batch.run").expect("registered");
    let reply = target.call_once(batch, json!({"steps": steps, "atomic": !opts.keep_going, "dryRun": opts.dry_run}))?;
    let mut first_failure = None;
    for step in reply["steps"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let n = step["step"].as_u64().unwrap_or(0);
        let s = step["command"].as_str().and_then(spec).expect("a step names a registered command");
        let r = if step["ok"] == json!(true) {
            target.finish(s, step["result"].clone())
        } else {
            Err(Fail::from_cmd(control::error_from_json(&step["error"])))
        };
        let line = match &r {
            Ok(v) => json!({"step": n, "command": s.id, "ok": true, "result": v}),
            Err(f) => json!({"step": n, "command": s.id, "ok": false, "error": control::error_json(&f.error)}),
        };
        if opts.json {
            println!("{line}");
        } else {
            match &r {
                Ok(_) => println!("{n:>3}. {}  ok", s.id),
                Err(f) => println!("{n:>3}. {}  FAILED: {}", s.id, f.error.message),
            }
        }
        if let Err(f) = r {
            first_failure.get_or_insert(f);
        }
    }
    if opts.dry_run {
        print_changes(opts, &reply["changes"]);
    } else if reply["rolledBack"] == json!(true) {
        eprintln!("{CLI_NAME}: a step failed, so the script's changes to sets and tiles were put back (--keep-going keeps the steps before it)");
    }
    match first_failure {
        Some(f) => Err(Fail { code: f.code, error: CmdError { code: "script_failed", message: f.error.message, candidates: vec![] } }),
        None => Ok(()),
    }
}

fn watch(opts: &Opts) -> Result<(), Fail> {
    let client = Client::connect(&opts.data_dir)
        .map_err(|_| Fail::new(NOT_RUNNING, "not_running", format!("{APP_NAME} isn't running; watch needs the app.")))?;
    let events = client.events().map_err(|e| Fail::new(FAILED, "io", e.to_string()))?;
    let stdout_is_tty = !opts.json;
    for e in events {
        if stdout_is_tty {
            let what = e["title"].as_str().or(e["name"].as_str()).map(|s| format!("  {s}")).unwrap_or_default();
            println!("{}{what}", e["event"].as_str().unwrap_or("?"));
        } else {
            println!("{e}");
        }
        // Flush per line so pipes (jq, scripts) see each event at once.
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    Ok(())
}


/// `serve`: the app without a window, for a show box (docs/CLI.md "Headless"). It opens
/// the library (taking the lock), plays through the output device, answers on the
/// control socket like the app, autosaves, and exits cleanly on SIGTERM/SIGINT or
/// `app.quit`.
fn serve(opts: &Opts, args: &[String]) -> Result<(), Fail> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let mut silent = false;
    let mut device: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--silent" => silent = true,
            "--device" => device = Some(it.next().ok_or_else(|| Fail::usage("--device needs a name"))?.clone()),
            other => return Err(Fail::usage(format!("serve: unknown option {other}"))),
        }
    }
    let dir = &opts.data_dir;
    if control::app_running(dir) {
        return Err(Fail::new(FAILED, "running", format!("{APP_NAME} is already running on {}", dir.display())));
    }
    let _lock = control::lock_library(dir, Duration::from_secs(5))
        .map_err(|e| Fail::new(FAILED, "busy", format!("can't open the library in {}: {e}", dir.display())))?;
    let store = Store::open(dir.clone()).map_err(|e| Fail::new(FAILED, "failed", format!("can't open the library: {e:#}")))?;
    if let Some(w) = &store.load_warning {
        eprintln!("{CLI_NAME}: {w}");
    }
    let engine = if silent {
        Engine::disabled("silent (serve --silent)")
    } else {
        Engine::new(device.as_deref().or(store.library.output_device.as_deref()))
    };
    match (&engine.device_name, &engine.error) {
        (Some(name), _) => eprintln!("{CLI_NAME}: playing through {name} at {} Hz", engine.sample_rate()),
        (None, Some(e)) => eprintln!("{CLI_NAME}: no audio output: {e}"),
        _ => {}
    }
    let mut ctl = Controller::new(store, engine);
    ctl.has_window = false;
    let lender = control::Lender::new(ctl);
    // Before the socket answers: a SIGTERM from then on (MIDI can take a while to start)
    // must still save and exit cleanly.
    let stop = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP] {
        signal_hook::flag::register(sig, stop.clone()).map_err(|e| Fail::new(FAILED, "failed", e.to_string()))?;
    }
    let server = control::serve(lender.clone(), dir, None)
        .map_err(|e| Fail::new(FAILED, "failed", format!("can't listen on the control socket: {e}")))?;
    eprintln!("{CLI_NAME}: serving {} on {}", dir.display(), server.path().display());
    leadstinger::midi::start(lender.clone(), None);
    leadstinger::osc::start(lender.clone(), None);

    let mut last_notice = None;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let quit = lender.with(|c| {
            c.tick_now();
            // Status-bar messages go to the log instead.
            if let Some(n) = &c.notice {
                if last_notice != Some(n.at) {
                    last_notice = Some(n.at);
                    eprintln!("{CLI_NAME}: {}{}", if n.error { "error: " } else { "" }, n.text);
                }
            }
            if let Err(e) = c.store.save_if_due() {
                eprintln!("{CLI_NAME}: couldn't save the library: {e:#}");
            }
            let quit = c.effects.iter().any(|e| matches!(e, leadstinger::controller::Effect::Quit));
            c.effects.clear();
            quit
        });
        if quit || stop.load(Ordering::Relaxed) {
            break;
        }
    }
    eprintln!("{CLI_NAME}: stopping");
    // Other threads still hold the controller, so the store isn't dropped: stop ffmpeg here.
    lender.with(|c| {
        c.store.stop_imports();
        c.store.save_now()
    })
    .map_err(|e| Fail::new(FAILED, "failed", format!("couldn't save the library: {e:#}")))?;
    drop(server);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{alias, check_safe, parse, spec, split_words, Opts};
    use serde_json::json;

    #[test]
    fn splits_like_a_shell_without_expanding() {
        let w = |s: &str| split_words(s).unwrap();
        assert_eq!(w("trigger key:1"), ["trigger", "key:1"]);
        assert_eq!(w(r#"play 'Game Night/Applause'  --fade 0.5"#), ["play", "Game Night/Applause", "--fade", "0.5"]);
        assert_eq!(w(r#"tile.set "Captain's \"Log\"" --title Log\ 1"#), ["tile.set", r#"Captain's "Log""#, "--title", "Log 1"]);
        assert_eq!(w("status; rm -rf ~ $(id)"), ["status;", "rm", "-rf", "~", "$(id)"]);
        assert_eq!(w(r#"x '' """#), ["x", "", ""]);
        assert!(split_words("play 'oops").is_err());
    }

    /// A QuipClipper `--folder` (quip.* and view.quip) is a folder in its clips library:
    /// sent as given, and not checked against `--allow-dir`. A local `--folder` is still
    /// made absolute.
    #[test]
    fn quip_folders_stay_library_paths() {
        let parsed = |a: &[&str]| parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_else(|f| panic!("{}", f.error.message));
        let (s, p) = parsed(&["quip.browse", "Heat (1995)"]);
        assert_eq!(p, json!({"folder": "Heat (1995)"}));
        let safe = Opts { ssh: false, json: true, offline: true, safe: true, keep_going: false, dry_run: false, data_dir: "data".into(), allow_dirs: vec![] };
        assert!(check_safe(&safe, s, &p).is_ok());
        let (s, p) = parsed(&["quip.search", "heat", "--folder", "Movies/Heat (1995)"]);
        assert_eq!(p["folder"], "Movies/Heat (1995)");
        assert!(check_safe(&safe, s, &p).is_ok());
        let (s, p) = parsed(&["view.quip", "--folder", "Movies/Heat (1995)", "--query", "bank"]);
        assert_eq!(p, json!({"folder": "Movies/Heat (1995)", "query": "bank"}));
        assert!(check_safe(&safe, s, &p).is_ok());
        let (s, p) = parsed(&["set.render", "--folder", "sounds"]);
        let folder = p["folder"].as_str().unwrap();
        assert!(std::path::Path::new(folder).is_absolute() && folder.ends_with("sounds"), "{folder}");
        assert!(check_safe(&safe, s, &p).is_err());
    }

    /// An object- or list-valued flag reaches the app under the name the command uses
    /// (`cues_source` stays snake-case); a flag the command doesn't name, or JSON that
    /// doesn't parse, is a usage error instead of being dropped.
    #[test]
    fn flags_keep_their_names_and_json_values() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let ok = |a: &[&str]| parse(&args(a)).unwrap_or_else(|f| panic!("{}", f.error.message)).1;
        let err = |a: &[&str]| parse(&args(a)).err().map(|f| (f.code, f.error.message)).expect("a usage error");
        let p = ok(&["tile.subtitlesSet", "key:1", "--cues", r#"[{"start":0,"end":1,"text":"Hi"}]"#, "--cues_source", r#"{"kind":"manual"}"#]);
        assert_eq!(p, json!({"tile": "key:1", "cues": [{"start": 0, "end": 1, "text": "Hi"}], "cues_source": {"kind": "manual"}}));
        assert_eq!(ok(&["tile.subtitlesSet", "key:1", "--cues-source={\"kind\":\"x\"}"])["cues_source"], json!({"kind": "x"}));
        assert_eq!(ok(&["tile.subtitlesSet", "key:1", "--keep-aligned", "false"])["keepAligned"], json!(false));
        assert_eq!(ok(&["tile.add", "--new-row"])["newRow"], json!(true));
        assert_eq!(ok(&["tile.set", "key:1", "--oneshot"])["oneShot"], json!(true));
        let (code, msg) = err(&["tile.subtitlesSet", "key:1", "--cuesSorce", "{}"]);
        assert!(code == super::USAGE_ERR && msg.contains("--cuesSorce"), "{msg}");
        let (code, msg) = err(&["tile.subtitlesSet", "key:1", "--cues", "[{start:0}]"]);
        assert!(code == super::USAGE_ERR && msg.contains("not valid JSON"), "{msg}");
        assert!(err(&["status", "--verbose"]).1.contains("no option --verbose"));
    }

    /// The `leadstinger-cli …` lines in code blocks and code spans of a Markdown file,
    /// each cut where the shell would end it (a comment, `;`, a pipe, a redirect, or the
    /// quote of a keybind config or authorized_keys line around it), with its line number.
    fn examples(markdown: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut fenced = false;
        for (n, line) in markdown.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            let code: Vec<&str> = if fenced { vec![line] } else { line.split('`').skip(1).step_by(2).collect() };
            for text in code {
                for (at, _) in text.match_indices("leadstinger-cli ") {
                    // `cargo run --bin leadstinger-cli -- …` is cargo's command line.
                    if text[..at].ends_with("--bin ") {
                        continue;
                    }
                    let rest = &text[at + "leadstinger-cli ".len()..];
                    let end = [" #", "; ", " | ", " < ", "\")", "')", "\","].iter().filter_map(|stop| rest.find(stop)).min().unwrap_or(rest.len());
                    out.push((n + 1, rest[..end].trim().to_owned()));
                }
            }
        }
        out
    }

    /// Every CLI example in the docs parses: the command exists, it gets at most one main
    /// argument, and each `--option` is one of its parameters (`leadstinger-cli help`).
    #[test]
    fn doc_examples_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<_> = ["README.md", "CHANGELOG.md", "AGENTS.md", "skills/leadstinger/SKILL.md", "starter/README.md"].iter().map(|f| root.join(f)).collect();
        for dir in ["docs", "examples"] {
            let mut docs: Vec<_> = std::fs::read_dir(root.join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
            docs.sort();
            for p in docs {
                if p.is_dir() {
                    files.push(p.join("README.md"));
                } else if p.extension().is_some_and(|e| e == "md") {
                    files.push(p);
                }
            }
        }
        // Plans (docs/*-PLAN.md) name commands that don't exist yet.
        files.retain(|f| f.exists() && !f.file_name().is_some_and(|n| n.to_string_lossy().ends_with("-PLAN.md")));
        let (mut checked, mut wrong) = (0, Vec::new());
        for file in &files {
            let name = file.strip_prefix(root).unwrap().display().to_string();
            for (line, example) in examples(&std::fs::read_to_string(file).unwrap()) {
                let mut words = split_words(&example).unwrap_or_else(|e| panic!("{name}:{line}: {example}: {e}"));
                // Global options, which main() takes out before parsing.
                let mut i = 0;
                while i < words.len() {
                    match words[i].as_str() {
                        "--json" | "--offline" | "--safe" | "--keep-going" | "--dry-run" | "--ssh" => drop(words.remove(i)),
                        "--data-dir" | "--allow-dir" => drop(words.drain(i..(i + 2).min(words.len()))),
                        _ => i += 1,
                    }
                }
                // Usage lines such as `help <command>` are templates, not examples, and
                // `ln -s …/leadstinger-cli ~/.local/bin/` names a folder, not a command.
                if words.is_empty() || words[0].contains('/') || words.iter().any(|w| w.starts_with(['<', '['])) {
                    continue;
                }
                checked += 1;
                let problem = match words[0].as_str() {
                    "commands" | "run" | "watch" | "serve" | "connect" | "mcp" => None,
                    "help" => words.get(1).filter(|c| spec(alias(c)).is_none()).map(|c| format!("no command {c:?}")),
                    _ => {
                        // `--token -` reads stdin; the test has none to give.
                        if let Some(token) = words.iter().position(|w| w == "--token").and_then(|t| words.get_mut(t + 1)) {
                            *token = "x".into();
                        }
                        match parse(&words) {
                            Err(f) => Some(f.error.message),
                            Ok((s, _)) => {
                                let known = |key: &str| s.params.split(|c: char| !c.is_alphanumeric()).any(|w| w == key);
                                let unknown: Vec<String> = words.iter().filter_map(|w| w.strip_prefix("--")).map(|k| super::camel(k.split('=').next().unwrap())).filter(|k| k != "params" && !known(k)).collect();
                                (!unknown.is_empty()).then(|| format!("{} has no parameter {}", s.id, unknown.join(", ")))
                            }
                        }
                    }
                };
                if let Some(p) = problem {
                    wrong.push(format!("{name}:{line}: leadstinger-cli {example}\n    {p}"));
                }
            }
        }
        assert!(checked > 100, "found only {checked} examples: is the extraction broken?");
        assert!(wrong.is_empty(), "CLI examples in the docs that the CLI refuses:\n{}", wrong.join("\n"));
    }
}
