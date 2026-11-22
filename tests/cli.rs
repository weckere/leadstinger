//! End-to-end: the real `leadstinger-cli` binary against a library on disk (offline) and
//! against a control socket served from this process (the app's side, minus the window).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use leadstinger::audio::Engine;
use leadstinger::control::{self, Lender};
use leadstinger::controller::Controller;
use leadstinger::store::Store;

/// A short temp dir (Unix socket paths must stay under ~104 bytes).
fn temp(name: &str) -> PathBuf {
    let d = PathBuf::from("/tmp").join(format!("leadstinger-cli-{name}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Out {
    code: i32,
    json: Value,
    stderr: String,
}

fn cli(data: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .arg("--data-dir")
        .arg(data)
        .arg("--json")
        .args(args)
        .env_remove("LEADSTINGER_DATA_DIR")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    Out {
        code: out.status.code().unwrap_or(-1),
        json: stdout.lines().last().and_then(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn tones(dir: &Path, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|n| {
            let p = dir.join(format!("{n}.wav"));
            leadstinger::wav::write_tone(&p, 8000, 0.4, &[440.0], 0.4).unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect()
}

#[test]
fn offline_library_commands() {
    let root = temp("off");
    let data = root.join("data");
    let files = tones(&root, &["Air Horn", "Applause"]);

    let r = cli(&data, &["add", &files[0], &files[1]]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json["added"].as_array().map(Vec::len), Some(2));

    assert_eq!(cli(&data, &["set.create", "Encore"]).code, 0);
    assert_eq!(cli(&data, &["set.select", "#1"]).code, 0);
    let r = cli(&data, &["tile.set", "Air Horn", "--mode", "hold", "--fade-out", "0.5", "--hotkey", "r"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!((r.json["mode"].as_str(), r.json["fade_out"].as_f64(), r.json["key"].as_str()), (Some("hold"), Some(0.5), Some("r")));
    // The pad options as flags (D-145): bare flags turn them on.
    let r = cli(&data, &["tile.set", "Air Horn", "--pausable", "--loop"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!((r.json["hold"].as_bool(), r.json["pausable"].as_bool(), r.json["loop"].as_bool()), (Some(true), Some(true), Some(true)));
    let r = cli(&data, &["tile.set", "Air Horn", "--oneshot", "--hold", "false"]);
    assert_eq!((r.json["hold"].as_bool(), r.json["pausable"].as_bool(), r.json["one_shot"].as_bool()), (Some(false), Some(false), Some(true)), "{}", r.stderr);
    assert_ne!(cli(&data, &["tile.set", "Air Horn", "--pausable", "--one-shot"]).code, 0, "both at once is refused");
    cli(&data, &["tile.set", "Air Horn", "--mode", "hold"]);

    // Two positionals: <tile> <path>.
    let pic = root.join("pic.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 40, 40, 255])).save(&pic).unwrap();
    let r = cli(&data, &["tile.picture", "Applause", pic.to_str().unwrap()]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.stderr);
    assert_eq!(cli(&data, &["tile.get", "Applause"]).json["picture"], true);

    let tiles = cli(&data, &["list"]).json;
    assert_eq!(tiles[0]["title"], "Air Horn");
    assert_eq!(tiles[1]["audio"], "not_loaded"); // offline doesn't decode
    let sets = cli(&data, &["sets"]).json;
    assert_eq!(sets[1]["name"], "Encore");

    // Live commands need the app.
    let r = cli(&data, &["trigger", "Applause"]);
    assert_eq!(r.code, 3, "{}", r.json);
    assert_eq!(r.json["error"]["code"], "not_running");
    // No file picker without the app.
    assert_eq!(cli(&data, &["tile.add"]).code, 2);
    // --safe refuses destructive commands and files outside --allow-dir.
    assert_eq!(cli(&data, &["--safe", "tile.remove", "Applause"]).code, 5);
    assert_eq!(cli(&data, &["--safe", "add", &files[0]]).code, 5);
    assert_eq!(cli(&data, &["--safe", "--allow-dir", root.to_str().unwrap(), "add", &files[0]]).code, 0);
    // Two "Air Horn"s now: ambiguous, with candidates.
    let r = cli(&data, &["tile.get", "Air Horn"]);
    assert_eq!(r.code, 4);
    assert_eq!(r.json["error"]["candidates"].as_array().map(Vec::len), Some(2));
    // Usage errors.
    assert_eq!(cli(&data, &["tile.sett"]).code, 2);

    // --ssh (a forced command): the words come from SSH_ORIGINAL_COMMAND, always --safe.
    let ssh = |remote: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
            .args(["--ssh", "--data-dir", data.to_str().unwrap()])
            .env("SSH_ORIGINAL_COMMAND", remote)
            .output()
            .unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let (code, out) = ssh("leadstinger-cli sets");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Encore"));
    assert_eq!(ssh("tile.set 'Applause' --title 'Applause!'").0, 0);
    assert_eq!(ssh("tile.remove 'Applause!'").0, 5, "destructive");
    assert_eq!(ssh("app.quit").0, 5);
    assert_eq!(ssh("--data-dir /tmp status").0, 5, "can't pick another library");
    assert_eq!(ssh("--allow-dir / add /etc/hosts").0, 5);
    assert_eq!(ssh(&format!("add {}", files[1])).0, 5, "files only from --allow-dir");
    assert_eq!(ssh("run /etc/passwd").0, 5, "scripts only on stdin");
    assert_eq!(ssh("sets 'unterminated").0, 2);

    // Everything above was saved.
    let lib: Value = serde_json::from_slice(&std::fs::read(data.join("Library.json")).unwrap()).unwrap();
    assert_eq!(lib["sets"].as_array().map(Vec::len), Some(2));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn commands_reach_the_running_app() {
    let root = temp("on");
    let data = root.join("data");
    // Long enough that it can't finish on its own before the test stops it, even on a
    // loaded build machine (the watcher must see tile.stopped, not tile.finished).
    let long = root.join("Rimshot.wav");
    leadstinger::wav::write_tone(&long, 8000, 10.0, &[440.0], 0.4).unwrap();
    let files = [long.to_string_lossy().into_owned()];
    let lender = Lender::new(Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test")));
    let _lock = control::lock_library(&data, Duration::ZERO).unwrap(); // the app holds it
    let _server = control::serve(lender.clone(), &data, None).unwrap();

    let r = cli(&data, &["add", &files[0]]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.stderr);
    // Wait for the decode, then play it through the socket.
    let deadline = Instant::now() + Duration::from_secs(10);
    while lender.with(|c| c.store.is_loading()) && Instant::now() < deadline {
        lender.with(|c| c.tick_now());
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = cli(&data, &["trigger", "rimshot"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.stderr);
    assert_eq!(r.json["state"], "playing");
    let status = cli(&data, &["status"]).json;
    assert_eq!(status["playing"][0]["title"], "Rimshot");

    // A watcher sees the stop.
    let mut watcher = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .args(["--data-dir", data.to_str().unwrap(), "--json", "watch"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(cli(&data, &["stop-all", "--immediate"]).code, 0);
    std::thread::sleep(Duration::from_millis(300));
    lender.with(|c| c.tick_now());
    std::thread::sleep(Duration::from_millis(200));
    watcher.kill().ok();
    let out = watcher.wait_with_output().unwrap();
    let events: Vec<Value> =
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert!(events.iter().any(|e| e["event"] == "tile.stopped" && e["title"] == "Rimshot"), "{events:?}");

    // The Random button's command (D-171): the set's only tile, every time, and the app
    // says which it played.
    for _ in 0..2 {
        let r = cli(&data, &["tile.random"]);
        assert_eq!((r.code, r.json["title"].as_str()), (0, Some("Rimshot")), "{} {}", r.json, r.stderr);
    }
    assert_eq!(lender.with(|c| c.notice.as_ref().map(|n| n.text.clone())).as_deref(), Some("Random: playing Rimshot"));
    assert_eq!(cli(&data, &["stop-all", "--immediate"]).code, 0);

    // A script: one result line per step, stopping at the first failure. Its changes to
    // sets and tiles are put back (one batch, atomic); a setting such as the theme isn't.
    let script = root.join("show.jsonl");
    std::fs::write(
        &script,
        [
            json!({"command": "set.create", "params": {"name": "Game Night"}}),
            json!({"command": "view.theme", "params": {"theme": "padd"}}),
            json!({"command": "tile.get", "params": {"tile": "nope"}}),
            json!({"command": "set.create", "params": {"name": "never"}}),
        ]
        .map(|v| v.to_string())
        .join("\n"),
    )
    .unwrap();
    let r = cli(&data, &["run", script.to_str().unwrap()]);
    assert_eq!(r.code, 1);
    assert_eq!(r.json["error"]["code"], "script_failed");
    assert_eq!(lender.with(|c| c.store.library.sets.len()), 1, "Game Night was put back, never never ran");
    assert_eq!(lender.with(|c| c.store.library.theme), leadstinger::model::ThemeKind::Panel);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn serve_runs_headless_and_saves_on_sigterm() {
    let root = temp("srv");
    let data = root.join("data");
    let files = tones(&root, &["Air Horn"]);
    let mut server = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .args(["--data-dir", data.to_str().unwrap(), "serve", "--silent"])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !control::app_running(&data) {
        assert!(Instant::now() < deadline, "serve never answered");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(cli(&data, &["status"]).json["window"], false);
    assert_eq!(cli(&data, &["add", &files[0]]).code, 0);
    assert_eq!(cli(&data, &["set.rename", "Show Box"]).code, 0);
    let r = cli(&data, &["tile.add"]);
    assert_eq!((r.code, r.json["error"]["code"].as_str()), (2, Some("no_window")));
    let deadline = Instant::now() + Duration::from_secs(10);
    while cli(&data, &["trigger", "Air Horn"]).json["state"] != "playing" {
        assert!(Instant::now() < deadline, "never played");
        std::thread::sleep(Duration::from_millis(50));
    }

    Command::new("kill").args(["-TERM", &server.id().to_string()]).status().unwrap();
    let status = server.wait().unwrap();
    assert!(status.success(), "serve exited with {status}");
    let lib: Value = serde_json::from_slice(&std::fs::read(data.join("Library.json")).unwrap()).unwrap();
    assert_eq!(lib["sets"][0]["name"], "Show Box", "saved on SIGTERM");
    assert!(!control::socket_path(&data).exists(), "socket removed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn set_files_move_sets_between_libraries() {
    let root = temp("sets");
    let (a, b) = (root.join("a"), root.join("b"));
    let files = tones(&root, &["Air Horn", "Rimshot"]);
    assert_eq!(cli(&a, &["set.create", "Game Night"]).code, 0);
    assert_eq!(cli(&a, &["add", &files[0], &files[1], "--set", "Game Night"]).code, 0);
    assert_eq!(cli(&a, &["tile.set", "Rimshot", "--mode", "restart", "--fade-out", "0.3", "--hotkey", "r"]).code, 0);

    // With the app closed, export writes one file; .lsset is added to a bare name.
    let r = cli(&a, &["set.export", root.join("night").to_str().unwrap(), "--set", "Game Night"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let file = root.join("night.lsset");
    assert_eq!((r.json["path"].as_str(), r.json["tiles"].as_u64()), (file.to_str(), Some(2)));

    // A fresh library imports it as a new set, settings and all.
    let r = cli(&b, &["set.import", file.to_str().unwrap()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json["sets"][0]["name"], "Game Night");
    let sets = cli(&b, &["sets"]).json;
    assert_eq!(sets.as_array().map(Vec::len), Some(1), "the new library's empty Set 1 is replaced: {sets}");
    let t = cli(&b, &["tile.get", "Game Night/Rimshot"]).json;
    assert_eq!((t["mode"].as_str(), t["key"].as_str()), (Some("restart"), Some("r")), "{t}");
    assert!((t["fade_out"].as_f64().unwrap() - 0.3).abs() < 1e-6, "{t}");

    // --safe keeps both inside --allow-dir.
    assert_eq!(cli(&b, &["--safe", "set.import", file.to_str().unwrap()]).code, 5);
    std::fs::remove_dir_all(root).ok();
}

/// Check for Updates from the CLI, offline, against a saved GitHub answer (never the
/// network): a newer release gives its page and notes; the same version is "latest"; a
/// bad answer fails the command (D-206).
#[test]
fn check_for_updates_reads_the_latest_release() {
    let dir = temp("updates");
    let run = |tag: &str| {
        let feed = dir.join(format!("{tag}.json"));
        std::fs::write(
            &feed,
            json!({"tag_name": tag, "draft": false, "prerelease": false, "html_url": format!("https://github.com/weckere/leadstinger/releases/tag/{tag}"), "body": "- New things"}).to_string(),
        )
        .unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"));
        cmd.arg("--data-dir").arg(&dir).arg("--offline").arg("--json").arg("app.checkForUpdates").env_remove("LEADSTINGER_DATA_DIR").env("LEADSTINGER_UPDATE_JSON", &feed);
        cmd.output().unwrap()
    };
    let newer = run("v999.0.0");
    assert!(newer.status.success(), "{}", String::from_utf8_lossy(&newer.stderr));
    let v: Value = serde_json::from_slice(&newer.stdout).unwrap();
    assert_eq!((v["status"].as_str(), v["latest"].as_str(), v["notes"].as_str()), (Some("newer"), Some("999.0.0"), Some("- New things")));
    assert_eq!(v["url"], "https://github.com/weckere/leadstinger/releases/tag/v999.0.0");

    let same = run(&format!("v{}", env!("CARGO_PKG_VERSION")));
    let v: Value = serde_json::from_slice(&same.stdout).unwrap();
    assert_eq!(v["status"], "latest");

    let bad = run("nightly");
    assert_eq!(bad.status.code(), Some(1));
    std::fs::remove_dir_all(dir).ok();
}

/// The socket holds a connection to the policy it asked for: `--safe` is refused in the
/// app too, a connection can't widen its policy again, and others stay unrestricted. A
/// `run` script reaches the app as one batch, so it undoes in one step.
#[test]
fn the_socket_keeps_a_connections_policy() {
    let root = temp("policy");
    let data = root.join("data");
    let lender = Lender::new(Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test")));
    let _lock = control::lock_library(&data, Duration::ZERO).unwrap();
    let _server = control::serve(lender.clone(), &data, None).unwrap();

    let mut c = control::Client::connect(&data).unwrap();
    c.call("set.create", json!({"name": "Doomed"})).unwrap();
    assert_eq!(c.call("policy", json!({"safe": true})).unwrap()["safe"], true);
    assert_eq!(c.call("set.delete", json!({"set": "Doomed"})).unwrap_err().code, "refused");
    assert_eq!(c.call("policy", json!({"safe": false})).unwrap()["safe"], true, "it can't widen itself");
    let batch = json!({"steps": [{"command": "set.delete", "params": {"set": "Doomed"}}]});
    assert_eq!(c.call("batch.run", batch.clone()).unwrap_err().code, "refused");
    // Another connection is the user's own.
    let mut other = control::Client::connect(&data).unwrap();
    assert_eq!(other.call("batch.run", batch).unwrap()["ok"], true);

    // The CLI's --safe asks for the policy before it sends anything.
    let r = cli(&data, &["--safe", "set.delete", "#1"]);
    assert_eq!(r.code, 5, "{} {}", r.json, r.stderr);

    let script = root.join("script.jsonl");
    std::fs::write(&script, "{\"command\": \"set.create\", \"params\": {\"name\": \"One\"}}\n{\"command\": \"set.create\", \"params\": {\"name\": \"Two\"}}\n").unwrap();
    let r = cli(&data, &["run", script.to_str().unwrap()]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.stderr);
    let undone = cli(&data, &["edit.undo"]);
    assert_eq!(undone.json["undid"], "Script", "{} {}", undone.json, undone.stderr);
    let names: Vec<String> = lender.with(|c| c.store.library.sets.iter().map(|s| s.name.clone()).collect());
    assert!(!names.contains(&"One".to_owned()) && !names.contains(&"Two".to_owned()), "{names:?}");

    // A failing script keeps nothing unless --keep-going.
    std::fs::write(&script, "{\"command\": \"set.create\", \"params\": {\"name\": \"Three\"}}\n{\"command\": \"set.rename\", \"params\": {\"set\": \"Nope\", \"name\": \"x\"}}\n").unwrap();
    let r = cli(&data, &["run", script.to_str().unwrap()]);
    assert_eq!(r.code, 1, "{} {}", r.json, r.stderr);
    assert!(r.stderr.contains("put back"), "{}", r.stderr);
    assert!(!lender.with(|c| c.store.library.sets.iter().any(|s| s.name == "Three")));
    let r = cli(&data, &["--keep-going", "run", script.to_str().unwrap()]);
    assert_eq!(r.code, 1);
    assert!(lender.with(|c| c.store.library.sets.iter().any(|s| s.name == "Three")));
}

/// `run --dry-run` reports what a script would change, and changes nothing (offline too).
#[test]
fn a_dry_run_script_changes_nothing() {
    let root = temp("dry");
    let data = root.join("data");
    let script = root.join("s.jsonl");
    std::fs::write(&script, "{\"command\": \"set.create\", \"params\": {\"name\": \"Rehearsal\"}}\n").unwrap();
    let r = cli(&data, &["run", "--dry-run", script.to_str().unwrap()]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.stderr);
    assert_eq!(r.json["dryRun"], true);
    assert_eq!(r.json["changes"]["sets"]["added"][0]["name"], "Rehearsal");
    let sets = cli(&data, &["set.list"]).json;
    assert!(sets.as_array().unwrap().iter().all(|s| s["name"] != "Rehearsal"), "{sets}");
}

/// `connect` speaks the socket protocol on stdin/stdout: `lines` in, one reply each out.
fn connect(data: &Path, args: &[&str], env: &[(&str, &str)], lines: &[Value]) -> Vec<Value> {
    use std::io::Write;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"));
    cmd.arg("--data-dir").arg(data).args(args).env_remove("LEADSTINGER_DATA_DIR");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// `connect` reaches the running app (with `--safe` set on the connection first, so it
/// can't be widened), or the library when the app is closed; `--ssh connect` is safe.
#[test]
fn connect_reaches_the_app_or_the_library() {
    let root = temp("connect");
    let data = root.join("data");
    let req = |id: u64, command: &str, params: Value| json!({"id": id, "command": command, "params": params});

    // Offline: the library, locked here; what needs the app answers not_running.
    let r = connect(&data, &["connect"], &[], &[
        req(1, "hello", json!({})),
        req(2, "set.create", json!({"name": "Kept"})),
        req(3, "tile.play", json!({"tile": "x"})),
        req(4, "batch.run", json!({"steps": [{"command": "transport.stopAll"}]})),
        req(5, "events.subscribe", json!({})),
    ]);
    assert_eq!(r[0]["result"]["offline"], true, "{r:?}");
    assert_eq!(r[1]["ok"], true, "{r:?}");
    for (i, reply) in r.iter().enumerate().skip(2) {
        assert_eq!(reply["error"]["code"], "not_running", "{i}: {reply}");
    }
    assert!(cli(&data, &["set.list"]).json.as_array().unwrap().iter().any(|s| s["name"] == "Kept"), "saved");

    // The app: --safe is the connection's policy, and a policy message can't widen it.
    let lender = Lender::new(Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test")));
    let _lock = control::lock_library(&data, Duration::ZERO).unwrap();
    let _server = control::serve(lender.clone(), &data, None).unwrap();
    let r = connect(&data, &["--safe", "connect"], &[], &[
        req(1, "hello", json!({})),
        req(2, "policy", json!({"safe": false})),
        req(3, "set.delete", json!({"set": "Kept"})),
        req(4, "set.create", json!({"name": "Live"})),
    ]);
    assert_eq!(r[0]["result"]["offline"], Value::Null, "the app answers, not the library: {r:?}");
    assert_eq!(r[1]["result"]["safe"], true, "{r:?}");
    assert_eq!(r[2]["error"]["code"], "refused", "{r:?}");
    assert_eq!(r[3]["ok"], true, "{r:?}");
    assert!(lender.with(|c| c.store.library.sets.iter().any(|s| s.name == "Live")));

    // An SSH key's forced command: connect is allowed, and always safe.
    let r = connect(&data, &["--ssh"], &[("SSH_ORIGINAL_COMMAND", "connect")], &[req(1, "set.delete", json!({"set": "Kept"}))]);
    assert_eq!(r[0]["error"]["code"], "refused", "{r:?}");
    assert!(lender.with(|c| c.store.library.sets.iter().any(|s| s.name == "Kept")));
    std::fs::remove_dir_all(root).ok();
}

/// The generated Python client (clients/python) drives the app through `connect`. Skipped
/// where there's no python3 (the Nix shell has none; macOS and most Linux hosts do).
#[test]
fn the_python_client_drives_the_app() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("no python3: the Python client test is skipped");
        return;
    }
    let root = temp("python");
    let data = root.join("data");
    let files = tones(&root, &["Rimshot"]);
    let lender = Lender::new(Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test")));
    let _lock = control::lock_library(&data, Duration::ZERO).unwrap();
    let _server = control::serve(lender.clone(), &data, None).unwrap();
    let script = r#"
import json, sys, leadstinger
data, wav = sys.argv[1], sys.argv[2]
out = {}
with leadstinger.connect(data_dir=data) as ls:
    out["offline"] = ls.hello().get("offline")
    ls.tile.add([wav])
    out["tiles"] = [t["title"] for t in ls.tile.list()]
    with ls.batch() as b:
        b.set.create("Show")
        b.tile.set("Rimshot", title="Rim")
    out["undo"] = b.result["undo"]
    try:
        ls.tile.get("nope")
    except leadstinger.Error as e:
        out["error"] = e.code
    try:
        with ls.batch() as b:
            b.set.create("Half")
            b.set.rename("x", set="Nope")
    except leadstinger.Error as e:
        out["batch_error"] = [e.code, e.result["rolledBack"]]
with leadstinger.connect(data_dir=data, safe=True) as ls:
    try:
        ls.set.delete("Show")
    except leadstinger.Error as e:
        out["safe"] = e.code
print(json.dumps(out))
"#;
    let out = Command::new("python3")
        .args(["-c", script, data.to_str().unwrap(), &files[0]])
        .env("PYTHONPATH", Path::new(env!("CARGO_MANIFEST_DIR")).join("clients/python"))
        .env("LEADSTINGER_CLI", env!("CARGO_BIN_EXE_leadstinger-cli"))
        .env_remove("LEADSTINGER_DATA_DIR")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let r: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {stdout} {}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(r["offline"], Value::Null, "{r}");
    assert_eq!(r["tiles"], json!(["Rimshot"]), "{r}");
    assert_eq!(r["undo"], "Script", "{r}");
    assert_eq!(r["error"], "not_found", "{r}");
    assert_eq!(r["batch_error"], json!(["not_found", true]), "{r}");
    assert_eq!(r["safe"], "refused", "{r}");
    let names: Vec<String> = lender.with(|c| c.store.library.sets.iter().map(|s| s.name.clone()).collect());
    assert!(names.contains(&"Show".to_owned()) && !names.contains(&"Half".to_owned()), "{names:?}");
    assert!(lender.with(|c| c.store.library.sets.iter().flat_map(|s| &s.tiles).any(|t| t.title == "Rim")));
    std::fs::remove_dir_all(root).ok();
}

/// `mcp` is an MCP server on stdio: the handshake, four tools, each call reaching the
/// library (offline, locked only for the call) or the app, destructive commands refused.
#[test]
fn mcp_serves_the_registry() {
    let root = temp("mcp");
    let data = root.join("data");
    let rpc = |id: u64, method: &str, params: Value| json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let tool = |id: u64, name: &str, args: Value| rpc(id, "tools/call", json!({"name": name, "arguments": args}));
    let session = |args: &[&str], calls: Vec<Value>| {
        let mut lines = vec![
            rpc(0, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ];
        lines.extend(calls);
        connect(&data, args, &[], &lines)
    };

    // Offline: the library, one call at a time.
    let r = session(&["mcp"], vec![
        rpc(1, "tools/list", json!({})),
        tool(2, "run", json!({"command": "set.create", "params": {"name": "Show"}})),
        tool(3, "query", json!({"command": "set.list"})),
        tool(4, "run", json!({"command": "set.delete", "params": {"set": "Show"}})),
        tool(5, "run", json!({"command": "tile.play", "params": {"tile": "x"}})),
        tool(6, "screenshot", json!({})),
    ]);
    assert_eq!(r.len(), 7, "one reply per request, nothing else on stdout: {r:?}");
    assert_eq!(r[0]["result"]["serverInfo"]["name"], "leadstinger", "{r:?}");
    let tools: Vec<&str> = r[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(tools, ["commands", "query", "run", "screenshot"]);
    assert_eq!(r[2]["result"]["isError"], Value::Null, "{}", r[2]);
    assert!(r[3]["result"]["structuredContent"]["result"].as_array().unwrap().iter().any(|s| s["name"] == "Show"), "{}", r[3]);
    for (i, code) in [(4, "refused"), (5, "not_running"), (6, "not_running")] {
        assert_eq!(r[i]["result"]["isError"], true, "{}", r[i]);
        assert!(r[i]["result"]["content"][0]["text"].as_str().unwrap().contains(code), "{i}: {}", r[i]);
    }
    // The library isn't held between calls: the one-shot CLI gets it straight away.
    assert_eq!(cli(&data, &["set.list"]).code, 0);

    // The app (headless, as `serve` runs it): --safe holds, and a batch is one undo step;
    // screenshots need a window.
    let mut ctl = Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test"));
    ctl.has_window = false;
    let lender = Lender::new(ctl);
    let _lock = control::lock_library(&data, Duration::ZERO).unwrap();
    let _server = control::serve(lender.clone(), &data, None).unwrap();
    let r = session(&["--safe", "mcp"], vec![
        tool(1, "run", json!({"steps": [{"command": "set.create", "params": {"name": "A"}}, {"command": "set.create", "params": {"name": "B"}}]})),
        tool(2, "run", json!({"command": "tile.add", "params": {"paths": ["/etc/hosts"]}})),
        tool(3, "screenshot", json!({})),
        tool(4, "commands", json!({"id": "tile.trigger"})),
    ]);
    assert_eq!(r[1]["result"]["structuredContent"]["undo"], "Script", "{}", r[1]);
    assert!(r[2]["result"]["content"][0]["text"].as_str().unwrap().contains("refused"), "{}", r[2]);
    assert!(r[3]["result"]["content"][0]["text"].as_str().unwrap().contains("no_window"), "{}", r[3]);
    assert_eq!(r[4]["result"]["structuredContent"]["inputSchema"]["required"], json!(["tile"]), "{}", r[4]);
    assert!(lender.with(|c| c.store.library.sets.iter().any(|s| s.name == "B")));

    // Over an SSH forced command it may run, but never with --allow-destructive.
    let r = connect(&data, &["--ssh"], &[("SSH_ORIGINAL_COMMAND", "mcp --allow-destructive")], &[]);
    assert!(r.is_empty() || r[0]["error"]["code"] == "refused", "{r:?}");
    let refused = Command::new(env!("CARGO_BIN_EXE_leadstinger-cli"))
        .args(["--data-dir", data.to_str().unwrap(), "--ssh"])
        .env("SSH_ORIGINAL_COMMAND", "mcp --allow-destructive")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(5));
    std::fs::remove_dir_all(root).ok();
}
