use std::time::Duration;
use leadstinger::brand::{self, APP_NAME, CLI_NAME, SLUG};
use leadstinger::{app, audio, control, demo, starter, store, theme};
use eframe::egui;

/// `--help`, with the environment variables under this brand's prefix.
fn help() -> String {
    let e = brand::env_name;
    format!(
        "\
{APP_NAME}: a live soundboard for macOS and Linux.

USAGE: {SLUG} [--help] [--version]
Companion command-line tool: {CLI_NAME} (every command in the app, scriptable).

Environment:
  {:<24} library location (default: the per-user data directory)
  {:<24} fill an empty library with synthesized demo sounds
  {:<24} a starter pack to import on first launch (see starter/README.md)
  {:<24} don't open an audio device (playback is simulated)
  {:<24} save a PNG screenshot to this path, then quit
  {:<24} initial window size in logical pixels, e.g. 940x800
  {:<24} startup UI steps, e.g. \"play:0,play:4,select:2,edit\"
  {:<24} draw the menu bar inside the window on macOS too
  {:<24} Linux: open the window through X11/XWayland (file drops
  {:<24} work) or native Wayland; overrides the Settings choice
(Older prefixes still work: {}.)",
        e("DATA_DIR"),
        e("DEMO=1"),
        e("STARTER_DIR"),
        e("SILENT=1"),
        e("SNAPSHOT"),
        e("WINDOW_SIZE"),
        e("SCRIPT"),
        e("IN_WINDOW_MENU=1"),
        e("X11=1|0"),
        "",
        brand::ENV_PREFIXES[1..].iter().map(|p| format!("{p}_*")).collect::<Vec<_>>().join(", "),
    )
}

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => println!("{} {}", brand::APP_NAME, env!("CARGO_PKG_VERSION")),
            "--help" | "-h" => println!("{}", help()),
            other => {
                eprintln!("{SLUG}: unknown argument {other:?}\n\n{}", help());
                std::process::exit(2);
            }
        }
        return;
    }

    let data_dir = brand::data_dir();
    if control::app_running(&data_dir) {
        eprintln!("{SLUG}: {APP_NAME} is already running with this library ({}).", data_dir.display());
        eprintln!("{SLUG}: use `{CLI_NAME}` to control it, or set {} for a separate library.", brand::env_name("DATA_DIR"));
        std::process::exit(0);
    }
    // Held for the app's life; the CLI takes it only while the app is closed.
    let _library_lock = match control::lock_library(&data_dir, Duration::from_secs(5)) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("{SLUG}: can't open the library in {}: {e}", data_dir.display());
            std::process::exit(1);
        }
    };
    let mut store = match store::Store::open(data_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{SLUG}: can't open the library: {e:#}");
            std::process::exit(1);
        }
    };
    match starter::preload_if_first_launch(&mut store) {
        Ok(Some(n)) => eprintln!("{SLUG}: loaded the starter pack ({n} sounds)"),
        Ok(None) => {}
        Err(e) => eprintln!("{SLUG}: couldn't load the starter pack: {e:#}"),
    }
    if brand::env("DEMO").is_some() {
        if let Err(e) = demo::populate_if_empty(&mut store) {
            eprintln!("{SLUG}: couldn't build the demo library: {e:#}");
        }
    }
    let engine = if brand::env("SILENT").is_some() {
        // A screenshot shows the app as people see it, without this notice.
        let notice = if brand::env("SNAPSHOT").is_some() { String::new() } else { format!("Silent mode ({})", brand::env_name("SILENT")) };
        audio::Engine::disabled(&notice)
    } else {
        audio::Engine::new(store.library.output_device.as_deref())
    };

    let x11 = use_x11(store.library.use_x11);
    #[allow(unused_mut)] // only Linux adjusts it
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app::APP_NAME)
            .with_app_id(brand::SLUG)
            .with_inner_size(window_size())
            .with_min_inner_size(MIN_SIZE)
            .with_drag_and_drop(true)
            .with_icon(theme::app_icon()),
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    if x11 && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        options.event_loop_builder = Some(Box::new(|b| {
            b.with_x11();
        }));
    }
    let mut opts = app::Options::from_env();
    opts.x11 = x11;
    if let Err(e) = eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::StingerApp::new(cc, store, engine, opts)))),
    ) {
        eprintln!("{SLUG}: {e}");
        std::process::exit(1);
    }
}

/// The smallest window the layout is designed for.
const MIN_SIZE: [f32; 2] = [720.0, 480.0];

/// The initial window size: `<PREFIX>_WINDOW_SIZE=WxH` (for screenshots at tiled widths),
/// else 1280×800. Never below `MIN_SIZE`.
fn window_size() -> [f32; 2] {
    let parsed = brand::env_string("WINDOW_SIZE").and_then(|v| {
        let (w, h) = v.split_once(['x', 'X'])?;
        Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?])
    });
    if brand::env("WINDOW_SIZE").is_some() && parsed.is_none() {
        eprintln!("{SLUG}: {} should look like 940x800; using 1280x800", brand::env_name("WINDOW_SIZE"));
    }
    let [w, h] = parsed.unwrap_or([1280.0, 800.0]);
    [w.max(MIN_SIZE[0]), h.max(MIN_SIZE[1])]
}

/// Whether the window should go through X11. Only Linux has a choice: a Wayland session
/// with XWayland (`DISPLAY` set) can use either, and a plain X11 session is always X11.
fn use_x11(setting: bool) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let has_x = std::env::var_os("DISPLAY").is_some();
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return has_x;
    }
    let wanted = match brand::env_string("X11").as_deref() {
        Some("1" | "true" | "yes") => true,
        Some("0" | "false" | "no") => false,
        _ => setting,
    };
    if wanted && !has_x {
        eprintln!("{SLUG}: X11 was asked for, but DISPLAY isn't set (no XWayland); using Wayland");
    }
    wanted && has_x
}
