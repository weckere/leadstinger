//! The macOS title bar inside the window (t-m22-mac-titlebar, D-228): the window's
//! content runs under a transparent title bar with its title hidden, so the traffic
//! lights float over the app's own top strip (the toolbar, or a set window's bar) and no
//! row is spent on an empty title bar. That strip leaves the lights room at its left,
//! moves the window when its empty parts are dragged, and zooms it (or whatever System
//! Settings says) on a double-click, as a title bar does. The lights themselves are
//! AppKit's own buttons, moved (t-m22-titlebar-lights, D-231, D-232): centred on
//! Console's strip, and in Panel hidden but for a black pill in the top-left corner
//! while the pointer is there.
//!
//! Linux keeps its window as it is (Hyprland draws no decorations, other desktops their
//! own title bar), and so do the tests' harnesses: nothing here changes them.

use eframe::egui::{self, Rect, Ui, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this run's windows have the integrated title bar ([`enable`]): never in the
/// tests' harnesses, which keep their layout.
static INTEGRATED: AtomicBool = AtomicBool::new(false);

/// Give this run's windows the integrated title bar, on macOS (the app does, at launch);
/// elsewhere, nothing.
pub fn enable() {
    INTEGRATED.store(cfg!(target_os = "macos"), Ordering::Relaxed);
}

/// A window's builder with the title bar inside the window once [`enable`]d; else as it was.
pub fn integrate(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if !INTEGRATED.load(Ordering::Relaxed) {
        return builder;
    }
    builder.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false).with_titlebar_buttons_shown(true)
}

/// Whether the window `ctx` is drawing has the traffic lights over its content now: an
/// integrated window that isn't full screen (where the title bar slides in over the top
/// only while the pointer is there, as for every app).
pub fn active(ctx: &egui::Context) -> bool {
    INTEGRATED.load(Ordering::Relaxed) && !fullscreen(ctx)
}

fn fullscreen(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
}

/// The room the traffic lights take at the window's top left, in `ctx`'s points (they
/// keep their size whatever the zoom): their right edge (Panel: their pill's) plus a gap,
/// and the title bar's height. Zero where [`active`] is false.
pub fn inset(ctx: &egui::Context) -> Vec2 {
    if !active(ctx) {
        return Vec2::ZERO;
    }
    let g = geometry();
    let right = if crate::theme::is_panel(ctx) { pill(&g, PILL_AT).max.x } else { g.x[2] + g.size.x };
    Vec2::new(right + GAP, g.bar) / ctx.zoom_factor()
}

/// Between the last light and the strip's first control, in screen points.
const GAP: f32 = 12.0;

/// Make `rect` (a window's top strip, the whole of it) its title bar: call it before
/// drawing the strip's controls, which then take the clicks over it, so only its empty
/// parts drag. Dragging moves the window and a double-click does what a title bar's does.
/// It also places the traffic lights for the window's theme (D-231, D-232): Console
/// centres them on the strip; Panel keeps them hidden, and shows them
/// in a black pill of their own while the pointer is in the window's top-left corner.
/// Nothing where the window has no integrated title bar; in full screen the lights are
/// left to macOS.
pub fn drag_area(ui: &Ui, rect: Rect) {
    title_bar(ui, rect, None);
}

/// [`drag_area`] for a Panel frame that wants the lights' pill with its top left at
/// `pill` (in the window's points) rather than in the very corner.
pub fn drag_area_pill_at(ui: &Ui, rect: Rect, pill: egui::Pos2) {
    title_bar(ui, rect, Some(pill));
}

/// The lights' pill's size in `ctx`'s points (the lights keep their size whatever the zoom).
pub fn pill_size(ctx: &egui::Context) -> Vec2 {
    pill(&geometry(), Vec2::ZERO).size() / ctx.zoom_factor()
}

fn title_bar(ui: &Ui, rect: Rect, pill_at: Option<egui::Pos2>) {
    let ctx = ui.ctx();
    if !INTEGRATED.load(Ordering::Relaxed) {
        return;
    }
    if fullscreen(ctx) {
        arrange(ctx, Arrangement::System);
        return;
    }
    let id = ui.scope_id().with("title-bar-drag");
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
    // A mouse affordance over the toolbar, which a screen reader already names: hidden
    // from the accessibility tree, which moves windows its own way.
    ctx.accesskit_node_builder(id, |n| n.set_hidden());
    if resp.double_clicked_by(egui::PointerButton::Primary) {
        double_click(ctx);
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if crate::theme::is_panel(ctx) {
        corner_lights(ctx, pill_at.map_or(PILL_AT, |p| (p.to_vec2() * ctx.zoom_factor()).round()));
    } else {
        let centre = rect.center().y.max(0.0) * ctx.zoom_factor();
        arrange(ctx, Arrangement::Centred { centre });
    }
}

/// Panel's lights (D-232): hidden, and shown in their pill (its top left `at`, in screen
/// points) while the pointer is in the window's top-left corner, up to [`HOVER_REACH`]
/// past the pill (or always, with `LEADSTINGER_LIGHTS_SHOWN=1`, for screenshots).
fn corner_lights(ctx: &egui::Context, at: Vec2) {
    let g = geometry();
    let shown = forced_shown() || pointer_in(ctx, hover_zone(&g, at));
    arrange(ctx, Arrangement::Pill { shown, at });
    if !shown {
        return;
    }
    let zoom = ctx.zoom_factor();
    let r = pill(&g, at);
    let r = Rect::from_min_max((r.min.to_vec2() / zoom).to_pos2(), (r.max.to_vec2() / zoom).to_pos2());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("title-bar-lights-pill")));
    painter.rect_filled(r, r.height() / 2.0, egui::Color32::BLACK);
    // The lights take the pointer's moves while it's over them; look again shortly so
    // the pill leaves with the pointer whatever the window was told.
    ctx.request_repaint_after(std::time::Duration::from_millis(150));
}

fn forced_shown() -> bool {
    static FORCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| std::env::var("LEADSTINGER_LIGHTS_SHOWN").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// How far past the pill, right and down, Panel's hover zone reaches from the window's
/// top-left corner, in screen points (D-232).
pub const HOVER_REACH: f32 = 14.0;

/// The pill's top left where the window doesn't say (a set window's bar), in screen
/// points from the window's, and the room round the lights inside it (D-232).
const PILL_AT: Vec2 = Vec2::new(6.0, 6.0);
const PILL_PAD: f32 = 8.0;

/// Panel's pill in screen points from the window's top left: the three lights with
/// [`PILL_PAD`] round them, its top left `at`.
fn pill(g: &Geometry, at: Vec2) -> Rect {
    let size = Vec2::new(g.x[2] + g.size.x - g.x[0], g.size.y) + Vec2::splat(2.0 * PILL_PAD);
    Rect::from_min_size(at.to_pos2(), size)
}

/// Panel's hover zone: from the window's top-left corner to [`HOVER_REACH`] past the pill.
fn hover_zone(g: &Geometry, at: Vec2) -> Rect {
    Rect::from_min_max(egui::Pos2::ZERO, pill(g, at).max + Vec2::splat(HOVER_REACH))
}

/// The traffic lights' arrangement in a window, in screen points from its top left.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Arrangement {
    /// Where macOS puts them (full screen): shown, left alone.
    System,
    /// Their left edges where macOS has them, their centres at `centre` down.
    Centred { centre: f32 },
    /// In Panel's pill (its top left `at`), or hidden.
    Pill { shown: bool, at: Vec2 },
}

/// Where the three lights go: their left edges, and their centres' depth from the
/// window's top; `None` for hidden or left alone. Only macOS moves the buttons; the
/// tests check the sums everywhere.
#[cfg(any(target_os = "macos", test))]
fn places(g: &Geometry, a: Arrangement) -> Option<([f32; 3], f32)> {
    match a {
        Arrangement::System | Arrangement::Pill { shown: false, .. } => None,
        Arrangement::Centred { centre } => Some((g.x, centre.max(g.size.y / 2.0))),
        Arrangement::Pill { shown: true, at } => {
            let r = pill(g, at);
            let left = r.min.x + PILL_PAD;
            Some((g.x.map(|x| left + x - g.x[0]), r.center().y))
        }
    }
}

/// What a double-click on a title bar does, as System Settings ▸ Desktop & Dock says
/// (`AppleActionOnDoubleClick`): zoom (the default, and for "Fill"), minimize, or nothing.
fn double_click(ctx: &egui::Context) {
    match double_click_action().as_deref() {
        Some("Minimize") => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        Some("None") => {}
        _ => {
            let zoomed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!zoomed));
        }
    }
}

#[cfg(target_os = "macos")]
fn double_click_action() -> Option<String> {
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    defaults.stringForKey(objc2_foundation::ns_string!("AppleActionOnDoubleClick")).map(|s| s.to_string())
}

#[cfg(not(target_os = "macos"))]
fn double_click_action() -> Option<String> {
    None
}

/// The traffic lights as macOS lays them out, in screen points: each one's left edge from
/// the window's, their size, and the title bar's height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    x: [f32; 3],
    size: Vec2,
    bar: f32,
}

/// The long-standing layout, for when there's nothing to measure.
const USUAL: Geometry = Geometry { x: [7.0, 27.0, 47.0], size: Vec2::new(14.0, 16.0), bar: 28.0 };

/// The lights measured once from the first window that has them (they differ between
/// macOS versions), before anything moves them; else [`USUAL`].
fn geometry() -> Geometry {
    static MEASURED: std::sync::OnceLock<Geometry> = std::sync::OnceLock::new();
    if let Some(m) = MEASURED.get() {
        return *m;
    }
    match measure() {
        Some(m) => *MEASURED.get_or_init(|| m),
        None => USUAL,
    }
}

#[cfg(target_os = "macos")]
fn measure() -> Option<Geometry> {
    use objc2_app_kit::{NSApplication, NSWindowButton};
    let mtm = objc2::MainThreadMarker::new()?;
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        let kinds = [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton];
        let Some(rects) = kinds.iter().map(|k| window.standardWindowButton(*k).map(|b| b.convertRect_toView(b.bounds(), None))).collect::<Option<Vec<_>>>() else {
            continue;
        };
        // In the window's own coordinates, which count up from its bottom left.
        let title_bar = window.frame().size.height - window.contentLayoutRect().size.height;
        if rects[2].size.width <= 0.0 || title_bar <= 0.0 {
            continue;
        }
        let x = [rects[0].origin.x as f32, rects[1].origin.x as f32, rects[2].origin.x as f32];
        return Some(Geometry { x, size: Vec2::new(rects[2].size.width as f32, rects[2].size.height as f32), bar: title_bar as f32 });
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn measure() -> Option<Geometry> {
    None
}

/// Whether the pointer is in `zone` (screen points from the top left of the window `ctx`
/// draws). On macOS it asks the window itself, since the lights, once shown, take the
/// pointer's moves.
fn pointer_in(ctx: &egui::Context, zone: Rect) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(pos) = mac::with_window(ctx, |w| {
        let p = w.mouseLocationOutsideOfEventStream();
        egui::pos2(p.x as f32, (w.frame().size.height - p.y) as f32)
    }) {
        return zone.contains(pos);
    }
    ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| zone.contains(p * ctx.zoom_factor()))
}

/// Put the window's traffic lights where `a` says. AppKit lays them out again whenever
/// the window resizes or changes, so each frame puts them back, touching them only when
/// they've moved.
fn arrange(ctx: &egui::Context, a: Arrangement) {
    #[cfg(target_os = "macos")]
    mac::arrange(ctx, a, &geometry());
    #[cfg(not(target_os = "macos"))]
    let _ = (ctx, a);
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{places, Arrangement, Geometry};
    use eframe::egui;
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowButton};
    use objc2_foundation::NSPoint;

    /// Run `f` on the window `ctx` draws, found by its title (the main window's and each
    /// set window's are their own).
    pub fn with_window<R>(ctx: &egui::Context, f: impl FnOnce(&NSWindow) -> R) -> Option<R> {
        let title = ctx.input(|i| i.viewport().title.clone())?;
        let mtm = objc2::MainThreadMarker::new()?;
        let windows = NSApplication::sharedApplication(mtm).windows();
        let window = windows.iter().find(|w| w.title().to_string() == title)?;
        Some(f(&window))
    }

    pub fn arrange(ctx: &egui::Context, a: Arrangement, g: &Geometry) {
        with_window(ctx, |window| {
            let kinds = [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton];
            let Some(buttons) = kinds.iter().map(|k| window.standardWindowButton(*k)).collect::<Option<Vec<_>>>() else { return };
            let Some((xs, centre)) = places(g, a) else {
                let hide = a != Arrangement::System;
                for b in &buttons {
                    if b.isHidden() != hide {
                        b.setHidden(hide);
                    }
                }
                return;
            };
            // The lights' container (the title bar's view) is as tall as the title bar:
            // make it reach twice their centre's depth, so they sit in its middle and
            // nothing clips them. It stays at the window's top.
            // SAFETY: AppKit's own views, read on the main thread.
            let Some(bar) = (unsafe { buttons[0].superview().and_then(|v| v.superview()) }) else { return };
            let top = window.frame().size.height;
            let height = (2.0 * centre as f64).max(g.bar as f64);
            let mut frame = bar.frame();
            if (frame.size.height - height).abs() > 0.25 || (frame.origin.y - (top - height)).abs() > 0.25 {
                frame.size.height = height;
                frame.origin.y = top - height;
                bar.setFrame(frame);
            }
            for (b, x) in buttons.iter().zip(xs) {
                let size = b.frame().size;
                let at = NSPoint::new(x as f64, (height - size.height) / 2.0);
                let now = b.frame().origin;
                if (now.x - at.x).abs() > 0.25 || (now.y - at.y).abs() > 0.25 {
                    b.setFrameOrigin(at);
                }
                if b.isHidden() {
                    b.setHidden(false);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_without_the_integrated_title_bar_leaves_no_room() {
        // The tests' harness never builds a native window, so its layout is unchanged.
        let ctx = egui::Context::default();
        assert_eq!(inset(&ctx), Vec2::ZERO);
        assert!(!active(&ctx));
    }

    #[test]
    fn console_centres_the_lights_on_its_strip_where_macos_has_them_across() {
        // A 64 pt toolbar: the lights' centres 32 pt down, their left edges as measured.
        assert_eq!(places(&USUAL, Arrangement::Centred { centre: 32.0 }), Some(([7.0, 27.0, 47.0], 32.0)));
        // Never higher than the window's top lets them sit.
        assert_eq!(places(&USUAL, Arrangement::Centred { centre: 2.0 }), Some(([7.0, 27.0, 47.0], 8.0)));
        assert_eq!(places(&USUAL, Arrangement::System), None);
    }

    #[test]
    fn panel_keeps_the_lights_in_a_pill_in_its_hover_corner() {
        let r = pill(&USUAL, PILL_AT);
        // The three lights (54 pt across, 16 high) with 8 pt round them.
        assert_eq!(r, Rect::from_min_size(egui::pos2(6.0, 6.0), Vec2::new(70.0, 32.0)));
        let at = Vec2::new(30.0, 16.0);
        assert_eq!(places(&USUAL, Arrangement::Pill { shown: true, at }), Some(([38.0, 58.0, 78.0], 32.0)));
        assert_eq!(places(&USUAL, Arrangement::Pill { shown: false, at }), None);
        // The zone that shows it runs from the window's corner to 14 pt past the pill.
        let zone = hover_zone(&USUAL, at);
        assert_eq!(zone, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(114.0, 62.0)));
        assert!(zone.contains(pill(&USUAL, at).max) && !zone.contains(egui::pos2(120.0, 10.0)));
    }
}
