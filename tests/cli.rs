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
        .env_remove("STTNGER_DATA_DIR")
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

    // A script: one result line per step, stopping at the first failure.
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
    assert_eq!(lender.with(|c| c.store.library.sets.len()), 2, "stopped before the last step");
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
    let t = cli(&b, &["tile.get", "Game Night/Rimshot"]).json;
    assert_eq!((t["mode"].as_str(), t["key"].as_str()), (Some("restart"), Some("r")), "{t}");
    assert!((t["fade_out"].as_f64().unwrap() - 0.3).abs() < 1e-6, "{t}");

    // --safe keeps both inside --allow-dir.
    assert_eq!(cli(&b, &["--safe", "set.import", file.to_str().unwrap()]).code, 5);
    std::fs::remove_dir_all(root).ok();
}
