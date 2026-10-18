//! `leadstinger-cli`: every command of the app from a shell (docs/CLI.md).
//!
//! It runs commands in the running app over the control socket, or, when the app is
//! closed, opens the library itself (taking the library lock) for the commands that
//! don't need the app.

use serde_json::{json, Map, Value};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use leadstinger::audio::Engine;
use leadstinger::brand::{APP_NAME, CLI_NAME};
use leadstinger::commands::{spec, Kind, Spec, COMMANDS};
use leadstinger::control::{self, Client};
use leadstinger::controller::{CmdError, Controller};
use leadstinger::logic::format_clock;
use leadstinger::store::Store;

fn usage() -> String {
    format!(
        "\
{CLI_NAME}: run any {APP_NAME} command (docs/CLI.md).

USAGE:
  {CLI_NAME} <command> [<main argument>…] [--param value]… [--params JSON]
  {CLI_NAME} commands [--filter TEXT] [--json | --markdown]
  {CLI_NAME} help <command>
  {CLI_NAME} run <script.jsonl | ->
  {CLI_NAME} watch
  {CLI_NAME} serve [--silent] [--device NAME]   headless: play and serve the socket, no window

Shortcuts: status, list, sets, trigger, play, stop, stop-all, go, add.
Options: --json  --offline  --data-dir DIR  --safe  --allow-dir DIR  --keep-going
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
    "event", "pictureLayer", "pictureFit",
];

struct Opts {
    /// Forced-command mode for an SSH key (docs/CLI.md "Remote agents").
    ssh: bool,
    json: bool,
    offline: bool,
    safe: bool,
    keep_going: bool,
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
    if words.first().is_some_and(|w| w == CLI_NAME || w == "sttnger-cli") {
        words.remove(0);
    }
    let mut out = Vec::new();
    for w in words {
        match w.as_str() {
            "--json" | "--safe" => {}
            "--keep-going" => opts.keep_going = true,
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

fn typed(key: &str, raw: &str) -> Value {
    if raw == "null" {
        return Value::Null;
    }
    if TEXT_KEYS.contains(&key) {
        return Value::String(raw.to_owned());
    }
    let looks_json = raw == "true"
        || raw == "false"
        || raw.starts_with(['[', '{', '"'])
        || raw.parse::<f64>().is_ok();
    if looks_json {
        if let Ok(v) = serde_json::from_str(raw) {
            return v;
        }
    }
    Value::String(raw.to_owned())
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
            let key = camel(k);
            let value = match inline {
                Some(v) => typed(&key, &v),
                None => match args.get(i + 1).filter(|n| !n.starts_with("--")) {
                    Some(v) => {
                        i += 1;
                        typed(&key, v)
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
            typed(main, &positional[0])
        } else if positional.len() == 2 && main != "tile" && spec.params.starts_with("{tile") {
            // `tile.picture <tile> <path>`, `tile.replaceAudio <tile> <path>`
            params.insert("tile".into(), json!(positional[0]));
            typed(main, &positional[1])
        } else {
            return Err(Fail::usage(format!("{} takes one main argument ({main}); quote names with spaces", spec.id)));
        };
        params.insert(main.to_owned(), v);
    }
    // Paths are relative to the caller, not to the app.
    if let Some(Value::String(p)) = params.get("path") {
        params.insert("path".into(), json!(absolute(p)));
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
fn check_safe(opts: &Opts, spec: &Spec, params: &Value) -> Result<(), Fail> {
    if !opts.safe {
        return Ok(());
    }
    if spec.destructive {
        return Err(Fail::new(REFUSED, "refused", format!("{} is destructive; --safe refuses it", spec.id)));
    }
    if spec.id == "app.quit" {
        return Err(Fail::new(REFUSED, "refused", "--safe doesn't quit the app; ask whoever is at it"));
    }
    let mut paths: Vec<&str> = Vec::new();
    for key in ["path", "paths"] {
        match params.get(key) {
            Some(Value::String(s)) => paths.push(s),
            Some(Value::Array(a)) => paths.extend(a.iter().filter_map(Value::as_str)),
            _ => {}
        }
    }
    for p in paths {
        let real = Path::new(p).canonicalize().unwrap_or_else(|_| PathBuf::from(p));
        if !opts.allow_dirs.iter().any(|d| real.starts_with(d)) {
            return Err(Fail::new(REFUSED, "refused", format!("--safe: {p} isn't inside an --allow-dir folder")));
        }
    }
    Ok(())
}

/// Where commands run: the app over its socket, or the library opened here.
enum Target {
    App(Client),
    Library { ctl: Box<Controller>, _lock: std::fs::File },
}

impl Target {
    fn open(opts: &Opts, need_app: bool) -> Result<Target, Fail> {
        if !opts.offline {
            if let Ok(c) = Client::connect(&opts.data_dir) {
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

    fn call(&mut self, spec: &Spec, params: Value) -> Result<Value, Fail> {
        match self {
            Target::App(c) => c.call(spec.id, params).map_err(Fail::from_cmd),
            Target::Library { ctl, .. } => {
                if spec.kind == Kind::Live {
                    return Err(Fail::new(NOT_RUNNING, "not_running", format!("{} needs the running app", spec.id)));
                }
                let r = ctl.execute(spec.id, params).map_err(Fail::from_cmd);
                ctl.effects.clear();
                if ctl.store.has_unsaved() {
                    ctl.store.save_now().map_err(|e| Fail::new(FAILED, "failed", format!("couldn't save the library: {e:#}")))?;
                }
                r
            }
        }
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
        _ if v.as_object().is_some_and(|m| m.is_empty()) => {}
        _ => println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()),
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
    println!("Output: {dev}{}", if v["edit_mode"] == true { "  ·  Edit mode" } else { "" });
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
        println!(
            "  #{:<3} {:<3} {:<width$}  {:>7}  {:<8} {}",
            t["position"],
            t["key"].as_str().map(|k| k.to_uppercase()).unwrap_or_else(|| "–".into()),
            t["title"].as_str().unwrap_or(""),
            clock(&t["duration"]),
            t["mode"].as_str().unwrap_or(""),
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
    let mut json = opts.json;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--filter" => filter = Some(it.next().ok_or_else(|| Fail::usage("--filter needs text"))?.to_lowercase()),
            "--markdown" => markdown = true,
            "--json" => json = true,
            other => return Err(Fail::usage(format!("commands: unknown option {other}"))),
        }
    }
    if markdown {
        print!("{}", leadstinger::commands::markdown());
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
                json!({
                    "id": s.id, "label": s.label, "main": s.main, "params": s.params,
                    "kind": format!("{:?}", s.kind).to_lowercase(), "destructive": s.destructive,
                    "shortcut": (!s.shortcut_text().is_empty()).then(|| s.shortcut_text()),
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
    let mut first_failure = None;
    for (n, (s, params)) in parsed.into_iter().enumerate() {
        let r = target.call(s, params);
        let line = match &r {
            Ok(v) => json!({"step": n + 1, "command": s.id, "ok": true, "result": v}),
            Err(f) => json!({"step": n + 1, "command": s.id, "ok": false, "error": control::error_json(&f.error)}),
        };
        if opts.json {
            println!("{line}");
        } else {
            match &r {
                Ok(_) => println!("{:>3}. {}  ok", n + 1, s.id),
                Err(f) => println!("{:>3}. {}  FAILED: {}", n + 1, s.id, f.error.message),
            }
        }
        if let Err(f) = r {
            if first_failure.is_none() {
                first_failure = Some(f);
            }
            if !opts.keep_going {
                break;
            }
        }
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
    let server = control::serve(lender.clone(), dir, None)
        .map_err(|e| Fail::new(FAILED, "failed", format!("can't listen on the control socket: {e}")))?;
    eprintln!("{CLI_NAME}: serving {} on {}", dir.display(), server.path().display());

    let stop = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP] {
        signal_hook::flag::register(sig, stop.clone()).map_err(|e| Fail::new(FAILED, "failed", e.to_string()))?;
    }
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
    lender.with(|c| c.store.save_now()).map_err(|e| Fail::new(FAILED, "failed", format!("couldn't save the library: {e:#}")))?;
    drop(server);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::split_words;

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
}
