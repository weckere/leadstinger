//! The accessibility check (M20, D-083): every screen of the real window, in both themes,
//! rendered headlessly with egui_kittest, and its AccessKit tree walked. Every control a
//! person can act on must have a name a screen reader can read, and hover text given
//! through [`theme::hint`] (or its siblings), which also says why when it's disabled.
//!
//! `KNOWN_GAPS` lists what still falls short. It may only shrink: a new gap fails the
//! test, and so does a listed gap that has been fixed (take it off the list).

use super::*;
use crate::model::PressOptions;
use eframe::egui::accesskit::{self, Action as AxAction, Role, Toggled};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use std::collections::{BTreeSet, HashSet};

/// The gaps left, as [`gaps`] words them.
const KNOWN_GAPS: &[&str] = &[];

/// A library like `LEADSTINGER_DEMO=1`'s (its first 12 and 6 tiles), plus an action tile and a 5.1 tile in "Game Night".
fn library() -> (Store, PathBuf) {
    let dir = std::env::temp_dir().join(format!("{}-a11y-{}", crate::brand::SLUG, Uuid::new_v4()));
    let mut store = Store::open(dir.clone()).expect("open a scratch library");
    crate::demo::populate_for_tests(&mut store).expect("demo library");
    (store, dir)
}

/// How long [`Board::until`] waits for background work (imports, decoding, a mock server)
/// before failing. Generous: under a parallel `cargo test` the threads can be slow to run.
const DEADLINE: Duration = Duration::from_secs(60);

/// The window, then its scratch library (dropped in that order, so the window saves first).
struct Board {
    harness: Harness<'static, StingerApp>,
    dir: Scratch,
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

impl Board {
    fn new(theme: ThemeKind) -> Board {
        Board::stepping(theme, 0.25)
    }

    /// A board whose frames are `dt` seconds apart (kittest's default is 0.25 s, longer
    /// than a double-click).
    fn stepping(theme: ThemeKind, dt: f32) -> Board {
        let (store, dir) = library();
        let opts = Options { in_window_menu: true, headless: true, ..Default::default() };
        let harness = Harness::builder()
            .with_step_dt(dt)
            .with_size(vec2(1280.0, 800.0))
            .with_accessibility_check(false)
            .build_eframe(move |cc| StingerApp::new(cc, store, Engine::disabled(""), opts));
        let mut b = Board { harness, dir: Scratch(dir) };
        b.run("view.theme", json!({"theme": theme}));
        // A fade cue for the first tile, and a six-channel file.
        b.run("tile.addAction", json!({"kind": "fade", "targets": ["#1"], "set": "Game Night"}));
        let surround = b.dir.0.join("surround.wav");
        let samples: Vec<f32> = (0..44_100 * 6).map(|i| ((i / 6) as f32 * 0.05).sin() * 0.2).collect();
        crate::wav::write_multichannel(&surround, 44_100, 0x3F, &samples).expect("write a 5.1 file");
        b.run("tile.add", json!({"paths": [surround], "set": "Game Night"}));
        b.settle();
        b.run("view.select", json!({}));
        b
    }

    fn run(&mut self, id: &str, params: Value) {
        let lender = self.harness.state().lender.clone();
        lender.with(|c| c.execute(id, params)).unwrap_or_else(|e| panic!("{id}: {e}"));
        self.harness.run_steps(3);
    }

    /// Step the window until `done`, giving background threads time between frames, and
    /// fail after [`DEADLINE`] naming what never happened. Wait on the condition, never on
    /// a number of frames: how many frames background work takes depends on the load.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Board) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !done(self) {
            assert!(Instant::now() < deadline, "gave up after {DEADLINE:?} waiting for {what}");
            self.harness.run_steps(1);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Step until imports and decoding are done (the tiles show their lengths).
    fn settle(&mut self) {
        let lender = self.harness.state().lender.clone();
        self.until("imports and decoding to finish", |_| lender.with(|c| !c.store.is_loading() && c.store.importing().next().is_none()));
        self.harness.run_steps(3);
    }

    /// Click the first control with this name (the menu bar comes first in the tree).
    fn click(&mut self, label: &str) {
        self.harness.get_all_by_label(label).next().unwrap_or_else(|| panic!("no {label:?}")).click();
        self.harness.run_steps(3);
    }

    fn hover(&mut self, label: &str) {
        self.harness.get_all_by_label(label).next().unwrap_or_else(|| panic!("no {label:?}")).hover();
        self.harness.run_steps(3);
    }

    fn press_all(&mut self, key: Key, m: Modifiers) {
        self.harness.key_press_modifiers(m, key);
        self.harness.run_steps(2);
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.harness.run_steps(3);
    }
}

/// Can a person act on this node? egui's input roles, plus custom-drawn areas that take
/// clicks but have no role yet (`ui.interact` without `widget_info`).
fn interactive(node: &egui_kittest::Node<'_>) -> bool {
    let ax = node.accesskit_node();
    if ax.is_hidden() || ax.data().is_read_only() {
        return false;
    }
    let role = ax.role();
    egui::accessibility::is_input(role) || (role == Role::Unknown && ax.data().supports_action(AxAction::Click))
}

/// A choice inside an open drop-down list: its label is the whole message, and the
/// drop-down itself has the hover text. (A list box of our own has a name, and its
/// options are checked like any control: the QuipClipper window's clips.)
fn in_dropdown(node: &egui_kittest::Node<'_>) -> bool {
    std::iter::successors(node.parent(), |n| n.parent()).any(|n| n.accesskit_node().role() == Role::ListBox && n.accesskit_node().label().is_none())
}

/// egui's own window chrome: named by egui, with no hook for hover text. (Window title
/// bars, which egui leaves unnamed, are skipped altogether.) A resizable window's edges
/// and corners are "Resize window right edge" and so on.
const EGUI_CHROME: &[&str] = &["Close window", "Resize panel"];

fn egui_chrome(label: &str) -> bool {
    EGUI_CHROME.contains(&label) || label.starts_with("Resize window ")
}

/// What falls short on the screen showing now, worded for `KNOWN_GAPS`.
fn gaps(board: &Board, screen: &str) -> Vec<String> {
    let log = theme::hint_log(&board.harness.ctx);
    let hinted: HashSet<u64> = log.0.keys().map(|id| id.value()).collect();
    let says_why: HashSet<u64> = log.0.iter().filter(|(_, w)| **w).map(|(id, _)| id.value()).collect();
    let mut out = Vec::new();
    for node in board.harness.root().children_recursive() {
        // AccessKit hands VoiceOver a Pane (each egui panel's role) as an unknown element,
        // and VoiceOver's cursor never goes into one: name the panel (theme::name_panel).
        if node.accesskit_node().role() == Role::Pane {
            out.push("VoiceOver can't enter: a panel with no role of its own".to_owned());
        }
        if !interactive(&node) {
            continue;
        }
        let ax = node.accesskit_node();
        let id = ax.locate().0 .0;
        let siblings: Vec<_> = node.parent().map(|p| p.accesskit_node().children().collect()).unwrap_or_default();
        // A window's title bar: egui's, beside its close button.
        if ax.role() == Role::Unknown && siblings.iter().any(|s| s.label().as_deref() == Some("Close window")) {
            continue;
        }
        // A set window drawn inside the harness's one window (embedded): egui's title
        // bar along its top, which the real, separate window doesn't have.
        let window = std::iter::successors(node.parent(), |n| n.parent()).find(|n| n.accesskit_node().label().is_some_and(|l| !l.trim().is_empty()));
        let title_bar = window.is_some_and(|w| {
            let w = w.accesskit_node();
            let (me, it) = (ax.bounding_box().map_or(f64::MAX, |r| r.y0), w.bounding_box().map_or(f64::MAX, |r| r.y0));
            w.label().is_some_and(|l| l.ends_with(&format!(" · {APP_NAME}"))) && (me - it).abs() < 2.0
        });
        if ax.role() == Role::Unknown && title_bar {
            continue;
        }
        let label = ax.label().filter(|l| !l.trim().is_empty());
        let what = match &label {
            Some(l) => format!("{:?} {l:?}", ax.role()),
            None => {
                let inside = std::iter::successors(node.parent(), |n| n.parent())
                    .find_map(|n| n.accesskit_node().label().filter(|l| !l.trim().is_empty()))
                    .unwrap_or_default();
                format!("{:?} inside {inside:?}", ax.role())
            }
        };
        let mut problems = Vec::new();
        if label.is_none() && !ax.data().placeholder().is_some_and(|p| !p.trim().is_empty()) {
            problems.push("no name");
        }
        if ax.role() == Role::Unknown {
            problems.push("no role");
        }
        // egui names an unnamed widget by its hover text, so a screen reader reads a
        // sentence where a short name belongs: give it one (theme::name).
        if label.is_some() && label == ax.description() && !in_dropdown(&node) {
            problems.push("named by its hover text");
        }
        // A name a screen reader reads as a symbol ("🗙", "▶ Preview") or a unit ("s").
        let symbol = |l: &str| !l.starts_with(|c: char| c.is_alphanumeric() || c == '“') || (l.chars().count() < 2 && !l.starts_with(|c: char| c.is_ascii_digit()));
        if label.as_deref().is_some_and(symbol) && !in_dropdown(&node) {
            problems.push("named by a symbol");
        }
        // VoiceOver calls a slider without an orientation a "circular slider".
        if ax.role() == Role::Slider && ax.orientation().is_none() {
            problems.push("no orientation");
        }
        // A slider's value box (its next sibling) shares the slider's hint.
        let part_of = (ax.role() == Role::SpinButton)
            .then(|| siblings.iter().find(|s| s.role() == Role::Slider && hinted.contains(&s.locate().0 .0)).map(|s| s.locate().0 .0))
            .flatten();
        let id = part_of.unwrap_or(id);
        if !in_dropdown(&node) && !label.as_deref().is_some_and(egui_chrome) {
            if !hinted.contains(&id) {
                problems.push("no hover text");
            } else if ax.is_disabled() && !says_why.contains(&id) {
                problems.push("disabled without a reason");
            }
        }
        if !problems.is_empty() {
            let gap = format!("{}: {what}", problems.join(", "));
            if std::env::var_os("A11Y_VERBOSE").is_some() {
                eprintln!("[{screen}] {gap} at {:?}", ax.raw_bounds());
            }
            out.push(gap);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every screen in one theme: each gap found, once.
fn walk(theme: ThemeKind) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut b = Board::new(theme);
    let mut look = |b: &Board, screen: &str| found.extend(gaps(b, screen));
    look(&b, "grid");
    b.run("tile.trigger", json!({"tile": "#1"}));
    look(&b, "grid, playing");
    b.run("transport.stopAll", json!({"immediate": true}));
    b.run("view.search", json!({"query": "a"}));
    look(&b, "search");
    b.run("view.search", json!({"close": true}));
    // An installed theme, so View ▸ Theme lists one (D-119).
    let harbor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
    b.run("theme.import", json!({"path": harbor, "use": false}));
    for menu in ["File", "Edit", "View", "Playback", "Help"] {
        b.click(menu);
        if menu == "View" {
            for sub in ["Columns", "Theme", "Appearance"] {
                b.hover(sub);
                look(&b, &format!("menu View ▸ {sub}"));
            }
        }
        look(&b, &format!("menu {menu}"));
        b.press(Key::Escape);
    }
    b.run("view.settings", json!({"open": true}));
    look(&b, "settings");
    b.run("view.settings", json!({"open": false}));
    b.run("view.about", json!({}));
    look(&b, "about");
    b.run("view.shortcuts", json!({}));
    look(&b, "shortcuts");
    b.close_windows();
    b.run("view.select", json!({"tile": "#1"}));
    b.run("tile.render", json!({}));
    assert!(b.harness.state().export.is_some(), "tile.render opens the Export Audio window");
    b.harness.get_by_label("WAV");
    look(&b, "export audio");
    b.harness.state_mut().export = None;
    let set_file = b.dir.0.join("Opened.lsset");
    b.run("set.export", json!({"path": set_file}));
    b.run("set.import", json!({"path": set_file, "ask": true}));
    b.harness.get_by_label("Import");
    look(&b, "import a set file?");
    b.harness.state_mut().confirm_sets.clear();
    b.run("view.select", json!({}));
    b.close_windows();
    b.open_quip();
    look(&b, "quipclipper");
    b.run("view.quip", json!({"open": false}));
    b.quip_new();
    look(&b, "quipclipper badge");
    // The inspector (D-144): the set's settings with nothing selected, then a tile on
    // each tab, several tiles, an action tile and a 5.1 tile.
    b.run("view.select", json!({}));
    look(&b, "set settings");
    b.run("view.select", json!({"tile": "#1"}));
    for tab in ["play", "sound", "cue", "control"] {
        b.run("view.inspector", json!({"tab": tab}));
        look(&b, &format!("one tile, {tab} tab"));
    }
    b.run("view.select", json!({"tile": "#2", "add": true}));
    look(&b, "two tiles");
    b.run("view.inspector", json!({"tab": "sound"}));
    look(&b, "two tiles, sound tab");
    b.run("view.select", json!({"tile": "#13"}));
    look(&b, "action tile");
    b.run("view.select", json!({"tile": "#14"}));
    look(&b, "5.1 tile");
    // A locked set (M22): its tile, its settings, and the menus.
    b.run("set.lock", json!({}));
    b.run("view.select", json!({"tile": "#1"}));
    look(&b, "locked set, a tile");
    b.run("view.select", json!({}));
    look(&b, "locked set, its settings");
    b.click("File");
    look(&b, "locked set, menu File");
    b.press(Key::Escape);
    b.run("set.unlock", json!({}));
    b.run("view.sidebar", json!({"open": false}));
    b.run("view.inspector", json!({"open": false}));
    look(&b, "sidebar and inspector hidden");
    b.run("view.sidebar", json!({"open": true}));
    b.run("view.inspector", json!({"tab": "play"}));
    b.run("view.select", json!({"tile": "#1"}));
    b.run("tile.set", json!({"tile": "#1", "vampStart": 0.2, "vampEnd": 0.4}));
    b.run("tile.markerAdd", json!({"tile": "#1", "at": 0.1, "name": "Hit"}));
    b.run("view.editor", json!({"tile": "#1"}));
    b.click("Waveform view");
    look(&b, "editor");
    b.run("view.editorSelect", json!({"from": 0.2, "to": 0.3}));
    look(&b, "editor, selection");
    b.run("view.editor", json!({"open": false}));
    b.run("set.layout", json!({"layout": "list"}));
    look(&b, "list, a tile");
    b.run("view.select", json!({}));
    look(&b, "list");
    b.run("set.select", json!({"set": "#2"}));
    look(&b, "list, cue set");
    // A set in a window of its own (D-222): a grid set beside the cue list, and
    // the View menu. (Embedded in the harness's one window, two cue lists would share
    // their controls' ids; real windows each have their own.)
    b.run("set.layout", json!({"set": "#1", "layout": "grid"}));
    b.run("window.open", json!({"set": "#1"}));
    look(&b, "set window");
    b.click("View");
    look(&b, "menu View, a set window open");
    b.press(Key::Escape);
    b.run("window.close", json!({"all": true}));
    found
}

impl Board {
    /// The QuipClipper window on a mock server, once its clips are listed.
    fn open_quip(&mut self) {
        let server = crate::quip::clips::tests::mock(Default::default());
        self.run("quip.connect", json!({"url": server, "token": "tok", "check": false}));
        self.run("view.quip", json!({"open": true}));
        self.until("the window to list the mock's clips", |b| b.harness.query_all_by_label_contains("Don't let yourself get attached").next().is_some());
        self.harness.run_steps(3);
    }

    /// One clip new on the mock server again (its seen cursor moved back), once the
    /// status bar's badge shows it.
    fn quip_new(&mut self) {
        let lender = self.harness.state().lender.clone();
        lender.with(|c| {
            let server = crate::quip::list(&c.store.data_dir).unwrap()[0].0.clone();
            crate::quip::set_seen(&c.store.data_dir, &server, "v1.100").unwrap();
            c.quip_watch.ask_now();
        });
        for _ in 0..500 {
            if self.harness.query_by_label("1 new in QuipClipper").is_some() {
                break;
            }
            self.harness.run_steps(1);
            std::thread::sleep(Duration::from_millis(10));
        }
        self.harness.run_steps(3);
    }

    fn close_windows(&mut self) {
        let lender = self.harness.state().lender.clone();
        lender.with(|c| {
            c.show_about = false;
            c.show_shortcuts = false;
            c.show_settings = false;
        });
        self.harness.run_steps(3);
    }
}

/// Both themes, against `KNOWN_GAPS`.
#[test]
fn every_control_is_named_and_hinted() {
    let mut found = walk(ThemeKind::Console);
    found.extend(walk(ThemeKind::Panel));
    let known: BTreeSet<String> = KNOWN_GAPS.iter().map(|s| s.to_string()).collect();
    let new = found.difference(&known).count();
    let fixed = known.difference(&found).count();
    let all = found.iter().map(|g| format!("    {g:?},")).collect::<Vec<_>>().join("\n");
    assert!(new == 0 && fixed == 0, "{new} new gap(s), {fixed} fixed but still listed. KNOWN_GAPS should now be:\n{all}");
}

/// A tile's hover text says what it is and how it plays.
#[test]
fn a_tile_describes_itself() {
    let b = Board::new(ThemeKind::Console);
    let root = b.harness.root();
    let horn = root
        .children_recursive()
        .filter_map(|n| n.accesskit_node().description())
        .find(|d| d.starts_with("Air Horn\n"))
        .expect("the Air Horn tile has hover text");
    for part in ["hold · key Q · 0:02.0", "Plays on Main", "Click to select it, double-click to play it"] {
        assert!(horn.contains(part), "{part:?} missing from {horn:?}");
    }
}

/// The labels a screen reader reads for custom-drawn controls, live.
#[test]
fn custom_controls_read_their_state() {
    let mut b = Board::new(ThemeKind::Console);
    let labels = |b: &Board| -> Vec<(Role, String)> {
        b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().label().map(|l| (n.accesskit_node().role(), l))).collect()
    };
    let has = |b: &Board, role: Role, want: &str| {
        let all = labels(b);
        assert!(all.iter().any(|(r, l)| *r == role && l.starts_with(want)), "no {role:?} starting {want:?} among {all:?}");
    };
    has(&b, Role::Button, "Air Horn, tile 5, row 2, key Q, hold, 2.0 seconds");
    has(&b, Role::Slider, "Master volume");
    has(&b, Role::Button, "Game Night, set 1 of 2, 14 tiles");
    has(&b, Role::Button, "Crickets, tile 6, row 2, key W, loops, 8.0 seconds");
    for (role, panel) in [(Role::Toolbar, "Toolbar"), (Role::Region, "Setlist"), (Role::Region, "Board"), (Role::Region, "Status bar")] {
        has(&b, role, panel);
    }
    // The transport counter is text (its words are the value, as egui's labels are).
    let counter = |b: &Board, want: &str| {
        let texts: Vec<String> = b.harness.root().children_recursive().filter(|n| n.accesskit_node().role() == Role::Label).filter_map(|n| n.accesskit_node().value()).collect();
        assert!(texts.iter().any(|t| t.starts_with(want)), "no counter reading {want:?} among {texts:?}");
    };
    counter(&b, "Nothing playing");
    b.run("tile.trigger", json!({"tile": "#2"}));
    has(&b, Role::Button, "Applause, tile 2, row 1, key 2, playing, ");
    counter(&b, "Applause, playing, ");
    // The tile's meter is in its label, while meters are shown (D-110).
    let applause = |b: &Board| labels(b).into_iter().find(|(r, l)| *r == Role::Button && l.starts_with("Applause, tile 2")).expect("Applause").1;
    assert!(applause(&b).contains(" left, level "), "{}", applause(&b));
    b.run("view.meters", json!({"on": false}));
    assert!(!applause(&b).contains("level"), "{}", applause(&b));
    b.run("view.meters", json!({"on": true}));
    b.run("transport.stopAll", json!({"immediate": true}));
    b.run("view.select", json!({"tile": "#1"}));
    for handle in ["Trim start", "Trim end"] {
        has(&b, Role::Slider, handle);
    }
    has(&b, Role::Tab, "Play");
    b.run("set.select", json!({"set": "#2"}));
    has(&b, Role::Button, "Cue 1, Preshow Music, ");
    // A cue list has no GO button: its bar has the mode, the cross-fade and Reset List (D-199).
    for (role, name) in [(Role::RadioButton, "Continuous"), (Role::RadioButton, "Queued"), (Role::Slider, "Cross-fade"), (Role::Button, "Reset List"), (Role::CheckBox, "Cue 1 armed")] {
        has(&b, role, name);
    }
    assert!(!labels(&b).iter().any(|(_, l)| l.starts_with("GO")));
    // The bottom bar: Setlist and Inspector by name, Grid and List as icons with names (D-216).
    for name in ["Setlist", "Inspector", "Grid", "Cue list", "Add Sound"] {
        has(&b, Role::Button, name);
    }
    b.click("View");
    has(&b, Role::MenuItemCheckBox, "Inspector");
    has(&b, Role::MenuItemCheckBox, "Setlist");
    b.press(Key::Escape);
    // A locked set says so, in the sidebar.
    b.run("set.lock", json!({}));
    has(&b, Role::Button, "Theatre Cues, set 2 of 2, 6 tiles, locked");
}

impl Board {
    fn focused_title(&self) -> Option<String> {
        let id = self.harness.state().focused_tile?;
        let lender = self.harness.state().lender.clone();
        lender.with(|c| c.store.library.tile(id).map(|t| t.title.clone()))
    }

    /// Tab until a tile or cue row has focus.
    fn tab_to_board(&mut self) {
        for _ in 0..200 {
            if self.focused_title().is_some() {
                return;
            }
            self.press(Key::Tab);
        }
        panic!("Tab never reached the board");
    }

    fn playing(&self, title: &str) -> bool {
        let lender = self.harness.state().lender.clone();
        lender.with(|c| c.statuses.keys().any(|id| c.store.library.tile(*id).is_some_and(|t| t.title == title)))
    }

    /// Put a key down and leave it down. [`Harness::key_press`] sends the release in the
    /// same frame, which lets go of a Hold tile at once: it then fades out on the
    /// wall clock, and under load is gone before anyone looks.
    fn key_down(&mut self, key: Key, pressed: bool) {
        self.harness.event(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        self.harness.run_steps(3);
    }
}

/// The board works without a mouse: Tab reaches the tiles, arrows move between them and
/// the focus selects (M22), Enter plays one, ⌘/Ctrl+Down moves one to the next row (not
/// in a locked set), and tile keys still don't fire while typing.
#[test]
fn the_board_works_from_the_keyboard() {
    let mut b = Board::new(ThemeKind::Console);
    b.tab_to_board();
    assert_eq!(b.focused_title().as_deref(), Some("Intro Theme"));
    b.press(Key::ArrowRight);
    assert_eq!(b.focused_title().as_deref(), Some("Applause"));
    let lender = b.harness.state().lender.clone();
    let selected = |l: &Arc<Lender>| l.with(|c| c.selected_tile.and_then(|id| c.store.library.tile(id)).map(|t| t.title.clone()));
    assert_eq!(selected(&lender).as_deref(), Some("Applause"), "the focus selects");
    b.press(Key::Enter);
    b.until("Enter to play the focused tile", |b| b.playing("Applause"));
    b.run("transport.stopAll", json!({"immediate": true}));

    // A locked set's tiles stay put.
    b.run("set.lock", json!({}));
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowDown);
    b.harness.run_steps(3);
    assert_eq!(lender.with(|c| c.current_set().row_lengths()), vec![4, 4, 4, 2], "nothing moves in a locked set");
    b.run("set.unlock", json!({}));
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowDown);
    b.harness.run_steps(3);
    let (rows, moved) = lender.with(|c| (c.current_set().row_lengths(), c.current_set().tiles[4].title.clone()));
    assert_eq!((rows, moved.as_str()), (vec![3, 5, 4, 2], "Applause"), "⌘/Ctrl+Down moves the focused tile to the next row, same place");
    assert_eq!(b.focused_title().as_deref(), Some("Applause"), "focus stays on the moved tile");
    b.run("view.select", json!({}));

    // Typing in the search box doesn't fire tile keys (Air Horn is key Q: row 2, D-188).
    b.run("view.search", json!({}));
    assert!(b.harness.ctx.egui_wants_keyboard_input(), "the search box has focus");
    b.harness.event(egui::Event::Text("q".into()));
    b.press(Key::Q);
    assert!(!b.playing("Air Horn"), "a tile key fired while typing");
    b.run("view.search", json!({"close": true}));
    b.key_down(Key::Q, true);
    b.until("tile keys to work again once the field is closed", |b| b.playing("Air Horn"));
    b.key_down(Key::Q, false);
}

/// A clicked pad takes the keyboard focus, and tile keys still play (retest A16): only a
/// text field stops them. Rows 5 to 8 take their keys with Option (Alt) held, read where
/// the key is (macOS types ¡ for ⌥1), and letting go stops a Hold pad even when Option
/// was let go first (D-188).
#[test]
fn tile_keys_play_with_a_pad_selected_and_with_option() {
    let mut b = Board::stepping(ThemeKind::Console, 0.02);
    let lender = b.harness.state().lender.clone();
    let rect = |b: &Board, title: &str| {
        let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.id)).unwrap();
        b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2
    };
    b.run("tile.set", json!({"tile": "Intro Theme", "title": "Overture"}));
    let p = rect(&b, "Applause").center();
    for pressed in [true, false] {
        b.harness.event(egui::Event::PointerMoved(p));
        b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        b.harness.run_steps(1);
    }
    b.harness.run_steps(10);
    assert_eq!(b.focused_title().as_deref(), Some("Applause"), "the click focuses the pad");
    assert!(b.harness.ctx.egui_wants_keyboard_input() && !b.harness.ctx.text_edit_focused());
    // The window's own keys too: ⌘/Ctrl+Z undoes.
    b.press_all(Key::Z, Modifiers::COMMAND);
    b.until("⌘/Ctrl+Z to undo with Applause selected", |b| b.harness.state().lender.with(|c| c.current_set().tiles[0].title == "Intro Theme"));
    // Air Horn (a Hold pad, row 2, slot 1) is Q.
    b.key_down(Key::Q, true);
    b.until("Q to play Air Horn with Applause selected", |b| b.playing("Air Horn"));
    b.key_down(Key::Q, false);
    let state = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().find(|t| t.title == "Air Horn").map(|t| c.engine.state(t.id))).unwrap();
    assert_ne!(state(&lender), PlayState::Playing, "let go: it stops");
    b.run("transport.stopAll", json!({"immediate": true}));

    // One pad a row: Air Horn, the fifth, is row 5's first key, ⌥1 / Alt+1.
    b.run("set.rows", json!({"perRow": 1}));
    let (title, key) = lender.with(|c| (c.current_set().tiles[4].title.clone(), c.current_set().tiles[4].hotkey));
    assert_eq!((title.as_str(), key.map(PadKey::name).as_deref()), ("Air Horn", Some("alt+1")));
    let alt = Modifiers::ALT;
    // On macOS the key is the character typed (¡ isn't a key egui names); the physical key is 1.
    let typed = if cfg!(target_os = "macos") { Key::Exclamationmark } else { Key::Num1 };
    b.harness.event(egui::Event::Key { key: typed, physical_key: Some(Key::Num1), pressed: true, repeat: false, modifiers: alt });
    b.harness.run_steps(3);
    b.until("⌥1 to play Air Horn", |b| b.playing("Air Horn"));
    assert!(!b.playing("Intro Theme"), "⌥1 isn't 1");
    // Option let go first, then 1.
    b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    b.key_down(Key::Num1, false);
    assert_ne!(state(&lender), PlayState::Playing, "let go of 1 after Option: it stops");
    // A screen reader hears it.
    let shown = if cfg!(target_os = "macos") { "Option 1" } else { "Alt 1" };
    b.harness.get_by_label_contains(&format!("Air Horn, tile 5, row 5, key {shown}"));
}

/// A set in a window of its own (D-222, D-223): it shows that set's board with its own
/// bar while the main window shows another set; a click in it selects the pad, which the
/// main window's inspector shows; its Grid key changes that set; the setlist says it's
/// open; closing it closes it. (The harness has one window, so egui draws the set window
/// inside it.)
#[test]
fn a_set_window_shows_its_set_and_drives_the_inspector() {
    let mut b = Board::new(ThemeKind::Console);
    let lender = b.harness.state().lender.clone();
    assert!(b.harness.query_by_label_contains("Preshow Music").is_none(), "Theatre Cues isn't shown yet");
    b.run("window.open", json!({"set": "Theatre Cues"}));
    b.harness.get_by_label("Set window bar");
    assert_eq!(b.harness.query_all_by_label("Stop All").count(), 2, "the set window has its own Stop All");
    b.harness.get_all_by_label_contains("Preshow Music").next().expect("its cue list is drawn").click();
    b.harness.run_steps(3);
    let (current, selected) = lender.with(|c| (c.current_set().name.clone(), c.selected_tile.and_then(|id| c.store.library.tile(id)).map(|t| t.title.clone())));
    assert_eq!(current, "Game Night", "the main window keeps its set");
    assert_eq!(selected.as_deref(), Some("Preshow Music"), "a click in the set window selects its pad");
    assert!(b.harness.query_by_label_contains("Theatre Cues, set 2 of 2, 6 tiles, in its own window").is_some(), "the setlist says it's open in a window");
    // Its Grid key switches that set, not the main window's.
    b.harness.get_all_by_label("Grid").last().expect("the set window's Grid key").click();
    b.harness.run_steps(3);
    lender.with(|c| {
        let layout = |name: &str| c.store.library.sets.iter().find(|s| s.name == name).map(|s| s.layout).unwrap();
        assert_eq!(layout("Theatre Cues"), Layout::Grid);
        assert_eq!(layout("Game Night"), Layout::Grid);
        assert_eq!(c.current_set().name, "Game Night");
    });
    b.run("window.close", json!({"set": "Theatre Cues"}));
    assert!(b.harness.query_by_label("Set window bar").is_none(), "closed");
}

/// No modes (M22, D-147): a click selects a pad and doesn't play it; a double-click
/// plays it; on a Hold pad the second press plays it while it's held, and letting go
/// stops it. ⌘-click adds to the selection without playing.
#[test]
fn a_click_selects_and_a_double_click_plays() {
    let mut b = Board::stepping(ThemeKind::Console, 0.02);
    let lender = b.harness.state().lender.clone();
    let at = |b: &Board, title: &str| {
        let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.id)).unwrap();
        b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2.center()
    };
    let button = |b: &mut Board, p: Pos2, pressed: bool, modifiers: Modifiers| {
        b.harness.event(egui::Event::ModifiersChanged(modifiers));
        b.harness.event(egui::Event::PointerMoved(p));
        b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers });
        b.harness.run_steps(1);
    };
    let selected = |l: &Arc<Lender>| l.with(|c| c.selected().iter().filter_map(|id| c.store.library.tile(*id)).map(|t| t.title.clone()).collect::<Vec<_>>());

    let p = at(&b, "Applause");
    button(&mut b, p, true, Modifiers::NONE);
    button(&mut b, p, false, Modifiers::NONE);
    b.harness.run_steps(10); // past the double-click time
    assert_eq!(selected(&lender), ["Applause"], "a click selects");
    assert!(!b.playing("Applause"), "a click doesn't play");
    // ⌘-click adds Crickets, and plays nothing.
    let q = at(&b, "Crickets");
    button(&mut b, q, true, Modifiers::COMMAND);
    button(&mut b, q, false, Modifiers::COMMAND);
    b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    b.harness.run_steps(8);
    assert_eq!(selected(&lender), ["Applause", "Crickets"]);
    assert!(!b.playing("Crickets"));
    // A double-click plays.
    button(&mut b, p, true, Modifiers::NONE);
    button(&mut b, p, false, Modifiers::NONE);
    button(&mut b, p, true, Modifiers::NONE);
    button(&mut b, p, false, Modifiers::NONE);
    b.until("a double-click to play Applause", |b| b.playing("Applause"));
    assert_eq!(selected(&lender), ["Applause"]);
    b.run("transport.stopAll", json!({"immediate": true}));

    // Air Horn is a Hold pad: the second press plays while it's down.
    let h = at(&b, "Air Horn");
    button(&mut b, h, true, Modifiers::NONE);
    button(&mut b, h, false, Modifiers::NONE);
    button(&mut b, h, true, Modifiers::NONE);
    b.harness.run_steps(2);
    let state = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().find(|t| t.title == "Air Horn").map(|t| c.engine.state(t.id))).unwrap();
    assert_eq!(state(&lender), PlayState::Playing, "held: it plays");
    button(&mut b, h, false, Modifiers::NONE);
    b.harness.run_steps(2);
    assert_ne!(state(&lender), PlayState::Playing, "let go: it stops");
}

/// Shift-click selects the rectangle of slots from the selected pad (D-169), ⌘-Shift adds
/// one, and a screen reader hears each pad in it as selected; nothing plays.
#[test]
fn shift_click_selects_a_rectangle_of_pads() {
    let mut b = Board::stepping(ThemeKind::Console, 0.02);
    let lender = b.harness.state().lender.clone();
    b.run("set.rows", json!({"perRow": 3}));
    b.settle();
    // The title in each slot, row by row.
    let grid: Vec<Vec<String>> = lender.with(|c| {
        let s = c.current_set();
        s.slots().iter().map(|r| r.iter().map(|x| s.tiles[x.unwrap()].title.clone()).collect()).collect()
    });
    assert!(grid.len() >= 3 && grid[2].len() == 3, "the demo set fills three rows of three: {grid:?}");
    let click = |b: &mut Board, title: &str, modifiers: Modifiers| {
        let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.id)).unwrap();
        let p = b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2.center();
        b.harness.event(egui::Event::ModifiersChanged(modifiers));
        b.harness.event(egui::Event::PointerMoved(p));
        for pressed in [true, false] {
            b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers });
            b.harness.run_steps(1);
        }
        b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        b.harness.run_steps(10);
    };
    // What a screen reader hears as selected (a pad reads as on), in board order.
    let heard = |b: &Board| -> Vec<String> {
        grid.iter()
            .flatten()
            .filter(|t| {
                let label = format!("{t}, tile ");
                let on = b.harness.query_all_by_label_contains(&label).any(|n| n.accesskit_node().role() == Role::Button && n.accesskit_node().toggled() == Some(Toggled::True));
                on
            })
            .cloned()
            .collect()
    };
    let g = |r: usize, s: usize| grid[r][s].clone();

    click(&mut b, &g(0, 1), Modifiers::NONE);
    click(&mut b, &g(2, 2), Modifiers::SHIFT);
    let rect = vec![g(0, 1), g(0, 2), g(1, 1), g(1, 2), g(2, 1), g(2, 2)];
    assert_eq!(heard(&b), rect, "rows 1-3 x slots 2-3, not the run from {} to {}", g(0, 1), g(2, 2));
    assert!(grid.iter().flatten().all(|t| !b.playing(t)), "selecting plays nothing");
    // Shift-click again: a new rectangle from the same pad.
    click(&mut b, &g(1, 0), Modifiers::SHIFT);
    assert_eq!(heard(&b), vec![g(0, 0), g(0, 1), g(1, 0), g(1, 1)]);
    // ⌘-click a pad, ⌘-Shift-click: its rectangle joins the selection.
    click(&mut b, &g(2, 0), Modifiers::COMMAND);
    click(&mut b, &g(2, 1), Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(heard(&b), vec![g(0, 0), g(0, 1), g(1, 0), g(1, 1), g(2, 0), g(2, 1)]);
    assert!(grid.iter().flatten().all(|t| !b.playing(t)));
}

/// The inspector's tabs (D-144) from the keyboard and for a screen reader: each is a
/// named tab that reads selected or not, Tab reaches it and Space shows it.
#[test]
fn the_inspector_tabs_work_from_the_keyboard() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        b.run("view.select", json!({"tile": "Rimshot"}));
        let tab = |b: &Board, name: &str| {
            let n = b.harness.get_all_by_label(name).find(|n| n.accesskit_node().role() == Role::Tab).unwrap_or_else(|| panic!("{theme:?}: no {name:?} tab"));
            n.accesskit_node().is_selected()
        };
        assert_eq!((tab(&b, "Play"), tab(&b, "Sound"), tab(&b, "Cue"), tab(&b, "Control")), (Some(true), Some(false), Some(false), Some(false)), "{theme:?}");
        let mut reached = false;
        for _ in 0..200 {
            b.press(Key::Tab);
            let focused = b.harness.ctx.memory(|m| m.focused());
            if focused == Some(egui::Id::unique(("inspector-tab", "cue"))) {
                reached = true;
                break;
            }
        }
        assert!(reached, "{theme:?}: Tab reaches the Cue tab");
        b.press(Key::Space);
        assert_eq!(lender.with(|c| c.inspector_tab), crate::controller::InspectorTab::Cue, "{theme:?}");
        assert_eq!(tab(&b, "Cue"), Some(true));
        assert!(b.harness.query_all_by_label("Pre-wait").next().is_some(), "{theme:?}: the Cue tab shows");
        b.run("view.inspector", json!({"tab": "control"}));
        assert!(b.harness.query_by_label_contains("Hotkey, ").is_some(), "{theme:?}: the Control tab shows");
    }
}

/// A vamp you can let go (D-153, D-157): the pad and the inspector say it's vamping and
/// that the next press plays on, then that it's playing on; the Cue tab's Play On button
/// is live only while it vamps. A double-click is the press, in both looks.
#[test]
fn a_vamping_pad_says_so_and_the_next_press_plays_on() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::stepping(theme, 0.02);
        let lender = b.harness.state().lender.clone();
        b.run("tile.set", json!({"tile": "Applause", "vampStart": 0.1, "vampEnd": 0.3}));
        b.run("view.select", json!({"tile": "Applause"}));
        b.run("view.inspector", json!({"tab": "cue"}));
        let (id, p) = {
            let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == "Applause").map(|t| t.id)).unwrap();
            (id, b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2.center())
        };
        let double_click = |b: &mut Board| {
            for pressed in [true, false, true, false] {
                b.harness.event(egui::Event::PointerMoved(p));
                b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
                b.harness.run_steps(1);
            }
            b.harness.run_steps(10); // past the double-click time
        };
        let play_on_live = |b: &Board| !b.harness.get_by_label("Play On").accesskit_node().is_disabled();
        assert!(!play_on_live(&b), "{theme:?}: Play On waits for the vamp");
        assert!(b.harness.query_by_label("The next press while it vamps plays on").is_some(), "{theme:?}");

        double_click(&mut b);
        let l = lender.clone();
        b.until("Applause to vamp", move |_| l.with(|c| c.engine.vamping(id)));
        b.harness.run_steps(3);
        assert!(b.harness.query_by_label_contains("vamping, the next press plays on").is_some(), "{theme:?}: the pad says it vamps");
        assert!(b.harness.query_by_label("Vamping: the next press plays on").is_some(), "{theme:?}: the inspector says so");
        assert!(play_on_live(&b), "{theme:?}: Play On is live");
        assert_eq!(gaps(&b, "vamping"), Vec::<String>::new());

        // The next press lets it go: it plays on, and says so.
        double_click(&mut b);
        b.harness.run_steps(3);
        assert!(lender.with(|c| !c.engine.vamping(id) && c.engine.state(id) == PlayState::Playing), "{theme:?}: played on");
        assert!(b.harness.query_by_label_contains("playing on, ").is_some(), "{theme:?}: the pad says it plays on");
        assert!(b.harness.query_by_label("Playing on to the end").is_some(), "{theme:?}");
        assert!(!play_on_live(&b), "{theme:?}: nothing left to let go");

        // The press after stops it.
        double_click(&mut b);
        assert_ne!(lender.with(|c| c.engine.state(id)), PlayState::Playing, "{theme:?}: stopped");
    }
}

/// A locked set still runs (D-184): its Cue tab greys the edits (the vamp, its times,
/// the notes, the pre-wait, removing a marker) but Play On and a marker's Jump stay live,
/// and Play On lets the vamp go. In both looks.
#[test]
fn a_locked_set_s_cue_tab_still_plays() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::stepping(theme, 0.02);
        let lender = b.harness.state().lender.clone();
        b.run("tile.set", json!({"tile": "Applause", "vampStart": 0.1, "vampEnd": 0.3}));
        b.run("tile.markerAdd", json!({"tile": "Applause", "at": 0.05, "name": "Hit"}));
        b.run("view.select", json!({"tile": "Applause"}));
        b.run("view.inspector", json!({"tab": "cue"}));
        b.run("set.lock", json!({}));
        let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == "Applause").map(|t| t.id)).unwrap();
        let disabled = |b: &Board, label: &str| b.harness.get_all_by_label(label).all(|n| n.accesskit_node().is_disabled());
        for edit in ["Vamp", "Vamp start", "Vamp end", "Notes", "Pre-wait", "Remove Hit", "Add marker at playhead"] {
            assert!(disabled(&b, edit), "{theme:?}: {edit} can't change in a locked set");
        }
        assert!(!disabled(&b, "Jump"), "{theme:?}: Jump still plays");

        b.run("tile.play", json!({"tile": "Applause"}));
        let l = lender.clone();
        b.until("Applause to vamp", move |_| l.with(|c| c.engine.vamping(id)));
        b.harness.run_steps(3);
        assert!(!disabled(&b, "Play On"), "{theme:?}: Play On is live in a locked set");
        // Below the fold at this size, so pressed as a screen reader would.
        b.harness.get_by_label("Play On").click_accesskit();
        b.harness.run_steps(3);
        assert!(lender.with(|c| !c.engine.vamping(id) && c.engine.state(id) == PlayState::Playing), "{theme:?}: played on");
        assert!(lender.with(|c| c.current_set().locked), "{theme:?}: still locked");
    }
}

/// The fade badge (D-168) and the Random button (D-171), in both themes: a pad with an
/// end fade-out says so in its label and hover text; Random is a named, hinted button
/// that plays a pad from the set (a locked one too) and says which, and it's greyed out,
/// saying why, in a set with nothing to play.
#[test]
fn the_fade_badge_and_random_read_out() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        b.run("tile.set", json!({"tile": "Air Horn", "fadeOut": 1.5, "stopFade": 0.5, "fadeIn": 0.2}));
        b.run("tile.set", json!({"tile": "Applause", "fadeIn": 1.0, "stopFade": 2.0}));
        let label = |b: &Board, title: &str| {
            b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().label()).find(|l| l.starts_with(&format!("{title}, tile "))).unwrap_or_else(|| panic!("no {title} pad"))
        };
        assert!(label(&b, "Air Horn").contains("hold, fades out"), "{theme:?}: {}", label(&b, "Air Horn"));
        assert!(!label(&b, "Applause").contains("fades out"), "{theme:?}: a fade-in or stop fade isn't an end fade-out");
        let tip = b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().description()).find(|d| d.starts_with("Air Horn\n")).expect("hover text");
        assert!(tip.contains("hold · fades out over its last 1.5s · key Q"), "{theme:?}: {tip}");

        b.run("set.lock", json!({}));
        let random = b.harness.get_by_label("Random");
        assert_eq!(random.accesskit_node().role(), Role::Button, "{theme:?}");
        assert_eq!(random.accesskit_node().description().as_deref(), Some(tips::RANDOM), "{theme:?}");
        assert!(!random.accesskit_node().is_disabled(), "{theme:?}: a locked set still plays");
        b.click("Random");
        let playing = lender.with(|c| c.statuses.keys().filter_map(|id| c.store.library.tile(*id)).map(|t| t.title.clone()).collect::<Vec<_>>());
        assert_eq!(playing.len(), 1, "{theme:?}: one pad played");
        assert_eq!(b.announced(), format!("Random: playing {}", playing[0]), "{theme:?}");
        b.run("transport.stopAll", json!({"immediate": true}));

        // A new, empty set has nothing to pick.
        b.run("set.create", json!({"name": "Empty"}));
        let random = b.harness.get_by_label("Random");
        assert!(random.accesskit_node().is_disabled(), "{theme:?}: nothing to play");
        assert!(random.accesskit_node().description().is_some_and(|d| d.contains(tips::RANDOM_WHY)), "{theme:?}");
    }
}

/// A locked set (M22) in the window: its pads don't drag, its settings and the tile's
/// are read-only (the inspector says so, with Unlock), Finder files don't land, and its
/// pads still play.
#[test]
fn a_locked_set_only_plays() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("set.rows", json!({"rows": [3, 8]}));
    b.run("set.lock", json!({}));
    let lender = b.harness.state().lender.clone();
    let titles = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
    let before = titles(&lender);
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let (from, onto) = (rect(&b, 0).center(), rect(&b, 5));
    b.harness.event(egui::Event::PointerMoved(from));
    b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    b.harness.run_steps(2);
    for k in 1..=6 {
        b.harness.event(egui::Event::PointerMoved(from + (pos2(onto.left() + 6.0, onto.center().y) - from) * (k as f32 / 6.0)));
        b.harness.run_steps(1);
    }
    b.harness.event(egui::Event::PointerButton { pos: onto.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    b.harness.run_steps(3);
    assert_eq!(titles(&lender), before, "a drag in a locked set moves nothing");
    // A sound from Finder doesn't land either; the status bar says why.
    let wav = b.dir.0.join("Extra.wav");
    crate::wav::write_tone(&wav, 8000, 0.2, &[440.0], 0.3).unwrap();
    finder_drop(&mut b, onto.center(), &[wav]);
    assert_eq!(titles(&lender).len(), before.len(), "no tile added");
    assert!(lender.with(|c| c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("locked"))), "the status bar says it's locked");
    // The inspector: read-only, with Unlock.
    b.run("view.select", json!({"tile": "#1"}));
    let title = b.harness.get_by_label("Title");
    assert!(title.accesskit_node().is_disabled(), "the title can't be changed");
    assert!(!b.harness.get_by_label("Preview").accesskit_node().is_disabled(), "Preview still plays");
    assert!(!b.harness.get_all_by_label("Sound").any(|n| n.accesskit_node().is_disabled()), "the tabs still switch");
    b.click("Unlock");
    assert!(!lender.with(|c| c.current_set().locked), "Unlock in the inspector unlocks the set");
}

/// Rows (D-116): Up and Down move the focus to the nearest tile of the next row, and in
/// Edit mode ⌘/Ctrl+arrows move a tile across rows and into new ones.
#[test]
fn rows_work_from_the_keyboard() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("set.rows", json!({"rows": [3, 8]}));
    let lender = b.harness.state().lender.clone();
    let titles = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
    let index = |b: &Board, l: &Arc<Lender>| titles(l).iter().position(|t| Some(t) == b.focused_title().as_ref());
    b.tab_to_board();
    b.press(Key::ArrowRight);
    b.press(Key::ArrowRight);
    assert_eq!(b.focused_title().as_deref(), Some("Rimshot"));
    let rimshot = b.harness.state().tile_rects.iter().find(|(_, i, _)| *i == 2).expect("drawn").2;
    b.press(Key::ArrowDown);
    let below = index(&b, &lender).expect("a tile has the focus");
    assert!((3..11).contains(&below), "Down lands in the second row: {:?}", b.focused_title());
    let r = b.harness.state().tile_rects.iter().find(|(_, i, _)| *i == below).expect("drawn").2;
    assert!(r.x_range().intersects(rimshot.x_range()), "on the nearest tile: {r:?} under {rimshot:?}");
    b.press(Key::ArrowUp);
    assert_eq!(b.focused_title().as_deref(), Some("Rimshot"), "Up goes back");

    let rows = |l: &Arc<Lender>| l.with(|c| c.current_set().row_lengths());
    // ⌘Up on the top row: a new row of its own above.
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowUp);
    b.harness.run_steps(3);
    assert_eq!((rows(&lender), titles(&lender)[0].as_str()), (vec![1, 2, 8, 3], "Rimshot"));
    assert_eq!(b.focused_title().as_deref(), Some("Rimshot"), "focus follows the tile");
    // ⌘Right from a row's end: the next row's start, and its own row goes.
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowRight);
    b.harness.run_steps(3);
    assert_eq!((rows(&lender), titles(&lender)[0..3].to_vec()), (vec![3, 8, 3], vec!["Rimshot".to_owned(), "Intro Theme".into(), "Applause".into()]));
    // ⌘Left from a row's start: the end of the row above (none here: nothing happens).
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowLeft);
    b.harness.run_steps(3);
    assert_eq!(titles(&lender)[0], "Rimshot");
    // ⌘Down: the second row, same place; then ⌘Left along it.
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowDown);
    b.harness.run_steps(3);
    assert_eq!((rows(&lender), titles(&lender)[2].as_str()), (vec![2, 9, 3], "Rimshot"));
}

/// D-170: a row never wraps. In a narrow board a row of eleven stays one row, every pad
/// of the set one width down to the minimum, and the board scrolls sideways to a pad
/// the keyboard moves to.
#[test]
fn rows_never_wrap_and_the_board_scrolls_sideways() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("view.sidebar", json!({"open": false}));
    b.run("view.inspector", json!({"open": false}));
    b.run("set.rows", json!({"rows": [1, 11]}));
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let rects = |b: &Board| (0..12).map(|i| rect(b, i)).collect::<Vec<_>>();
    // Wide: the row of eleven fits, so every pad is its share, all one width.
    let wide = rects(&b);
    assert!(wide[1..12].iter().all(|r| r.top() == wide[1].top()), "one row");
    assert!(wide.iter().all(|r| r.width() == wide[11].width() && r.width() > grid::PAD_MIN_WIDTH), "{wide:?}");
    assert!(wide[11].right() < 1280.0);
    // Narrow: still one row, at the minimum, running past the window's edge.
    b.harness.set_size(vec2(720.0, 800.0));
    b.harness.run_steps(4);
    let narrow = rects(&b);
    assert!(narrow[1..12].iter().all(|r| r.top() == narrow[1].top()), "still one row: {narrow:?}");
    assert!(narrow.iter().all(|r| r.width() == grid::PAD_MIN_WIDTH), "{narrow:?}");
    assert_eq!(narrow[0].left(), narrow[1].left(), "the rows' slots line up");
    assert!(narrow[11].right() > 720.0, "the row runs off the board");
    // Arrow along the row to its last pad: the board scrolls it into view.
    b.tab_to_board();
    b.press(Key::ArrowDown);
    for _ in 0..11 {
        b.press(Key::ArrowRight);
    }
    b.harness.run_steps(10);
    assert_eq!(b.focused_title().as_deref(), Some("Outro"), "the last pad of the row");
    let r = rect(&b, 11);
    assert!(r.right() <= 720.0 && r.left() >= 0.0, "scrolled into view: {r:?}");
    assert!(rect(&b, 0).left() < narrow[0].left(), "the board moved sideways");
}

/// Found in the M22 retest: a tile clicked with the mouse takes ⌘/Ctrl+arrows (before,
/// only a tile focused from the keyboard did, and plain arrows set the master volume).
#[test]
fn a_clicked_tile_moves_with_the_move_keys() {
    let mut b = Board::new(ThemeKind::Console);
    let lender = b.harness.state().lender.clone();
    let titles = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
    let before = titles(&lender);
    let at = b.harness.state().tile_rects.iter().find(|(_, i, _)| *i == 0).expect("drawn").2.center();
    b.harness.event(egui::Event::PointerMoved(at));
    b.harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    b.harness.run_steps(1);
    b.harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    b.harness.run_steps(3);
    let master = lender.with(|c| c.store.library.master_volume);
    b.harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowRight);
    b.harness.run_steps(3);
    assert_eq!(titles(&lender)[1], before[0], "⌘→ moved the clicked tile: {:?}", titles(&lender));
    b.press(Key::ArrowDown);
    assert_eq!(lender.with(|c| c.store.library.master_volume), master, "plain arrows move the focus, not the volume");
}

/// View ▸ Theme lists an imported theme as a named check item (D-119), checked while
/// it's in use; picking it uses it, and the built-ins are unchecked then.
#[test]
fn view_theme_lists_imported_themes() {
    let mut b = Board::new(ThemeKind::Console);
    let harbor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
    b.run("theme.import", json!({"path": harbor, "use": false}));
    let item = |b: &Board, name: &str| {
        let n = b.harness.get_all_by_label(name).find(|n| n.accesskit_node().role() == Role::MenuItemCheckBox).unwrap_or_else(|| panic!("no {name:?} item"));
        n.accesskit_node().toggled()
    };
    b.click("View");
    b.hover("Theme");
    assert_eq!((item(&b, "Console"), item(&b, "Harbor")), (Some(accesskit::Toggled::True), Some(accesskit::Toggled::False)));
    b.click("Harbor");
    let lender = b.harness.state().lender.clone();
    assert_eq!(lender.with(|c| c.look.id.clone()), "harbor");
    b.click("View");
    b.hover("Theme");
    assert_eq!((item(&b, "Console"), item(&b, "Harbor")), (Some(accesskit::Toggled::False), Some(accesskit::Toggled::True)));
    b.press(Key::Escape);
    // Removed, it leaves the menu.
    b.run("theme.remove", json!({"theme": "harbor"}));
    b.click("View");
    b.hover("Theme");
    assert!(b.harness.query_by_label("Harbor").is_none(), "Harbor is still listed");
}

/// The inspector's pad options (D-144, D-145): named toggle buttons that read on, off or
/// mixed; Pausable and One-shot turn each other off; a mixed option turns on for all.
#[test]
fn the_inspector_options_are_toggle_buttons() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        let toggle = |b: &Board, name: &str| {
            let n = b.harness.get_all_by_label(name).find(|n| n.accesskit_node().role() == Role::Button && n.accesskit_node().toggled().is_some());
            n.unwrap_or_else(|| panic!("{theme:?}: no {name:?} toggle")).accesskit_node().toggled()
        };
        let click = |b: &mut Board, name: &str| {
            b.harness.get_all_by_label(name).find(|n| n.accesskit_node().role() == Role::Button && n.accesskit_node().toggled().is_some()).expect("toggle").click();
            b.harness.run_steps(3);
        };
        let press = |title: &str| lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.press)).unwrap();
        b.run("view.select", json!({"tile": "Air Horn"}));
        assert_eq!((toggle(&b, "Hold"), toggle(&b, "Pausable"), toggle(&b, "Disabled")), (Some(Toggled::True), Some(Toggled::False), Some(Toggled::False)), "{theme:?}");
        click(&mut b, "Pausable");
        assert_eq!(press("Air Horn"), PressOptions { hold: true, pausable: true, one_shot: false }, "{theme:?}");
        click(&mut b, "One-shot");
        assert_eq!(press("Air Horn"), PressOptions { hold: true, pausable: false, one_shot: true }, "{theme:?}: One-shot turns Pausable off");
        assert_eq!(toggle(&b, "Pausable"), Some(Toggled::False));
        // Two tiles, one held: Hold reads mixed, and a click holds both.
        b.run("view.select", json!({"tile": "Crickets", "add": true}));
        assert_eq!(toggle(&b, "Hold"), Some(Toggled::Mixed), "{theme:?}");
        click(&mut b, "Hold");
        assert!(press("Air Horn").hold && press("Crickets").hold, "{theme:?}");
        assert_eq!(toggle(&b, "Hold"), Some(Toggled::True));
    }
}

/// Mute (D-251): the speaker beside Volume is a toggle that keeps the level, a mixed
/// selection reads mixed, and the pad says it's muted to a screen reader and in its
/// hover text (its badge is the same speaker, drawn).
#[test]
fn mute_sits_beside_the_volume_and_the_pad_says_so() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        let toggle = |b: &Board| {
            let n = b.harness.get_all_by_label("Mute").find(|n| n.accesskit_node().role() == Role::Button && n.accesskit_node().toggled().is_some());
            n.unwrap_or_else(|| panic!("{theme:?}: no Mute toggle")).accesskit_node().toggled()
        };
        let click = |b: &mut Board| {
            b.harness.get_all_by_label("Mute").find(|n| n.accesskit_node().role() == Role::Button && n.accesskit_node().toggled().is_some()).expect("Mute").click();
            b.harness.run_steps(3);
        };
        let tile = |title: &str| lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| (t.muted, t.volume))).unwrap();
        let label = |b: &Board, title: &str| {
            b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().label()).find(|l| l.starts_with(&format!("{title}, tile "))).unwrap_or_else(|| panic!("no {title} pad"))
        };
        b.run("view.select", json!({"tile": "Air Horn"}));
        let volume = tile("Air Horn").1;
        assert_eq!(toggle(&b), Some(Toggled::False), "{theme:?}");
        assert!(!label(&b, "Air Horn").contains("muted"));
        click(&mut b);
        assert_eq!(tile("Air Horn"), (true, volume), "{theme:?}: muted at its volume");
        assert_eq!(toggle(&b), Some(Toggled::True), "{theme:?}");
        assert!(label(&b, "Air Horn").contains("hold"), "{theme:?}: {}", label(&b, "Air Horn"));
        assert!(label(&b, "Air Horn").contains("muted"), "{theme:?}: {}", label(&b, "Air Horn"));
        let tip = b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().description()).find(|d| d.starts_with("Air Horn\n")).expect("hover text");
        assert!(tip.contains("muted · hold"), "{theme:?}: {tip}");
        b.run("edit.undo", json!({}));
        assert!(!tile("Air Horn").0, "{theme:?}: one undo step");
        b.run("tile.set", json!({"tile": "Air Horn", "muted": true}));
        // Two tiles, one muted: Mute reads mixed, and a click mutes both.
        b.run("view.select", json!({"tile": "Crickets", "add": true}));
        assert_eq!(toggle(&b), Some(Toggled::Mixed), "{theme:?}");
        click(&mut b);
        assert!(tile("Air Horn").0 && tile("Crickets").0, "{theme:?}");
        click(&mut b);
        assert!(!tile("Air Horn").0 && !tile("Crickets").0, "{theme:?}: and unmutes both");
    }
}

/// A tile's emoji (D-253): an Emoji field beside the title with hover text that keeps
/// only emoji, one undo step; the pad draws it before its title when there is room, and
/// leaves it out of the pad's spoken name (a screen reader would read "dog face" first).
#[test]
fn the_emoji_field_keeps_emoji_and_the_pad_draws_them() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        let emoji = || lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == "Air Horn").map(|t| t.emoji.clone())).unwrap();
        b.run("view.select", json!({"tile": "Air Horn"}));
        let field = b.harness.get_by_label("Emoji");
        assert_eq!(field.accesskit_node().description().as_deref(), Some(tips::EMOJI_TIP), "{theme:?}");
        field.focus();
        b.harness.run_steps(2);
        b.harness.get_by_label("Emoji").type_text("📯 horn");
        b.harness.run_steps(3);
        assert_eq!(emoji(), "📯", "{theme:?}: only the emoji stay");
        let pad = b.harness.root().children_recursive().filter_map(|n| n.accesskit_node().label()).find(|l| l.starts_with("Air Horn, tile ")).expect("the pad");
        assert!(!pad.contains('📯'), "{theme:?}: not in the spoken name: {pad}");
        b.press(Key::Escape);
        b.run("edit.undo", json!({}));
        assert_eq!(emoji(), "", "{theme:?}: one undo step");
    }
    let all = |_| true;
    let tile = Tile { title: "Horn".into(), emoji: "📯".into(), ..Default::default() };
    assert_eq!((pad_title_with(&tile, 120.0, all), pad_title_with(&tile, 40.0, all)), ("📯 Horn".to_owned(), "Horn".to_owned()), "a narrow pad leaves it off");
    assert_eq!(pad_title_with(&Tile { title: "Horn".into(), ..Default::default() }, 120.0, all), "Horn");
    // An emoji the face lacks is left off (a box would draw), the joiner kept.
    let vest = Tile { title: "Dog".into(), emoji: "🐕\u{200D}\u{1F9BA}".into(), ..Default::default() };
    assert_eq!(pad_title_with(&vest, 120.0, |c| c != '\u{1F9BA}'), "🐕\u{200D} Dog");
    assert_eq!(pad_title_with(&Tile { emoji: "\u{1F9BA}".into(), ..vest.clone() }, 120.0, |c| c != '\u{1F9BA}'), "Dog", "none it can draw: none");
}

/// The inspector keeps the width it opens at (D-211): nothing in it, on any screen, is
/// wider than the column. egui lets a side panel's content widen it, and the panel keeps
/// that width, so a single row too wide (a long output name, a fixed-width menu) would
/// leave it wider for good.
#[test]
fn everything_in_the_inspector_fits_its_width() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        let check = |b: &Board, screen: &str| {
            let w = b.harness.state().inspector_width;
            assert!((w - INSPECTOR_WIDTH).abs() < 2.0, "{theme:?}, {screen}: the inspector is {w} wide, not {INSPECTOR_WIDTH}");
        };
        check(&b, "set settings");
        b.run("output.add", json!({"name": "Headphones Cue Mix"}));
        b.run("view.select", json!({"tile": "#1"}));
        b.run("tile.set", json!({"tile": "#1", "speed": 1.25, "pitch": 2.0, "hotkey": "alt+q", "sends": "Main,Headphones Cue Mix"}));
        b.run("tile.set", json!({"tile": "#1", "vampStart": 0.2, "vampEnd": 0.4}));
        b.run("tile.markerAdd", json!({"tile": "#1", "at": 0.1, "name": "Hit"}));
        for tab in ["play", "sound", "cue", "control"] {
            b.run("view.inspector", json!({"tab": tab}));
            check(&b, &format!("one tile, {tab} tab"));
        }
        b.run("view.select", json!({"tile": "#2", "add": true}));
        for tab in ["play", "sound"] {
            b.run("view.inspector", json!({"tab": tab}));
            check(&b, &format!("two tiles, {tab} tab"));
        }
        b.run("view.select", json!({"tile": "#13"}));
        check(&b, "a fade action tile");
        b.run("tile.set", json!({"tile": "#2", "title": "Applause, with a title far too long for the column"}));
        b.run("tile.addAction", json!({"kind": "play", "targets": ["#2", "#3@1.5"], "set": "Game Night"}));
        check(&b, "a group with a long target");
        b.run("view.select", json!({"tile": "#14"}));
        b.run("view.inspector", json!({"tab": "sound"}));
        b.run("tile.set", json!({"tile": "#14", "channels": "left,right,both:-3,mute,left:-3,right:-3"}));
        check(&b, "a 5.1 tile's channels");
        b.run("view.select", json!({"tile": "#3"}));
        b.run("view.editor", json!({"tile": "#3"}));
        check(&b, "the editor open");
        b.run("view.editor", json!({"open": false}));
        lender.with(|c| c.stage_recording(2.0));
        b.harness.run_steps(3);
        check(&b, "recording");
    }
}

/// Settings > Grid layout (D-114): three named radio buttons that switch the mode.
#[test]
fn settings_choose_the_grid_layout() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("view.settings", json!({}));
    let radio = |b: &Board, name: &str| {
        let n = b.harness.get_all_by_label(name).find(|n| n.accesskit_node().role() == Role::RadioButton).unwrap_or_else(|| panic!("no {name:?} radio"));
        n.accesskit_node().toggled()
    };
    assert_eq!(radio(&b, "Rows"), Some(accesskit::Toggled::True), "Rows is the default");
    // Below the fold: from the keyboard, as a screen-reader user would.
    b.harness.get_all_by_label("Auto columns").find(|n| n.accesskit_node().role() == Role::RadioButton).expect("radio").focus();
    b.harness.run_steps(2);
    b.press(Key::Space);
    let lender = b.harness.state().lender.clone();
    assert_eq!(lender.with(|c| c.store.library.grid), crate::model::GridMode::Auto);
    assert_eq!(radio(&b, "Auto columns"), Some(accesskit::Toggled::True));
}

/// Rows (D-116): a dragged tile lands where the marker says: between two tiles of a row,
/// or as a new row in the gap between rows.
#[test]
fn tiles_drag_into_rows_and_between_them() {
    let mut b = Board::new(ThemeKind::Panel);
    b.run("set.rows", json!({"rows": [3, 8]}));
    let lender = b.harness.state().lender.clone();
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let drag = |b: &mut Board, from: Pos2, to: Pos2| {
        b.harness.event(egui::Event::PointerMoved(from));
        b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        b.harness.run_steps(2);
        for k in 1..=6 {
            b.harness.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
            b.harness.run_steps(1);
        }
        b.harness.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        b.harness.run_steps(3);
    };
    let titles = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| t.title.clone()).collect::<Vec<_>>());
    // Intro Theme onto the left half of the second row's third tile: before it.
    let (from, onto) = (rect(&b, 0).center(), rect(&b, 5));
    drag(&mut b, from, pos2(onto.left() + 6.0, onto.center().y));
    let rows = lender.with(|c| c.current_set().row_lengths());
    assert_eq!((rows, titles(&lender)[4].as_str()), (vec![2, 9, 3], "Intro Theme"), "{:?}", titles(&lender));
    // Applause into the gap between the rows: a new row of its own.
    let (from, top, bottom) = (rect(&b, 0).center(), rect(&b, 0).bottom(), rect(&b, 2).top());
    drag(&mut b, from, pos2(from.x, (top + bottom) / 2.0));
    let rows = lender.with(|c| c.current_set().row_lengths());
    assert_eq!((rows, titles(&lender)[1].as_str()), (vec![1, 1, 9, 3], "Applause"), "{:?}", titles(&lender));
}

/// M22 (D-141), with real pointer drags in Console: while a pad is held over the board it
/// shows the drop: the pads after a gap slide along, a blank slot fills with nothing else
/// moving, and past the last row a phantom row holds the pad's ghost. Letting go does
/// exactly that.
#[test]
fn pads_drag_into_gaps_blanks_and_phantom_rows() {
    let mut b = Board::new(ThemeKind::Console);
    // A narrow window, so the row of eight stays at the smallest pads while one is lifted
    // out of it (D-217) and its pads keep their places.
    b.harness.set_size(vec2(720.0, 800.0));
    b.run("set.rows", json!({"rows": [3, 8]}));
    b.run("set.addBlank", json!({"row": 1, "slot": 2}));
    let lender = b.harness.state().lender.clone();
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let slot = |b: &Board, row: usize, at: usize| b.harness.state().row_grid.as_ref().unwrap().0.cells.iter().find(|c| (c.row, c.at) == (row, at)).unwrap().rect;
    let hold = |b: &mut Board, from: Pos2, to: Pos2| {
        b.harness.event(egui::Event::PointerMoved(from));
        b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        b.harness.run_steps(2);
        for k in 1..=6 {
            b.harness.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
            b.harness.run_steps(1);
        }
        for _ in 0..30 {
            b.harness.run_steps(1);
        }
    };
    let release = |b: &mut Board, at: Pos2| {
        b.harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        b.harness.run_steps(3);
        for _ in 0..30 {
            b.harness.run_steps(1);
        }
    };
    let grid = |l: &Arc<Lender>| {
        l.with(|c| {
            let s = c.current_set();
            s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".to_owned(), |i| s.tiles[i].title.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
        })
    };
    let start = grid(&lender);
    assert_eq!((start[0].len(), start[0][1].as_str()), (4, "_"), "{start:?}");

    // The first pad of row 2 held over the blank: it fills, nothing in row 1 moves.
    let (r2, r3) = (rect(&b, 2), rect(&b, 3));
    let blank = slot(&b, 0, 1).center();
    hold(&mut b, r3.center(), blank);
    assert_eq!(rect(&b, 2), r2, "nothing slides for a blank slot");
    release(&mut b, blank);
    let g = grid(&lender);
    assert_eq!(&g[0][..2], &[start[0][0].clone(), start[1][0].clone()], "{g:?}");
    assert_eq!(g[0].len(), 4, "the blank filled");

    // The last pad held over the second pad's left half: the gap opens, the second pad
    // and those after it slide right; let go, it's there.
    // (Row 2's second pad: the row is wider than the board, so its last is off screen.)
    let (r1, last) = (rect(&b, 1), rect(&b, 5));
    let gap = pos2(r1.left() + 6.0, r1.center().y);
    hold(&mut b, last.center(), gap);
    assert!(rect(&b, 1).left() > r1.left() + r1.width() / 2.0, "the second pad slides along: {r1:?} → {:?}", rect(&b, 1));
    release(&mut b, gap);
    let moved = grid(&lender);
    assert_eq!(moved[0][1], g[1][1], "{moved:?}");
    assert_eq!(moved[0].len(), 5, "a full row grows by one");

    // The last row's bottom edge (the rows fill the board): a phantom row with the ghost
    // in it, then a row of its own.
    let first = rect(&b, 0);
    let n = moved.len();
    let below = pos2(300.0, b.harness.state().row_grid.as_ref().unwrap().0.bands[n - 1].bottom - 4.0);
    hold(&mut b, first.center(), below);
    let ghost = egui::Id::unique(("ghost", n, 0usize));
    assert!(b.harness.state().slot_anim.contains_key(&ghost), "a ghost in a new (phantom) last row");
    release(&mut b, below);
    let g = grid(&lender);
    assert_eq!((g.len(), g[n].clone()), (n + 1, vec![moved[0][0].clone()]), "{g:?}");
}

/// Retest of 0.3.0 (D-190): a row's own last pad dragged right into the empty slot after
/// it goes there, leaving a blank where it was (before, only a pad from another row could
/// be dropped there).
#[test]
fn a_rows_last_pad_drags_into_the_empty_slot_after_it() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("set.rows", json!({"rows": [3, 3]}));
    let lender = b.harness.state().lender.clone();
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let grid = |l: &Arc<Lender>| {
        l.with(|c| {
            let s = c.current_set();
            s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".to_owned(), |i| s.tiles[i].title.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
        })
    };
    let start = grid(&lender);
    let (third, pitch) = (rect(&b, 2), rect(&b, 1).left() - rect(&b, 0).left());
    let to = third.center() + vec2(pitch, 0.0);
    b.harness.event(egui::Event::PointerMoved(third.center()));
    b.harness.event(egui::Event::PointerButton { pos: third.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    b.harness.run_steps(2);
    for k in 1..=6 {
        b.harness.event(egui::Event::PointerMoved(third.center() + (to - third.center()) * (k as f32 / 6.0)));
        b.harness.run_steps(1);
    }
    b.harness.run_steps(30);
    b.harness.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    b.harness.run_steps(30);
    let g = grid(&lender);
    assert_eq!(g[0], vec![start[0][0].clone(), start[0][1].clone(), "_".to_owned(), start[0][2].clone()], "{g:?}");
    assert_eq!(g[1], start[1], "the next row is untouched");
}

/// Retest on ed25128 (D-202): with every row as wide as the board there's no empty slot
/// drawn after a row's last pad. A pad of that row dropped on the last pad's right half
/// takes a new slot after it and leaves a blank, instead of swapping with it; the last
/// pad itself, dragged past its own middle, moves one slot right.
#[test]
fn a_pad_drags_to_its_full_rows_end_and_leaves_a_blank() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("set.rows", json!({"perRow": 3}));
    let lender = b.harness.state().lender.clone();
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let grid = |l: &Arc<Lender>| {
        l.with(|c| {
            let s = c.current_set();
            s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".to_owned(), |i| s.tiles[i].title.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
        })
    };
    let drag = |b: &mut Board, from: Pos2, to: Pos2| {
        b.harness.event(egui::Event::PointerMoved(from));
        b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        b.harness.run_steps(2);
        for k in 1..=6 {
            b.harness.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
            b.harness.run_steps(1);
        }
        b.harness.run_steps(30);
        b.harness.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        b.harness.run_steps(30);
    };
    let s = grid(&lender);
    let (a, bb, c) = (s[0][0].clone(), s[0][1].clone(), s[0][2].clone());
    let third = rect(&b, 2);
    let board_right = b.harness.state().tile_rects.iter().map(|(_, _, r)| r.right()).fold(0.0, f32::max);
    assert!(third.right() >= board_right - 1.0, "the row fills the board");
    // The middle pad onto the last one's right half: [A _ C B], not [A C B].
    let middle = rect(&b, 1).center();
    drag(&mut b, middle, pos2(third.right() - 10.0, third.center().y));
    let g = grid(&lender);
    assert_eq!(g[0], vec![a.clone(), "_".to_owned(), c.clone(), bb.clone()], "{g:?}");
    assert_eq!(g[1], s[1], "the next row is untouched");
    // The row's last pad (B, now third in reading order) past its own middle: one slot right.
    let last = rect(&b, 2);
    drag(&mut b, last.center(), pos2(last.right() - 10.0, last.center().y));
    let g = grid(&lender);
    assert_eq!(g[0], vec![a, "_".to_owned(), c, "_".to_owned(), bb], "{g:?}");
}

/// t-m22-drag-selection (D-219): a pad dragged while it's part of the selection takes the
/// selection with it, as one run in set order, with a ghost for each while it's held;
/// an unselected pad moves alone and becomes the selection; and the selection dropped
/// on another set in the sidebar moves to that set's end.
#[test]
fn a_selection_drags_together() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        b.run("set.rows", json!({"rows": [3, 3]}));
        let lender = b.harness.state().lender.clone();
        let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
        let titles = |l: &Arc<Lender>| {
            l.with(|c| {
                let s = c.current_set();
                s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".to_owned(), |i| s.tiles[i].title.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
            })
        };
        let id = |l: &Arc<Lender>, i: usize| l.with(|c| c.current_set().tiles[i].id);
        let ghosts = |b: &Board| b.harness.state().slot_anim.keys().filter(|k| (0..2).any(|r| (0..8).any(|a| **k == egui::Id::unique(("ghost", r, a))))).count();
        let press = |b: &mut Board, from: Pos2, to: Pos2| {
            b.harness.event(egui::Event::PointerMoved(from));
            b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            b.harness.run_steps(2);
            for k in 1..=6 {
                b.harness.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
                b.harness.run_steps(1);
            }
            b.harness.run_steps(30);
        };
        let release = |b: &mut Board, to: Pos2| {
            b.harness.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            b.harness.run_steps(30);
        };
        let s = titles(&lender);
        // The first and third pads selected; the first dragged to the second row's start.
        b.run("view.select", json!({"tiles": [id(&lender, 0), id(&lender, 2)]}));
        let to = rect(&b, 3).left_center() + vec2(8.0, 0.0);
        let from = rect(&b, 0).center();
        press(&mut b, from, to);
        b.harness.state_mut().slot_anim.clear();
        b.harness.run_steps(1);
        assert_eq!(ghosts(&b), 2, "{theme:?}: a ghost for each pad dragged");
        release(&mut b, to);
        let g = titles(&lender);
        let row1: Vec<String> = [s[0][0].clone(), s[0][2].clone()].into_iter().chain(s[1].iter().cloned()).collect();
        assert_eq!(g[..2], [vec![s[0][1].clone()], row1.clone()], "{theme:?}");
        assert!(lender.with(|c| c.selection.len()) == 2, "{theme:?}: they stay selected");
        // An unselected pad (alone in its row) to the next row's end: it moves alone,
        // and it's selected now.
        let lone = id(&lender, 0);
        let last = rect(&b, row1.len());
        let to = last.right_center() - vec2(8.0, 0.0);
        let from = rect(&b, 0).center();
        press(&mut b, from, to);
        release(&mut b, to);
        let g = titles(&lender);
        assert_eq!(g[0], row1.iter().cloned().chain([s[0][1].clone()]).collect::<Vec<_>>(), "{theme:?}");
        let first = id(&lender, 0);
        assert!(lender.with(|c| c.is_selected(lone) && !c.is_selected(first)), "{theme:?}: the dragged pad is the selection");
        // The first two, selected, dropped on the second set in the sidebar: its end.
        // (Rows of three first: the long row scrolled the board sideways.)
        b.run("set.rows", json!({"perRow": 3}));
        b.harness.run_steps(10);
        let (a, c) = (id(&lender, 0), id(&lender, 1));
        b.run("view.select", json!({"tiles": [a, c]}));
        let before = lender.with(|c| (c.store.library.sets[1].tiles.len(), c.current_set().tiles.len()));
        let set = b.harness.get_by_label_contains(", set 2 of ").rect().center();
        let from = rect(&b, 0).center();
        press(&mut b, from, set);
        release(&mut b, set);
        let after = lender.with(|l| {
            let t = &l.store.library.sets[1].tiles;
            (t.len(), l.current_set().tiles.len(), t[t.len() - 2].id == a && t[t.len() - 1].id == c)
        });
        assert_eq!(after, (before.0 + 2, before.1 - 2, true), "{theme:?}: both moved to the other set's end, in order");
    }
}

/// t-m22-alt-drag-copy (D-229): Alt-click takes a selected pad out of the selection and
/// leaves an unselected one alone, playing nothing; Alt-drag drops copies (with a ghost
/// for each and the originals still on the board) into a gap or onto another set, as one
/// undo step; letting go of Alt before the drop moves instead.
#[test]
fn alt_click_subtracts_and_alt_drag_copies() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        b.run("set.rows", json!({"rows": [3, 3]}));
        let lender = b.harness.state().lender.clone();
        let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
        let titles = |l: &Arc<Lender>| {
            l.with(|c| {
                let s = c.current_set();
                s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".to_owned(), |i| s.tiles[i].title.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
            })
        };
        let ids = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| t.id).collect::<Vec<_>>());
        let ghosts = |b: &Board| b.harness.state().slot_anim.keys().filter(|k| (0..3).any(|r| (0..8).any(|a| **k == egui::Id::unique(("ghost", r, a))))).count();
        let modifiers = |b: &mut Board, m: Modifiers| {
            b.harness.event(egui::Event::ModifiersChanged(m));
            b.harness.run_steps(2);
        };
        let press = |b: &mut Board, from: Pos2, to: Pos2, m: Modifiers| {
            b.harness.event(egui::Event::PointerMoved(from));
            b.harness.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: m });
            b.harness.run_steps(2);
            for k in 1..=6 {
                b.harness.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
                b.harness.run_steps(1);
            }
            b.harness.run_steps(30);
        };
        let release = |b: &mut Board, to: Pos2, m: Modifiers| {
            b.harness.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: m });
            b.harness.run_steps(30);
            b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
            b.harness.run_steps(2);
        };
        let click = |b: &mut Board, i: usize, m: Modifiers| {
            let p = rect(b, i).center();
            b.harness.event(egui::Event::ModifiersChanged(m));
            b.harness.event(egui::Event::PointerMoved(p));
            for pressed in [true, false] {
                b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: m });
                b.harness.run_steps(1);
            }
            b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
            b.harness.run_steps(10);
        };
        let selected = |l: &Arc<Lender>| l.with(|c| c.selected());
        let s = titles(&lender);
        let all = ids(&lender);

        // Alt-click: the middle pad of three goes out; an unselected pad changes nothing.
        b.run("view.select", json!({"tiles": [all[0], all[1], all[2]]}));
        click(&mut b, 1, Modifiers::ALT);
        assert_eq!(selected(&lender), vec![all[0], all[2]], "{theme:?}: Alt-click took it out");
        click(&mut b, 4, Modifiers::ALT);
        assert_eq!(selected(&lender), vec![all[0], all[2]], "{theme:?}: an unselected pad leaves the selection");
        assert!(s.iter().flatten().all(|t| !b.playing(t)), "{theme:?}: Alt-click plays nothing");
        // ⌘-click still toggles.
        click(&mut b, 1, Modifiers::COMMAND);
        assert_eq!(selected(&lender), vec![all[0], all[2], all[1]], "{theme:?}");
        b.run("view.select", json!({"tiles": [all[0], all[2]]}));

        // Alt-drag the first and third to the second row's start: copies go in there.
        let to = rect(&b, 3).left_center() + vec2(8.0, 0.0);
        let from = rect(&b, 0).center();
        modifiers(&mut b, Modifiers::ALT);
        press(&mut b, from, to, Modifiers::ALT);
        b.harness.state_mut().slot_anim.clear();
        b.harness.run_steps(1);
        assert_eq!(ghosts(&b), 2, "{theme:?}: a ghost for each copy");
        assert_eq!(b.harness.state().tile_rects.len(), all.len(), "{theme:?}: the originals are still drawn");
        release(&mut b, to, Modifiers::ALT);
        let g = titles(&lender);
        let row1: Vec<String> = [s[0][0].clone(), s[0][2].clone()].into_iter().chain(s[1].iter().cloned()).collect();
        assert_eq!(g[..2], [s[0].clone(), row1], "{theme:?}: the originals stay, the copies lead row 2");
        let now = ids(&lender);
        assert_eq!(now.len(), all.len() + 2, "{theme:?}");
        assert!(now[..3] == all[..3] && !all.contains(&now[3]) && !all.contains(&now[4]), "{theme:?}: new tiles, originals untouched");
        assert_eq!(selected(&lender), vec![all[0], all[2]], "{theme:?}: the originals stay selected");
        // One undo step takes both copies away.
        b.run("edit.undo", json!({}));
        b.harness.run_steps(5);
        assert_eq!((titles(&lender), ids(&lender)), (s.clone(), all.clone()), "{theme:?}: one undo");

        // Alt let go before the drop: it's a move after all.
        let to = rect(&b, 3).left_center() + vec2(8.0, 0.0);
        let from = rect(&b, 0).center();
        modifiers(&mut b, Modifiers::ALT);
        press(&mut b, from, to, Modifiers::ALT);
        modifiers(&mut b, Modifiers::NONE);
        release(&mut b, to, Modifiers::NONE);
        assert_eq!(ids(&lender).len(), all.len(), "{theme:?}: moved, not copied");
        assert_eq!(titles(&lender)[0], vec![s[0][1].clone()], "{theme:?}");
        b.run("edit.undo", json!({}));
        b.harness.run_steps(5);

        // Alt-dropped on the second set in the sidebar: copies at its end, this set as it was.
        b.run("set.rows", json!({"perRow": 3}));
        b.harness.run_steps(10);
        b.run("view.select", json!({"tiles": [all[0], all[1]]}));
        let before = lender.with(|c| c.store.library.sets[1].tiles.len());
        let set = b.harness.get_by_label_contains(", set 2 of ").rect().center();
        let from = rect(&b, 0).center();
        modifiers(&mut b, Modifiers::ALT);
        press(&mut b, from, set, Modifiers::ALT);
        release(&mut b, set, Modifiers::ALT);
        let (n, last_two, here) = lender.with(|l| {
            let t = &l.store.library.sets[1].tiles;
            (t.len(), t[t.len() - 2..].iter().map(|t| (t.id, t.title.clone())).collect::<Vec<_>>(), l.current_set().tiles.iter().map(|t| t.id).collect::<Vec<_>>())
        });
        assert_eq!(n, before + 2, "{theme:?}: two copies in the other set");
        assert_eq!(last_two.iter().map(|x| x.1.clone()).collect::<Vec<_>>(), vec![s[0][0].clone(), s[0][1].clone()], "{theme:?}: in order, at its end");
        assert!(last_two.iter().all(|x| !all.contains(&x.0)), "{theme:?}: copies, not the originals");
        assert_eq!(here, all, "{theme:?}: this set keeps its pads");
    }
}

/// Retest A1 (D-191): a long notice in the status bar is cut short with "…" and never
/// runs over the "Idle" on its left, at the window's narrowest too; hovered, it shows
/// the whole text and stays up after its time.
#[test]
fn a_long_status_notice_never_runs_over_idle() {
    let mut b = Board::new(ThemeKind::Console);
    b.harness.set_size(vec2(LAYOUT_MIN[0], LAYOUT_MIN[1]));
    let long = "the grid has slots now: drag a sound between two others, into a blank slot or below the last row, and the board shows where it goes";
    let lender = b.harness.state().lender.clone();
    lender.with(|c| c.notify(long));
    b.harness.run_steps(3);
    // The status bar's label (the live region has the same words, unseen).
    let shown = |b: &Board| b.harness.query_all_by_label_contains("the grid has slots now").filter(|n| n.accesskit_node().bounding_box().is_some()).map(|n| n.rect()).find(|r| r.height() > 0.0 && r.bottom() > LAYOUT_MIN[1] - 40.0);
    let idle = b.harness.get_by_label("Idle").rect();
    let n = shown(&b).expect("the notice shows");
    assert!(n.left() >= idle.right() && n.right() <= LAYOUT_MIN[0], "the notice {n:?} stays clear of Idle {idle:?}");
    assert!(n.width() < 700.0, "cut short (the whole text is about 1000 px): {n:?}");
    // Hovered past its 4 s: still there, with the whole text in the tooltip.
    b.harness.hover_at(n.center());
    b.harness.run_steps(3);
    lender.with(|c| c.notice.as_mut().unwrap().at -= Duration::from_secs(10));
    b.harness.run_steps(3);
    assert!(shown(&b).is_some(), "it stays while hovered");
    assert!(b.harness.query_all_by_label(long).count() >= 1, "the full text on hover");
    // Let go: it goes.
    b.harness.hover_at(pos2(LAYOUT_MIN[0] / 2.0, 100.0));
    b.harness.run_steps(3);
    assert!(shown(&b).is_none());
}

/// A Finder drop on macOS (t-m20-drop-target): winit gives the files but no position,
/// and AppKit sends no mouse moves during the drag, so egui's pointer is wherever it was
/// before (here: gone, as the cursor left for Finder). `MacDrops` puts AppKit's cursor
/// position into each frame's input instead; `mac_drop::follow` is that step.
fn finder_drop(b: &mut Board, at: Pos2, files: &[PathBuf]) {
    finder_hover(b, at, files);
    finder_release(b, at, files);
}

/// The first half of [`finder_drop`]: the files held over `at`.
fn finder_hover(b: &mut Board, at: Pos2, files: &[PathBuf]) {
    b.harness.event(egui::Event::PointerMoved(pos2(4.0, 4.0)));
    b.harness.run_steps(1);
    b.harness.event(egui::Event::PointerGone);
    b.harness.run_steps(1);
    let zoom = b.harness.ctx.zoom_factor();
    for _ in 0..3 {
        let raw = b.harness.input_mut();
        raw.hovered_files = files.iter().map(|f| egui::HoveredFile { path: Some(f.clone()), ..Default::default() }).collect();
        crate::mac_drop::follow(Some(at * zoom), zoom, raw);
        b.harness.run_steps(1);
    }
}

/// The second half of [`finder_drop`]: the files let go at `at`.
fn finder_release(b: &mut Board, at: Pos2, files: &[PathBuf]) {
    let zoom = b.harness.ctx.zoom_factor();
    let raw = b.harness.input_mut();
    raw.hovered_files.clear();
    raw.dropped_files = files.iter().map(|f| Arc::new(crate::wayland_drop::DroppedPath(f.clone())) as egui::DroppedFileHandle).collect();
    crate::mac_drop::follow(Some(at * zoom), zoom, raw);
    b.harness.run_steps(3);
}

/// t-m20-record-ux (D-127), in both themes: Record New Tile scrolls the board to the new
/// tile and opens it in the inspector; the tile, the status bar and the counter say it's
/// recording and from which input; Stop Recording is in the status bar and in the hero;
/// Settings shows the input with ducking off; Preview is the hero's big button.
#[test]
fn a_recording_shows_where_from_and_how_to_stop() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        // Enough tiles that the end of the set is below the window.
        for _ in 0..30 {
            b.run("tile.addAction", json!({"kind": "stop", "targets": ["#1"]}));
        }
        lender.with(|c| {
            c.engine.device_name = Some("Fake Speakers".into());
            c.input_lister = || (vec!["Fake Mic".into()], Some("Fake Mic".into()));
            c.input_opener = Arc::new(|device, voice| Ok(crate::input::Input::fake("Fake Mic", device, voice)));
        });
        let id: Uuid = lender.with(|c| c.execute("record.start", json!({}))).unwrap()["tile"].as_str().unwrap().parse().unwrap();
        let l = lender.clone();
        b.until("the input to open", move |_| l.with(|c| c.input.is_some()));
        b.harness.run_steps(3);
        assert_eq!(lender.with(|c| c.selected_tile), Some(id), "{theme:?}: open in the inspector");
        let rect = b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2;
        assert!(b.harness.state().board_rect.contains(rect.center()), "{theme:?}: scrolled into view: {rect:?}");
        assert!(b.harness.query_by_label_contains("Recording 0:00 from Fake Mic").is_some(), "{theme:?}: the status bar says so");
        assert!(b.harness.query_by_label_contains("recording from Fake Mic").is_some(), "{theme:?}: the tile says so");
        assert_eq!(b.harness.get_all_by_label("Stop Recording").count(), 2, "{theme:?}: status bar and inspector");
        assert_eq!(gaps(&b, "recording"), Vec::<String>::new());
        b.click("Stop Recording");
        assert!(lender.with(|c| c.recording.is_none()), "{theme:?}: the status bar's Stop stops it");

        // Settings ▸ Voice shows the input with ducking off.
        b.run("view.settings", json!({"open": true}));
        assert!(!lender.with(|c| c.store.library.voice_duck));
        assert!(b.harness.query_by_label("Input device").is_some(), "{theme:?}: the input picker");
        b.close_windows();

        // Preview is the hero's big button (D-144): under the title, above the options.
        b.run("view.select", json!({"tile": "#1"}));
        let top = |b: &Board, label: &str| b.harness.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label:?}")).rect().top();
        assert!(top(&b, "Title") < top(&b, "Preview"), "{theme:?}: Preview under the title");
        assert!(top(&b, "Preview") < top(&b, "Loop"), "{theme:?}: and above the options");
    }
}

/// Finder files go where they're dropped, as dragged pads do (D-142): into the gap
/// under the pointer (one sound on a tile is a new tile beside it, never a replacement),
/// with the board opening as many slots as there are files while they're held there; a
/// picture becomes the tile's picture; below the last row they make a row of their own.
#[test]
fn a_finder_drop_lands_on_the_tile_under_it() {
    let mut b = Board::new(ThemeKind::Console);
    let lender = b.harness.state().lender.clone();
    let tiles = |l: &Arc<Lender>| l.with(|c| c.current_set().tiles.iter().map(|t| (t.id, t.file.clone(), t.image.clone())).collect::<Vec<_>>());
    let rect = |b: &Board, i: usize| b.harness.state().tile_rects.iter().find(|(_, j, _)| *j == i).expect("drawn").2;
    let before = tiles(&lender);
    let sound = b.dir.0.join("Dropped Sound.wav");
    crate::wav::write_tone(&sound, 8000, 0.2, &[440.0], 0.3).expect("write a tone");
    let other = b.dir.0.join("Another Sound.wav");
    crate::wav::write_tone(&other, 8000, 0.2, &[660.0], 0.3).expect("write a tone");
    let picture = b.dir.0.join("Dropped Picture.png");
    std::fs::write(&picture, crate::picture::tests::tiny_png()).expect("write a picture");

    // One sound on the second tile's right half: a new tile after it; nothing is
    // replaced (D-142: Replace Audio… does that).
    let r = rect(&b, 1);
    finder_drop(&mut b, pos2(r.right() - 10.0, r.center().y), std::slice::from_ref(&sound));
    b.settle();
    let after = tiles(&lender);
    assert_eq!(after.len(), before.len() + 1, "added, not replaced");
    assert_eq!((after[1].0, &after[1].1), (before[1].0, &before[1].1), "the second tile keeps its sound");
    assert!(!before.iter().any(|t| t.0 == after[2].0), "the new tile is third");
    let before = after;

    // Two sounds held over the first tile's right half: the board opens two slots
    // there, pushing the second tile along, and they land after the first tile.
    let (r0, r1) = (rect(&b, 0), rect(&b, 1));
    let at = pos2(r0.right() - 10.0, r0.center().y);
    let both = [sound.clone(), other.clone()];
    finder_hover(&mut b, at, &both);
    for _ in 0..30 {
        b.harness.run_steps(1);
    }
    let moved = rect(&b, 1);
    assert!(moved.left() > r1.left() + r1.width() || moved.top() > r1.top(), "the second tile slides along: {r1:?} → {moved:?}");
    finder_release(&mut b, at, &both);
    b.settle();
    let after = tiles(&lender);
    assert_eq!(after.len(), before.len() + 2);
    assert_eq!((after[0].0, after[3].0), (before[0].0, before[1].0), "both land after the first tile, before the second");

    let onto = rect(&b, 2).center();
    finder_drop(&mut b, onto, std::slice::from_ref(&picture));
    let pictured = tiles(&lender);
    assert!(pictured[2].2.is_some() && pictured[2].2 != after[2].2, "the third tile has the dropped picture");
    assert_eq!(pictured.len(), after.len(), "a picture never adds a tile");

    let before = pictured;
    let last = rect(&b, before.len() - 1);
    finder_drop(&mut b, pos2(last.center().x, last.bottom() + 6.0), std::slice::from_ref(&sound));
    b.settle();
    let after = tiles(&lender);
    assert_eq!(after.len(), before.len() + 1, "a new tile");
    assert_eq!(&after[..before.len()].iter().map(|t| t.0).collect::<Vec<_>>(), &before.iter().map(|t| t.0).collect::<Vec<_>>());
    lender.with(|c| assert_eq!(c.current_set().row_lengths().last(), Some(&1), "below the last row: a row of its own"));
}

/// D-152: the full editor on a locked set's tile is visibly read-only, in both looks. A
/// banner says so with Unlock; the handles, the trim and marker fields and the actions
/// that would change the tile are disabled and say so; previewing, the cursor and the
/// selection still work; Unlock brings the controls back.
#[test]
fn the_editor_of_a_locked_tile_is_read_only() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        b.run("tile.markerAdd", json!({"tile": "#1", "at": 0.1, "name": "Hit"}));
        b.run("tile.set", json!({"tile": "#1", "vampStart": 0.2, "vampEnd": 0.4}));
        b.run("set.lock", json!({}));
        // Only the editor's Unlock, not the inspector's.
        b.run("view.inspector", json!({"open": false}));
        b.run("view.editor", json!({"tile": "#1"}));
        let lender = b.harness.state().lender.clone();
        let tile = |l: &Arc<Lender>| l.with(|c| c.store.library.tile(c.editor.unwrap()).cloned().unwrap());
        let before = tile(&lender);
        assert!(b.harness.query_by_label_contains("is locked: this editor is read-only").is_some(), "{theme:?}: the banner");

        let slider = |b: &Board, name: &str| b.harness.get_all_by_label(name).any(|n| n.accesskit_node().role() == Role::Slider && n.accesskit_node().is_disabled());
        for name in ["Trim start, read-only", "Trim end, read-only", "Vamp start, read-only", "Vamp end, read-only", "Marker Hit, read-only"] {
            assert!(slider(&b, name), "{theme:?}: {name} is a disabled slider");
        }
        let disabled = |b: &Board, name: &str| {
            let nodes: Vec<_> = b.harness.get_all_by_label(name).collect();
            !nodes.is_empty() && nodes.iter().all(|n| n.accesskit_node().is_disabled())
        };
        for name in [
            "Trim start, time", "Trim end, samples", "Vamp start, time", "Marker Hit, name", "Marker Hit, time", "Add marker at cursor",
            "Start here", "End here", "Reset Trim", "Trim to Selection", "Vamp Selection", "Remove Hit", "Vamp", "Snap", "Add marker at playhead",
        ] {
            assert!(disabled(&b, name), "{theme:?}: {name} is disabled");
        }
        // A disabled control says why.
        let why = b.harness.get_by_label("Reset Trim").accesskit_node().description().unwrap_or_default().to_owned();
        assert!(why.contains("locked"), "{theme:?}: {why:?}");
        for name in ["Preview", "Waveform view", "Cursor, time", "Selection start, time", "Jump", "Close Editor"] {
            assert!(b.harness.get_all_by_label(name).any(|n| !n.accesskit_node().is_disabled()), "{theme:?}: {name} still works");
        }

        // Selecting and previewing work; nothing about the tile changes.
        for (field, text) in [("Selection start, time", "0:00.050"), ("Selection end, time", "0:00.300")] {
            b.harness.get_by_label(field).focus();
            b.harness.run_steps(2);
            b.press_all(Key::A, Modifiers::COMMAND);
            b.harness.get_by_label(field).type_text(text);
            b.press(Key::Enter);
        }
        assert_eq!(lender.with(|c| c.editor_selection), Some((0.05, 0.3)), "{theme:?}: the selection still moves");
        b.harness.get_all_by_label("Preview").find(|n| !n.accesskit_node().is_disabled()).expect("Preview").click();
        b.harness.run_steps(3);
        assert!(lender.with(|c| c.notice.as_ref().is_none_or(|n| !n.text.contains("locked"))), "{theme:?}: previewing isn't refused");
        assert_eq!(tile(&lender), before, "{theme:?}: nothing changed");

        b.click("Unlock");
        assert!(!lender.with(|c| c.current_set().locked), "{theme:?}: Unlock in the banner unlocks the set");
        assert!(b.harness.query_by_label_contains("this editor is read-only").is_none());
        let start = b.harness.get_all_by_label("Trim start").find(|n| n.accesskit_node().role() == Role::Slider).expect("the start handle");
        assert!(!start.accesskit_node().is_disabled(), "{theme:?}: the handles come back");
        assert!(!disabled(&b, "Reset Trim"));
    }
}

/// The editor works without a mouse: its handles take the arrows (Alt: one sample), its
/// time fields take typed times, and the focused waveform zooms with + and −.
#[test]
fn the_editor_works_from_the_keyboard() {
    let mut b = Board::new(ThemeKind::Console);
    b.run("view.editor", json!({"tile": "#1"}));
    let lender = b.harness.state().lender.clone();
    let tile = |l: &Arc<Lender>| l.with(|c| c.store.library.tile(c.editor.unwrap()).cloned().unwrap());
    let rate = lender.with(|c| c.store.audio(c.store.library.tile(c.editor.unwrap()).unwrap()).unwrap().sample_rate);
    assert!(b.harness.query_by_label("Waveform view").is_some(), "the editor shows its waveform");

    b.harness.get_all_by_label("Trim start").find(|n| n.accesskit_node().role() == Role::Slider).expect("the start handle").focus();
    b.harness.run_steps(2);
    b.harness.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    b.harness.run_steps(3);
    assert!((tile(&lender).start - 1.0 / f64::from(rate)).abs() < 1e-9, "Alt+Right moves the start one sample: {}", tile(&lender).start);

    let field = b.harness.get_by_label("Trim end, time");
    field.focus();
    b.harness.run_steps(2);
    b.press_all(Key::A, Modifiers::COMMAND);
    b.harness.get_by_label("Trim end, time").type_text("0:00.500");
    b.press(Key::Enter);
    assert!((tile(&lender).end.unwrap() - 0.5).abs() < 1e-9, "a typed end: {:?}", tile(&lender).end);
    let field = b.harness.get_by_label("Trim start, samples");
    field.focus();
    b.harness.run_steps(2);
    b.press_all(Key::A, Modifiers::COMMAND);
    b.harness.get_by_label("Trim start, samples").type_text("100smp");
    b.press(Key::Enter);
    assert!((tile(&lender).start - 100.0 / f64::from(rate)).abs() < 1e-9, "a typed sample: {}", tile(&lender).start);

    b.harness.get_by_label("Waveform view").focus();
    b.harness.run_steps(2);
    let before = b.harness.state().editor_view.as_ref().and_then(|v| v.view()).unwrap();
    b.press(Key::Plus);
    let after = b.harness.state().editor_view.as_ref().and_then(|v| v.view()).unwrap();
    assert!((after.fpp - before.fpp / 2.0).abs() < 1e-9, "+ zooms in: {before:?} → {after:?}");
    b.press(Key::Minus);
    let back = b.harness.state().editor_view.as_ref().and_then(|v| v.view()).unwrap();
    assert!((back.fpp - before.fpp).abs() < 1e-9);

    // Selecting another tile closes it; the menu item toggles it.
    b.run("view.select", json!({"tile": "#2"}));
    assert!(lender.with(|c| c.editor.is_none()));
    b.run("view.editor", json!({}));
    assert!(lender.with(|c| c.editor.is_some()));
    let refused = lender.with(|c| c.execute("view.editor", json!({"tile": "#13"}))).unwrap_err();
    assert_eq!(refused.code, "bad_params", "an action tile has no sound to edit");
}

/// The editor's pointer and gesture input through egui's own event pipeline, as winit
/// hands it over: a drag across the waveform selects, a double-click selects between
/// marks, a drag along the ruler pans, a pinch (winit's PinchGesture becomes
/// `Event::Zoom`) and ⌘/Ctrl+scroll zoom around the pointer, over a handle too. The
/// selection's fields and buttons and a marker's name field work from the keyboard (D-104).
#[test]
fn the_editor_selects_zooms_and_renames() {
    use egui::{MouseWheelSource, MouseWheelUnit, PointerButton, TouchPhase};
    let mut b = Board::new(ThemeKind::Panel);
    b.run("tile.markerAdd", json!({"tile": "#1", "at": 0.1, "name": "Hit"}));
    b.run("view.editor", json!({"tile": "#1"}));
    let lender = b.harness.state().lender.clone();
    let tile = |l: &Arc<Lender>| l.with(|c| c.store.library.tile(c.editor.unwrap()).cloned().unwrap());
    let (rate, dur) = lender.with(|c| {
        let a = c.store.audio(c.store.library.tile(c.editor.unwrap()).unwrap()).unwrap();
        (f64::from(a.sample_rate), a.duration())
    });
    let selection = |l: &Arc<Lender>| l.with(|c| c.editor_selection);
    let view = |b: &Board| b.harness.state().editor_view.as_ref().and_then(|v| v.view()).unwrap();
    let wave = b.harness.get_by_label("Waveform view").rect();
    let sec = |v: editor::View, x: f32| v.frame_at(x - wave.left()) / rate;
    let press = |b: &mut Board, pos: Pos2, pressed: bool, modifiers: Modifiers| {
        b.harness.event(egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
        b.harness.run_steps(1);
    };

    // A drag across the middle selects it (snapped within a few points), and doesn't pan.
    let before = view(&b);
    let y = wave.center().y;
    let (x0, x1) = (wave.left() + wave.width() / 3.0, wave.left() + wave.width() * 2.0 / 3.0);
    b.harness.hover_at(pos2(x0, y));
    b.harness.run_steps(1);
    press(&mut b, pos2(x0, y), true, Modifiers::NONE);
    for k in 1..=6 {
        b.harness.hover_at(pos2(x0 + (x1 - x0) * k as f32 / 6.0, y));
        b.harness.run_steps(1);
    }
    press(&mut b, pos2(x1, y), false, Modifiers::NONE);
    b.harness.run_steps(2);
    let near = 8.0 * before.fpp / rate;
    let (a, z) = selection(&lender).expect("a drag selects");
    assert!((a - sec(before, x0)).abs() <= near && (z - sec(before, x1)).abs() <= near, "{a}..{z}");
    assert_eq!(view(&b), before, "selecting doesn't pan");

    // Shift+click extends it from its far end; a click clears it and places the cursor.
    let x2 = wave.left() + wave.width() * 0.9;
    b.harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    press(&mut b, pos2(x2, y), true, Modifiers::SHIFT);
    press(&mut b, pos2(x2, y), false, Modifiers::SHIFT);
    b.harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    b.harness.run_steps(2);
    let (a2, z2) = selection(&lender).unwrap();
    assert!((a2 - a).abs() < 1e-9 && (z2 - sec(before, x2)).abs() <= near, "extended: {a2}..{z2}");
    press(&mut b, pos2(x0, y), true, Modifiers::NONE);
    press(&mut b, pos2(x0, y), false, Modifiers::NONE);
    b.harness.run_steps(2);
    assert_eq!(selection(&lender), None, "a click clears the selection");
    assert!(b.harness.state().editor_view.as_ref().unwrap().cursor.is_some(), "and places the cursor");

    // A double-click selects between the marks either side: the marker Hit and the end.
    // (The harness steps a quarter of a second a frame, slower than any double-click.)
    b.harness.ctx.options_mut(|o| o.input_options.max_double_click_delay = 1.5);
    b.harness.run_steps(8);
    for _ in 0..2 {
        press(&mut b, pos2(x0, y), true, Modifiers::NONE);
        press(&mut b, pos2(x0, y), false, Modifiers::NONE);
    }
    b.harness.run_steps(2);
    assert_eq!(selection(&lender), Some((0.1, dur)), "between Hit and the end");

    // A pinch zooms around the pointer, over the start handle too.
    let at = pos2(wave.left() + 2.0, y);
    b.harness.hover_at(at);
    b.harness.run_steps(1);
    let under = sec(view(&b), at.x);
    b.harness.event(egui::Event::Zoom(4.0));
    b.harness.run_steps(2);
    let pinched = view(&b);
    assert!((pinched.fpp - before.fpp / 4.0).abs() < 1e-9, "a pinch zooms in: {before:?} → {pinched:?}");
    assert!((sec(pinched, at.x) - under).abs() < 1e-6, "around the pointer");
    // ⌘/Ctrl+scroll (a wheel turned with the zoom modifier held) zooms too.
    let mid = wave.center();
    b.harness.hover_at(mid);
    let wheel = egui::Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, -2.0), phase: TouchPhase::Move, source: MouseWheelSource::Wheel, modifiers: Modifiers::COMMAND };
    b.harness.event_modifiers(wheel, Modifiers::COMMAND);
    b.harness.run_steps(20);
    let wheeled = view(&b);
    assert!(wheeled.fpp > pinched.fpp * 1.2, "⌘+wheel down zooms out: {pinched:?} → {wheeled:?}");
    // Dragging the ruler pans, and selects nothing.
    b.run("view.editorSelect", json!({"clear": true}));
    b.run("view.editorZoom", json!({"from": 0.1, "to": 0.2}));
    let zoomed = view(&b);
    let ruler = pos2(mid.x, wave.top() + 6.0);
    b.harness.hover_at(ruler);
    b.harness.run_steps(1);
    press(&mut b, ruler, true, Modifiers::NONE);
    for k in 1..=5 {
        b.harness.hover_at(pos2(ruler.x - 20.0 * k as f32, ruler.y));
        b.harness.run_steps(1);
    }
    press(&mut b, pos2(ruler.x - 100.0, ruler.y), false, Modifiers::NONE);
    b.harness.run_steps(2);
    let panned = view(&b);
    assert!((panned.start - zoomed.start - 100.0 * zoomed.fpp).abs() < 2.0 * zoomed.fpp, "dragging the ruler left moves later: {zoomed:?} → {panned:?}");
    assert_eq!(selection(&lender), None);

    // From the keyboard: type a selection, then trim and vamp to it.
    for (field, text) in [("Selection start, time", "0:00.050"), ("Selection end, time", "0:00.300")] {
        b.harness.get_by_label(field).focus();
        b.harness.run_steps(2);
        b.press_all(Key::A, Modifiers::COMMAND);
        b.harness.get_by_label(field).type_text(text);
        b.press(Key::Enter);
    }
    assert_eq!(selection(&lender), Some((0.05, 0.3)));
    for button in ["Trim to Selection", "Vamp Selection"] {
        b.harness.get_by_label(button).focus();
        b.harness.run_steps(2);
        b.press(Key::Enter);
    }
    let t = tile(&lender);
    assert_eq!((t.start, t.end, t.vamp), (0.05, Some(0.3), Some((0.05, 0.3))), "trimmed and vamped to the selection");
    b.harness.get_by_label("Zoom to selection").focus();
    b.harness.run_steps(2);
    b.press(Key::Enter);
    b.harness.run_steps(2);
    let fitted = view(&b);
    assert!(sec(fitted, wave.left()) < 0.05 && sec(fitted, wave.right()) > 0.3 && sec(fitted, wave.right()) < 0.33, "{fitted:?}");

    // A marker's name field renames it on Enter; Esc puts it back.
    let name = "Marker Hit, name";
    b.harness.get_by_label(name).focus();
    b.harness.run_steps(2);
    b.press_all(Key::A, Modifiers::COMMAND);
    b.harness.get_by_label(name).type_text("Boom");
    b.press(Key::Enter);
    assert_eq!(tile(&lender).markers[0].name, "Boom");
    b.harness.get_by_label("Marker Boom, name").focus();
    b.harness.run_steps(2);
    b.harness.get_by_label("Marker Boom, name").type_text("x");
    b.press(Key::Escape);
    assert_eq!(tile(&lender).markers[0].name, "Boom", "Esc keeps the old name");
    // And its time field moves it.
    b.harness.get_by_label("Marker Boom, time").focus();
    b.harness.run_steps(2);
    b.press_all(Key::A, Modifiers::COMMAND);
    b.harness.get_by_label("Marker Boom, time").type_text("0:00.200");
    b.press(Key::Enter);
    assert_eq!(tile(&lender).markers[0].at, 0.2);
}

/// In the cue list the focus selects a row, Enter plays it (Play Selection) and the
/// selection stays on it; Space is Pause All (D-197).
#[test]
fn the_cue_list_works_from_the_keyboard() {
    let mut b = Board::new(ThemeKind::Panel);
    b.run("set.select", json!({"set": "#2"}));
    b.tab_to_board();
    let first = b.focused_title().expect("a row has focus");
    b.press(Key::ArrowDown);
    let second = b.focused_title().expect("a row has focus");
    assert_ne!(first, second, "Down moves the focus to the next row");
    b.harness.run_steps(2);
    let lender = b.harness.state().lender.clone();
    let pointer = |l: &std::sync::Arc<Lender>| l.with(|c| c.cue.get(&c.current_set().id).copied().unwrap_or(0));
    assert_eq!(pointer(&lender), 1, "the focused row is selected");
    b.press(Key::Enter);
    b.harness.run_steps(3);
    assert!(b.playing(&second), "Enter plays it");
    assert_eq!((pointer(&lender), b.focused_title()), (1, Some(second.clone())), "and it stays selected");
    // Space pauses everything, and again carries on.
    let state = |l: &std::sync::Arc<Lender>, t: &str| l.with(|c| c.current_set().tiles.iter().find(|x| x.title == t).map(|x| c.engine.state(x.id)));
    b.press(Key::Space);
    b.harness.run_steps(2);
    assert_eq!(state(&lender, &second), Some(PlayState::Paused));
    b.press(Key::Space);
    b.harness.run_steps(2);
    assert_eq!(state(&lender, &second), Some(PlayState::Playing));
    // Each row's armed mark is a check box a screen reader can toggle.
    let mark = |b: &Board, n: usize| {
        b.harness.query_all_by_label(&format!("Cue {n} armed")).next().map(|n| n.accesskit_node().toggled())
    };
    assert_eq!((mark(&b, 1), mark(&b, 2)), (Some(Some(Toggled::True)), Some(Some(Toggled::False))));
    b.run("transport.stopAll", json!({"immediate": true}));
}

/// Space is Pause All in every set, on a focused grid pad too; Enter plays the pad (D-199).
#[test]
fn space_pauses_everything_on_the_grid_too() {
    let mut b = Board::new(ThemeKind::Console);
    b.tab_to_board();
    let pad = b.focused_title().expect("a pad has focus");
    b.press(Key::Space);
    b.harness.run_steps(2);
    assert!(!b.playing(&pad), "Space doesn't play the focused pad");
    b.press(Key::Enter);
    b.harness.run_steps(2);
    assert!(b.playing(&pad), "Enter does");
    let lender = b.harness.state().lender.clone();
    let paused = |l: &std::sync::Arc<Lender>| l.with(|c| c.engine.statuses().values().all(|s| s.state == PlayState::Paused));
    b.press(Key::Space);
    b.harness.run_steps(2);
    assert!(paused(&lender), "Space pauses everything");
    b.press(Key::Space);
    b.harness.run_steps(2);
    assert!(b.playing(&pad) && !paused(&lender), "and again carries on");
    b.run("transport.stopAll", json!({"immediate": true}));
}

/// A set file opened from the file manager is imported only after Import (D-091).
#[test]
fn an_opened_set_file_asks_before_importing() {
    let mut b = Board::new(ThemeKind::Console);
    let lender = b.harness.state().lender.clone();
    let sets = || lender.with(|c| c.store.library.sets.len());
    let before = sets();
    let file = b.dir.0.join("Show.lsset");
    b.run("set.export", json!({"path": file}));
    b.run("set.import", json!({"path": file, "ask": true}));
    assert!(
        b.harness.root().children_recursive().any(|n| n.accesskit_node().role() == Role::Dialog && n.accesskit_node().label().as_deref() == Some("Import this set file?")),
        "the dialog is named, so a screen reader says more than \"dialog\""
    );
    b.click("Cancel");
    assert_eq!(sets(), before, "Cancel imports nothing");
    assert!(b.harness.query_by_label("Import this set file?").is_none());
    // Opened twice before answering: asked once.
    b.run("set.import", json!({"path": file, "ask": true}));
    b.run("set.import", json!({"path": file, "ask": true}));
    assert_eq!(b.harness.state().confirm_sets.len(), 1);
    b.press(Key::Enter);
    assert_eq!(sets(), before + 1, "Enter (Import has the focus) imports it");
    assert!(b.harness.state().confirm_sets.is_empty());
}

/// A QuipClipper link from a server that isn't connected asks first, with Cancel focused,
/// so a key meant for the board cancels; Connect connects (D-132) and then imports.
#[test]
fn an_unknown_quipclipper_server_asks_before_connecting() {
    use crate::quip::tests::{response, serve};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let p = std::env::temp_dir().join(format!("sb-quip-ask-{}.wav", uuid::Uuid::new_v4()));
    crate::wav::write_tone(&p, 8000, 1.0, &[440.0], 0.4).unwrap();
    let wav = std::fs::read(&p).unwrap();
    std::fs::remove_file(p).ok();
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    // An open QuipClipper without pairing: Connect connects it without a token.
    let base = serve(move |req, _| {
        h.fetch_add(1, Ordering::SeqCst);
        match req {
            "GET /api/config HTTP/1.1" => response("200 OK", "", br#"{"media_roots": [], "auth_required": false}"#),
            "GET /api/clips/download/a.wav HTTP/1.1" => response("200 OK", "Content-Type: audio/wav\r\n", &wav),
            _ => response("404 Not Found", "", b"{}"),
        }
    });
    let mut b = Board::new(ThemeKind::Console);
    let lender = b.harness.state().lender.clone();
    let tiles = || lender.with(|c| c.store.library.sets.iter().map(|s| s.tiles.len()).sum::<usize>());
    let before = tiles();
    let link = format!("leadstinger://quip?server={base}&clip=a.wav&title=Hold%20on");
    let dialog = |b: &Board| b.harness.root().children_recursive().any(|n| n.accesskit_node().role() == Role::Dialog && n.accesskit_node().label().as_deref() == Some("Connect to QuipClipper?"));

    let (id, params) = crate::quip::opened_command(&link);
    b.run(id, params.clone());
    assert!(dialog(&b), "the prompt is a named dialog");
    assert!(b.harness.query_by_label("Hold on").is_none() && b.harness.query_by_label_contains("It sent “Hold on”").is_some());
    // Space, the GO key, presses Cancel: nothing connects, nothing is fetched.
    b.press(Key::Space);
    assert!(!dialog(&b));
    assert_eq!(hits.load(Ordering::SeqCst), 0, "nothing went over the network");
    assert_eq!(tiles(), before);
    // Cancelled: the same server's links are refused for now, without asking again.
    b.run(id, params.clone());
    assert!(!dialog(&b));
    assert!(lender.with(|c| c.notice.as_ref().is_some_and(|n| n.text.contains("chose not to connect"))));

    // Connect from Settings forgives it; a new link asks again, and Connect and Import works.
    b.harness.state_mut().quip_declined.clear();
    b.run(id, params);
    assert!(dialog(&b));
    b.click("Connect and Import");
    b.until("the clip imported", |_| tiles() == before + 1);
    assert!(!dialog(&b));
    let dir = lender.with(|c| c.store.data_dir.clone());
    assert_eq!(crate::quip::token_for(&dir, &base).unwrap().as_deref(), Some(""), "connected without a token");
}

impl Board {
    /// What the live region says now (the status node's label, without the no-break
    /// space that marks a repeat).
    /// Step until the live region starts with `prefix` (an import's announcement comes
    /// a frame or two after the import finishes), then return what it says.
    fn announced_after(&mut self, prefix: &str) -> String {
        self.until(&format!("an announcement starting {prefix:?}"), |b| b.announced().starts_with(prefix));
        self.announced()
    }

    fn announced(&self) -> String {
        let node = self.harness.get_by_role(Role::Status);
        node.accesskit_node().label().unwrap_or_default().trim_end_matches('\u{a0}').to_owned()
    }
}

/// Screen readers hear what changes without moving focus (t-m20-announce, D-093).
#[test]
fn changes_are_announced_in_a_live_region() {
    let mut b = Board::new(ThemeKind::Console);
    let node = b.harness.get_by_role(Role::Status);
    assert_eq!(node.accesskit_node().live(), egui::accesskit::Live::Polite);
    let lender = b.harness.state().lender.clone();
    let title = |n: usize| lender.with(|c| c.current_set().tiles[n].title.clone());
    b.run("tile.trigger", json!({"tile": "#1"}));
    assert_eq!(b.announced(), format!("Playing {}", title(0)));
    b.run("transport.stopAll", json!({"immediate": true}));
    assert_eq!(b.announced(), "Stop All: stopped");
    // The same words again still change the label, so they're read again.
    let before = b.harness.get_by_role(Role::Status).accesskit_node().label().unwrap_or_default();
    b.run("transport.stopAll", json!({"immediate": true}));
    assert_ne!(b.harness.get_by_role(Role::Status).accesskit_node().label().unwrap_or_default(), before);
    // A cue list: Play Selection plays the selected cue, which stays selected (D-197).
    b.run("set.select", json!({"set": "#2"}));
    assert_eq!(lender.with(|c| c.current_set().layout), Layout::List);
    b.run("cue.go", json!({}));
    assert_eq!(b.announced(), format!("Playing {}", title(0)));
    b.run("transport.stopAll", json!({"immediate": true}));
    // Queued: the stopped cue hands the selection to the next armed one, which is said.
    assert_eq!(b.announced_after("Next cue"), format!("Next cue 2, {}", title(1)));
    // An import finishing, and one failing.
    let wav = b.dir.0.join("Short Beep.wav");
    let samples: Vec<f32> = (0..4_410).map(|i| (i as f32 * 0.05).sin() * 0.2).collect();
    crate::wav::write_multichannel(&wav, 44_100, 0x4, &samples).expect("write a mono file");
    b.run("tile.add", json!({"paths": [wav]}));
    b.settle();
    assert_eq!(b.announced_after("Imported"), "Imported Short Beep");
    let bad = b.dir.0.join("Broken.wav");
    std::fs::write(&bad, b"not audio").unwrap();
    // With ffmpeg on PATH the import starts and fails in the background (announced);
    // without it (the Nix build's sandbox) tile.add refuses the file at once.
    match lender.with(|c| c.execute("tile.add", json!({"paths": [bad]}))) {
        Ok(_) => {
            b.harness.run_steps(3);
            b.settle();
            let said = b.announced_after("Couldn't import");
            assert!(said.starts_with("Couldn't import Broken"), "{said}");
        }
        Err(e) => assert!(e.to_string().contains("Broken.wav"), "{e}"),
    }
    // A set file opened from the file manager.
    let file = b.dir.0.join("Show.lsset");
    b.run("set.export", json!({"path": file}));
    b.run("set.import", json!({"path": file, "ask": true}));
    assert_eq!(b.announced(), "Set file opened: Show.lsset. Import it?");
}

/// The QuipClipper window (M21): its clips are a list box a screen reader names; Up and
/// Down move along it, Space previews, Enter adds to the current set, and tile keys
/// don't fire while it has the focus.
#[test]
fn the_quip_browser_works_from_the_keyboard() {
    let mut b = Board::new(ThemeKind::Console);
    b.open_quip();
    let root = b.harness.root();
    let list = root.children_recursive().find(|n| n.accesskit_node().role() == Role::ListBox && n.accesskit_node().label().as_deref() == Some("Clips")).expect("a list box named Clips");
    let rows: Vec<String> = list.children_recursive().filter(|n| n.accesskit_node().role() == Role::ListBoxOption).filter_map(|n| n.accesskit_node().label()).collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].starts_with("Don't let yourself get attached, Heat (1995), audio clip, 2.0 seconds, 5.1"), "{rows:?}");
    assert!(root.children_recursive().any(|n| n.accesskit_node().label().as_deref() == Some("“[old] cuts”, folder")), "a folder name that starts with a symbol is quoted");

    let focused = |b: &Board| b.harness.root().children_recursive().find(|n| n.accesskit_node().is_focused()).and_then(|n| n.accesskit_node().label()).unwrap_or_default();
    b.harness.get_all_by_label_contains("Don't let yourself get attached").find(|n| n.accesskit_node().role() == Role::ListBoxOption).expect("the row").focus();
    b.harness.run_steps(3);
    b.press(Key::ArrowDown);
    assert!(focused(&b).starts_with("Quips.lsset"), "Down moves to the next clip: {}", focused(&b));
    b.press(Key::ArrowUp);
    assert!(focused(&b).starts_with("Don't let"), "Up moves back: {}", focused(&b));

    // Tile keys belong to the window while it has the focus (Air Horn is key Q: row 2, D-188).
    b.press(Key::Q);
    assert!(!b.playing("Air Horn"), "a tile key fired from the QuipClipper window");

    let lender = b.harness.state().lender.clone();
    b.press(Key::Space);
    let previewing = lender.with(|c| c.quip_jobs.preview_state(true));
    assert!(matches!(previewing, Some(crate::quip::clips::PreviewState::Loading(ref c) | crate::quip::clips::PreviewState::Playing(ref c)) if c == "Heat/line.wav"), "{previewing:?}");
    let before = lender.with(|c| c.current_set().tiles.len());
    b.press(Key::Enter);
    let after: Vec<(String, f64, Option<f64>)> = lender.with(|c| c.current_set().tiles.iter().map(|t| (t.title.clone(), t.start, t.end)).collect());
    assert_eq!(after.len(), before + 1, "Enter adds the clip");
    assert_eq!(after.last().unwrap(), &("Don't let yourself get attached".to_owned(), 0.5, Some(1.5)), "trimmed to the line");
    b.run("quip.preview", json!({"stop": true}));
}

/// The status bar's "N new in QuipClipper" (D-103), in both themes: a button with a name
/// and hover text that appears without taking the focus or saying anything, and opens the
/// window on the new clips.
#[test]
fn the_quip_badge_is_quiet_and_opens_the_new_clips() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        b.open_quip();
        b.run("view.quip", json!({"open": false}));
        assert!(b.harness.query_by_label("1 new in QuipClipper").is_none(), "{theme:?}: nothing new yet");
        b.run("view.select", json!({"tile": "#1"}));
        let focused = |b: &Board| b.harness.root().children_recursive().find(|n| n.accesskit_node().is_focused()).map(|n| n.accesskit_node().label());
        let (focus, said) = (focused(&b), b.announced());
        b.quip_new();
        let badge = b.harness.get_by_label("1 new in QuipClipper");
        assert_eq!(badge.accesskit_node().role(), Role::Button, "{theme:?}");
        assert_eq!(badge.accesskit_node().live(), egui::accesskit::Live::Off, "{theme:?}: not a live region");
        assert_eq!((focused(&b), b.announced()), (focus, said), "{theme:?}: the badge takes no focus and says nothing");
        let tip = theme::hint_log(&b.harness.ctx).0.contains_key(&egui::Id::unique("panel-quip-badge"));
        assert!(theme != ThemeKind::Panel || tip, "Panel's block has hover text");

        b.click("1 new in QuipClipper");
        let lender = b.harness.state().lender.clone();
        assert!(lender.with(|c| c.show_quip), "{theme:?}: the click opens the window");
        assert!(b.harness.query_by_label_contains("New since you last looked: 1").is_some(), "{theme:?}: on the new clips");
        assert!(b.harness.query_all_by_label_contains("Don't let yourself get attached").next().is_some());
        assert!(b.harness.query_by_label("1 new in QuipClipper").is_none(), "{theme:?}: seen now");
        let seen = lender.with(|c| crate::quip::seen_cursor(&c.store.data_dir, &crate::quip::list(&c.store.data_dir).unwrap()[0].0).unwrap());
        assert_eq!(seen.as_deref(), Some("v1.200"));
    }
}

/// Enter on a focused pad toggles it (retest A18, D-196): a second Enter stops it, even a
/// One-shot pad, whose hotkey still starts it again; a Pausable pad still pauses.
#[test]
fn enter_on_a_focused_pad_stops_what_it_started() {
    let mut b = Board::stepping(ThemeKind::Console, 0.02);
    let lender = b.harness.state().lender.clone();
    let id = |title: &str| lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.id)).unwrap();
    let state = |id: Uuid| lender.with(|c| c.engine.state(id));
    let tap = |b: &mut Board, key: Key| {
        b.key_down(key, true);
        b.key_down(key, false);
    };
    for (title, second) in [("Rimshot", PlayState::Stopped), ("Applause", PlayState::Stopped), ("Ambience Bed", PlayState::Paused)] {
        let pad = id(title);
        b.harness.state_mut().focus_script = Some(pad);
        b.harness.run_steps(3);
        assert_eq!(b.focused_title().as_deref(), Some(title));
        tap(&mut b, Key::Enter);
        assert_eq!(state(pad), PlayState::Playing, "{title}: Enter plays");
        tap(&mut b, Key::Enter);
        let after = state(pad);
        assert!(after == second || second == PlayState::Stopped && after == PlayState::Stopping, "{title}: the second Enter gives {after:?}");
        b.run("transport.stopAll", json!({"immediate": true}));
    }
    // Rimshot is One-shot: its key (3) still starts it again while it plays.
    let rimshot = id("Rimshot");
    tap(&mut b, Key::Num3);
    tap(&mut b, Key::Num3);
    assert_eq!(state(rimshot), PlayState::Playing, "a One-shot pad's key restarts it");
}

/// Edit… opens the waveform editor (t-m22-edit-opens-editor): the inspector's button after a
/// click on the pad, the tile menu's Edit… (greyed out for an action tile), ⌘/Ctrl+Shift+E,
/// and in a locked set too, read-only.
#[test]
fn edit_opens_the_editor() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::stepping(theme, 0.02);
        let lender = b.harness.state().lender.clone();
        let editor = |l: &Arc<Lender>| l.with(|c| c.editor);
        // Press and release in separate frames, as a mouse does.
        let click = |b: &mut Board, p: Pos2, button: egui::PointerButton| {
            b.harness.event(egui::Event::PointerMoved(p));
            b.harness.run_steps(2);
            for pressed in [true, false] {
                b.harness.event(egui::Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE });
                b.harness.run_steps(2);
            }
            b.harness.run_steps(15); // past the double-click time
        };
        let pad = |b: &Board, title: &str| {
            let id = lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == title).map(|t| t.id)).unwrap();
            (id, b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == id).expect("drawn").2.center())
        };
        let primary = egui::PointerButton::Primary;

        // A click on the pad, then the inspector's Edit…; Close Editor closes it.
        let (horn, at) = pad(&b, "Air Horn");
        click(&mut b, at, primary);
        assert_eq!(lender.with(|c| c.selected_tile), Some(horn), "{theme:?}: a click selects");
        let edit = b.harness.get_by_label("Edit waveform").rect().center();
        click(&mut b, edit, primary);
        assert_eq!(editor(&lender), Some(horn), "{theme:?}: the inspector's Edit… opens the editor");
        let close = b.harness.get_all_by_label("Close Editor").last().expect("Close Editor").rect().center();
        click(&mut b, close, primary);
        assert_eq!(editor(&lender), None, "{theme:?}: Close Editor closes it");

        // ⌘/Ctrl+Shift+E opens and closes it for the selected tile.
        b.press_all(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(editor(&lender), Some(horn), "{theme:?}: ⌘⇧E opens the editor");
        b.press_all(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(editor(&lender), None, "{theme:?}: ⌘⇧E closes it");

        // The tile menu's Edit… opens the editor on that tile.
        let (crickets, at) = pad(&b, "Crickets");
        click(&mut b, at, egui::PointerButton::Secondary);
        // The menu's Edit… comes after the inspector's in the tree.
        b.harness.get_all_by_label("Edit waveform").last().expect("the menu's Edit…").click();
        b.harness.run_steps(3);
        assert_eq!(editor(&lender), Some(crickets), "{theme:?}: the tile menu's Edit… opens the editor");
        b.run("view.editor", json!({"open": false}));
        // An action tile has no sound: its Edit… is greyed out and says why.
        let cue = lender.with(|c| c.current_set().tiles.iter().find(|t| t.action.is_some()).map(|t| t.title.clone())).unwrap();
        let (fade, at) = pad(&b, &cue);
        click(&mut b, at, egui::PointerButton::Secondary);
        let item = b.harness.get_all_by_label("Edit waveform").last().expect("the menu's Edit…");
        assert!(item.accesskit_node().is_disabled(), "{theme:?}: an action tile's Edit… is greyed out");
        assert!(item.accesskit_node().description().unwrap_or_default().contains("no sound"), "{theme:?}: and says why");
        b.press(Key::Escape);
        assert_ne!(editor(&lender), Some(fade));

        // A locked set's pad still opens the editor, read-only.
        b.run("set.lock", json!({}));
        let (_, at) = pad(&b, "Air Horn");
        click(&mut b, at, primary);
        let edit = b.harness.get_by_label("Edit waveform").rect().center();
        click(&mut b, edit, primary);
        assert_eq!(editor(&lender), Some(horn), "{theme:?}: a locked set's Edit… opens the editor");
        assert!(b.harness.query_by_label_contains("is locked: this editor is read-only").is_some(), "{theme:?}: read-only");
    }
}

/// A pad has one mark for selection and the keyboard focus in each look (D-215): in
/// Console the focus ring goes round a pad clicked with the mouse or moved to with an
/// arrow; in Panel neither draws the ring's rectangle, the pad draws its white outline.
#[test]
fn a_focused_pad_has_one_mark_in_each_look() {
    for theme in [ThemeKind::Console, ThemeKind::Panel] {
        let mut b = Board::new(theme);
        let lender = b.harness.state().lender.clone();
        let tile = |name: &str| lender.with(|c| c.current_set().tiles.iter().find(|t| t.title == name).map(|t| t.id)).unwrap();
        let applause = tile("Applause");
        let p = b.harness.state().tile_rects.iter().find(|(t, _, _)| *t == applause).expect("drawn").2.center();
        for pressed in [true, false] {
            b.harness.event(egui::Event::PointerMoved(p));
            b.harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            b.harness.run_steps(1);
        }
        b.harness.run_steps(10); // past the double-click time
        let ctx = b.harness.ctx.clone();
        let pad = |t| egui::Id::unique(("tile", t));
        assert_eq!(ctx.memory(|m| m.focused()), Some(pad(applause)), "{theme:?}: the click focuses the pad");
        let ring = || theme::focus_ring_drawn(&ctx);
        let ring_on = |t| if theme == ThemeKind::Console { Some(pad(t)) } else { None };
        assert_eq!(ring(), ring_on(applause), "{theme:?}: after a click");
        assert!(lender.with(|c| c.is_selected(applause)), "{theme:?}: and selects it");

        // Down from Applause (row 1) lands on the pad below it.
        b.press(Key::ArrowDown);
        let focused = ctx.memory(|m| m.focused());
        assert!(focused.is_some_and(|f| f != pad(applause)), "{theme:?}: the arrow moves the focus");
        let below = lender.with(|c| c.current_set().tiles.iter().map(|t| t.id).find(|t| Some(pad(*t)) == focused)).expect("a pad");
        assert_eq!(ring(), ring_on(below), "{theme:?}: after an arrow");

        // A cue row the same (t-m22-panel-outline-rows: Panel drew the ring round one).
        b.run("set.layout", json!({"layout": "list"}));
        b.run("view.select", json!({"tile": "#2"}));
        b.harness.state_mut().focus_script = Some(applause);
        b.harness.run_steps(3);
        let row = egui::Id::unique(("cue-row", applause));
        assert_eq!(ctx.memory(|m| m.focused()), Some(row), "{theme:?}: the cue row has the focus");
        assert_eq!(ring(), if theme == ThemeKind::Console { Some(row) } else { None }, "{theme:?}: a focused cue row");
        // With the window in the background too: egui's `has_focus` is false then, and
        // a Panel row drew the ring as well as its outline.
        b.harness.input_mut().focused = false;
        b.harness.run_steps(2);
        assert_eq!(ring(), if theme == ThemeKind::Console { Some(row) } else { None }, "{theme:?}: a focused cue row, window in the background");
        b.harness.input_mut().focused = true;
        b.harness.run_steps(1);

        // And a set in Panel's sets column: its own outline, not the ring.
        if theme == ThemeKind::Panel {
            let set = lender.with(|c| c.store.library.sets[0].id);
            let id = egui::Id::unique(("panel-set", set));
            ctx.memory_mut(|m| m.request_focus(id));
            b.harness.run_steps(3);
            assert_eq!(ctx.memory(|m| m.focused()), Some(id), "the set has the focus");
            assert_eq!(ring(), None, "a focused set draws its own outline");
        }
    }
}
