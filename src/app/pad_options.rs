//! The pad options (D-145): Loop, Solo, Pausable, One-shot, Hold and Disabled, and Mute
//! (D-251). Their icons are our own, drawn with the painter in whatever colour the theme
//! gives (D-146), so they need no font and look the same in Console and Panel. A pad's
//! face shows the badges for Mute, Loop, Hold, Pausable and One-shot, and one for an end
//! fade-out (D-168); the inspector has a row of toggle buttons, and Mute's speaker beside
//! the volume slider.

use crate::model::Tile;
use crate::theme::{self, Tokens};
use eframe::egui::accesskit::{Role as AxRole, Toggled};
use eframe::egui::{self, pos2, vec2, Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadOption {
    Loop,
    Solo,
    Pausable,
    OneShot,
    Hold,
    Disabled,
    /// Plays at zero gain (the inspector's speaker beside Volume, not the row).
    Muted,
}

impl PadOption {
    /// The inspector's row of big toggle buttons, in order; Disabled is a smaller one
    /// (or, three by three, the sixth: `option_row`).
    pub const ROW: [PadOption; 5] = [PadOption::Loop, PadOption::Solo, PadOption::Pausable, PadOption::OneShot, PadOption::Hold];

    /// The badges a pad's face can show, right to left from its corner: Mute nearest it,
    /// so a narrow pad keeps it longest.
    pub const BADGES: [PadOption; 5] = [PadOption::Muted, PadOption::Loop, PadOption::Hold, PadOption::Pausable, PadOption::OneShot];

    pub fn label(self) -> &'static str {
        match self {
            PadOption::Loop => "Loop",
            PadOption::Solo => "Solo",
            PadOption::Pausable => "Pausable",
            PadOption::OneShot => "One-shot",
            PadOption::Hold => "Hold",
            PadOption::Disabled => "Disabled",
            PadOption::Muted => "Mute",
        }
    }

    /// What it does, for the hover text.
    pub fn tip(self) -> &'static str {
        match self {
            PadOption::Loop => "Play it again from the start each time it ends, until stopped",
            PadOption::Solo => "Starting it fades out every other playing pad",
            PadOption::Pausable => "A press while it plays pauses it; the next press resumes (turns One-shot off)",
            PadOption::OneShot => "A press while it plays starts it again from the beginning (turns Pausable off)",
            PadOption::Hold => "It plays while the key, pad or mouse button is held, and stops when let go (with Pausable, pauses)",
            PadOption::Disabled => "Keys, pads and GO skip it; Preview still plays it",
            PadOption::Muted => "Silence it, keeping its volume: it still starts, keeps its time and cues, ducks and solos, but plays at no level",
        }
    }

    /// Its `tile.set` parameter.
    pub fn key(self) -> &'static str {
        match self {
            PadOption::Loop => "loop",
            PadOption::Solo => "solo",
            PadOption::Pausable => "pausable",
            PadOption::OneShot => "oneShot",
            PadOption::Hold => "hold",
            PadOption::Disabled => "disabled",
            PadOption::Muted => "muted",
        }
    }

    pub fn get(self, t: &Tile) -> bool {
        match self {
            PadOption::Loop => t.looping,
            PadOption::Solo => t.solo,
            PadOption::Pausable => t.press.pausable,
            PadOption::OneShot => t.press.one_shot,
            PadOption::Hold => t.press.hold,
            PadOption::Disabled => t.disabled,
            PadOption::Muted => t.muted,
        }
    }

    /// Turn it on or off; Pausable and One-shot turn each other off.
    pub fn set(self, t: &mut Tile, on: bool) {
        match self {
            PadOption::Loop => t.looping = on,
            PadOption::Solo => t.solo = on,
            PadOption::Pausable => t.press.set_pausable(on),
            PadOption::OneShot => t.press.set_one_shot(on),
            PadOption::Hold => t.press.hold = on,
            PadOption::Disabled => t.disabled = on,
            PadOption::Muted => t.muted = on,
        }
    }
}

/// An icon at a pad's top right: an option that's on, or the clip's end fade-out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Option(PadOption),
    FadeOut,
}

/// Whether the pad shows the fade badge (D-168): its clip fades out at its end. The
/// fade-in and the stop fade don't count, a loop never reaches its end on its own, and
/// an action tile has no clip.
pub fn fades_out(t: &Tile) -> bool {
    t.action.is_none() && !t.looping && t.fade_out > 0.0
}

/// The badges a pad's face shows, right to left from its corner: the options that are
/// on, then the fade.
pub fn badges(t: &Tile) -> Vec<Badge> {
    let mut out: Vec<Badge> = PadOption::BADGES.into_iter().filter(|o| o.get(t)).map(Badge::Option).collect();
    if fades_out(t) {
        out.push(Badge::FadeOut);
    }
    out
}

/// The [`badges`] that fit in `room` at `pitch` each: a narrow pad drops them from the
/// left, the fade first, rather than lose them all.
pub fn fit_badges(t: &Tile, room: f32, pitch: f32) -> Vec<Badge> {
    let mut out = badges(t);
    out.truncate((room / pitch).floor().max(0.0) as usize);
    out
}

/// How a tile plays, in words, for its hover card and screen readers: "muted, hold,
/// pausable, loops, solo, fades out". Empty for a plain pad (a press plays, a press while
/// playing stops).
pub fn option_words(t: &Tile) -> Vec<&'static str> {
    let mut words = Vec::new();
    if t.muted {
        words.push("muted");
    }
    if t.press.hold {
        words.push("hold");
    }
    if t.press.pausable {
        words.push("pausable");
    }
    if t.press.one_shot {
        words.push("one-shot");
    }
    if t.looping {
        words.push("loops");
    }
    if t.solo {
        words.push("solo");
    }
    if fades_out(t) {
        words.push("fades out");
    }
    words
}

/// Draw an option's icon in a `size` square around `c`.
pub fn paint_icon(p: &Painter, c: Pos2, size: f32, opt: PadOption, color: Color32) {
    let s = size;
    let w = (s * 0.11).max(1.4);
    let stroke = Stroke::new(w, color);
    match opt {
        PadOption::Loop => {
            // Most of a circle, clockwise, with an arrowhead where it ends.
            let r = s * 0.34;
            let (from, to) = (-0.18 * TAU, 0.62 * TAU);
            let at = |a: f32| c + r * vec2(a.cos(), a.sin());
            let points: Vec<Pos2> = (0..=24).map(|i| at(from + (to - from) * i as f32 / 24.0)).collect();
            p.add(Shape::line(points, stroke));
            let tip = at(to);
            let along = vec2(-to.sin(), to.cos());
            let out = vec2(to.cos(), to.sin());
            let head = s * 0.2;
            p.add(Shape::convex_polygon(vec![tip + along * head * 0.9, tip + out * head * 0.75, tip - out * head * 0.75], color, Stroke::NONE));
        }
        PadOption::Solo => {
            // A dot in a ring: this one alone.
            p.circle_stroke(c, s * 0.38, stroke);
            p.circle_filled(c, s * 0.15, color);
        }
        PadOption::Pausable => {
            let (bw, bh, gap) = (s * 0.17, s * 0.62, s * 0.14);
            for dx in [-(gap / 2.0 + bw / 2.0), gap / 2.0 + bw / 2.0] {
                p.rect_filled(Rect::from_center_size(c + vec2(dx, 0.0), vec2(bw, bh)), s * 0.03, color);
            }
        }
        PadOption::OneShot => {
            // A 1 in a ring.
            p.circle_stroke(c, s * 0.4, stroke);
            let x = c.x + s * 0.03;
            p.line_segment([pos2(x, c.y - s * 0.22), pos2(x, c.y + s * 0.22)], stroke);
            p.line_segment([pos2(x, c.y - s * 0.22), pos2(x - s * 0.12, c.y - s * 0.1)], stroke);
        }
        PadOption::Hold => {
            // A button held down: a short arrow pressing on its cap, low on its base.
            let head = s * 0.17;
            let tip = c.y - s * 0.06;
            p.line_segment([pos2(c.x, c.y - s * 0.42), pos2(c.x, tip - head * 0.8)], stroke);
            p.add(Shape::convex_polygon(vec![pos2(c.x - head, tip - head), pos2(c.x + head, tip - head), pos2(c.x, tip)], color, Stroke::NONE));
            let cap = Rect::from_min_max(pos2(c.x - s * 0.22, c.y + s * 0.06), pos2(c.x + s * 0.22, c.y + s * 0.22));
            p.rect_filled(cap, s * 0.04, color);
            let base = c.y + s * 0.38;
            p.line_segment([pos2(c.x - s * 0.42, base), pos2(c.x + s * 0.42, base)], stroke);
        }
        PadOption::Disabled => {
            let r = s * 0.36;
            p.circle_stroke(c, r, stroke);
            let d = r * std::f32::consts::FRAC_1_SQRT_2;
            p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
        }
        PadOption::Muted => paint_speaker(p, c, s, true, color),
    }
}

/// A loudspeaker in a `size` square around `c`, facing right: with `muted`, a cross where
/// its sound would be (the Mute badge and the inspector's toggle); without, two arcs.
pub fn paint_speaker(p: &Painter, c: Pos2, size: f32, muted: bool, color: Color32) {
    let s = size;
    let stroke = Stroke::new((s * 0.11).max(1.4), color);
    let left = c.x - s * 0.42;
    // The magnet, then the cone flaring out to the right.
    p.rect_filled(Rect::from_min_max(pos2(left, c.y - s * 0.13), pos2(left + s * 0.18, c.y + s * 0.13)), 0.0, color);
    let (neck, mouth) = (left + s * 0.16, left + s * 0.44);
    p.add(Shape::convex_polygon(
        vec![pos2(neck, c.y - s * 0.13), pos2(mouth, c.y - s * 0.36), pos2(mouth, c.y + s * 0.36), pos2(neck, c.y + s * 0.13)],
        color,
        Stroke::NONE,
    ));
    if muted {
        let (x, d) = (c.x + s * 0.24, s * 0.15);
        p.line_segment([pos2(x - d, c.y - d), pos2(x + d, c.y + d)], stroke);
        p.line_segment([pos2(x - d, c.y + d), pos2(x + d, c.y - d)], stroke);
    } else {
        for r in [s * 0.18, s * 0.34] {
            let arc: Vec<Pos2> = (0..=10).map(|i| {
                let a = (i as f32 / 10.0 - 0.5) * 0.5 * TAU * 0.55;
                pos2(mouth - s * 0.04 + r * a.cos(), c.y + r * a.sin())
            }).collect();
            p.add(Shape::line(arc, stroke));
        }
    }
}

/// The fade badge in a `size` square around `c`: a wedge under a gentle fade-out curve,
/// full height at the left, easing down to nothing at the right.
pub fn paint_fade_out(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let (half_w, half_h) = (size * 0.42, size * 0.36);
    let (left, top, bottom) = (c.x - half_w, c.y - half_h, c.y + half_h);
    // The gain 1 − x² is concave, so the area under it is convex.
    let mut points: Vec<Pos2> = (0..=12).map(|i| {
        let x = i as f32 / 12.0;
        pos2(left + 2.0 * half_w * x, top + 2.0 * half_h * x * x)
    }).collect();
    points.push(pos2(left, bottom));
    p.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A die showing five in a `size` square around `c`: the Random button (D-171).
pub fn paint_die(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size;
    let face = Rect::from_center_size(c, vec2(s * 0.84, s * 0.84));
    p.rect_stroke(face, s * 0.16, Stroke::new((s * 0.09).max(1.3), color), StrokeKind::Inside);
    let (d, r) = (s * 0.2, s * 0.075);
    for (dx, dy) in [(-d, -d), (d, -d), (0.0, 0.0), (-d, d), (d, d)] {
        p.circle_filled(c + vec2(dx, dy), r, color);
    }
}

/// A triangle pointing left or right in a `size` square around `c`: the bottom bar's
/// Setlist and Inspector arrows, which point the way a click moves the panel (D-216).
pub fn paint_arrow(p: &Painter, c: Pos2, size: f32, left: bool, color: Color32) {
    let (w, h) = (size * 0.42, size * 0.5);
    let dir = if left { -1.0 } else { 1.0 };
    let tip = c + vec2(dir * w / 2.0, 0.0);
    let back = c.x - dir * w / 2.0;
    p.add(Shape::convex_polygon(vec![tip, pos2(back, c.y - h), pos2(back, c.y + h)], color, Stroke::NONE));
}

/// Four squares, two by two, in a `size` square around `c`: the Grid button (D-216).
/// `square` gives Panel's sharp corners.
pub fn paint_grid_icon(p: &Painter, c: Pos2, size: f32, square: bool, color: Color32) {
    let (cell, gap) = (size * 0.42, size * 0.16);
    let corner = if square { 0.0 } else { size * 0.08 };
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let at = c + vec2(dx, dy) * (cell + gap) / 2.0;
        p.rect_filled(Rect::from_center_size(at, vec2(cell, cell)), corner, color);
    }
}

/// Three stacked bars, each with a dot before it, in a `size` square around `c`: the
/// List button (D-216). `square` gives Panel's sharp corners.
pub fn paint_list_icon(p: &Painter, c: Pos2, size: f32, square: bool, color: Color32) {
    let (bar, step) = (size * 0.18, size * 0.38);
    let (left, right) = (c.x - size / 2.0, c.x + size / 2.0);
    let corner = if square { 0.0 } else { bar / 2.0 };
    for i in [-1.0, 0.0, 1.0] {
        let y = c.y + i * step;
        p.rect_filled(Rect::from_min_max(pos2(left, y - bar / 2.0), pos2(left + bar * 1.4, y + bar / 2.0)), corner, color);
        p.rect_filled(Rect::from_min_max(pos2(left + bar * 2.4, y - bar / 2.0), pos2(right, y + bar / 2.0)), corner, color);
    }
}

/// A padlock in a `size` square around `c`, closed or open: the set lock (M22), drawn
/// like the option icons so it needs no font.
pub fn paint_lock(p: &Painter, c: Pos2, size: f32, closed: bool, color: Color32) {
    let s = size;
    let stroke = Stroke::new((s * 0.11).max(1.4), color);
    let body = Rect::from_min_max(pos2(c.x - s * 0.32, c.y - s * 0.02), pos2(c.x + s * 0.32, c.y + s * 0.42));
    p.rect_filled(body, s * 0.07, color);
    // The shackle: a half ring over the body, its right leg lifted out when open.
    let (r, top) = (s * 0.2, c.y - s * 0.22);
    let lift = if closed { 0.0 } else { s * 0.16 };
    let arc: Vec<Pos2> = (0..=16).map(|i| {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
        pos2(c.x + r * a.cos(), top - lift + r * a.sin())
    }).collect();
    p.add(Shape::line(arc, stroke));
    p.line_segment([pos2(c.x - r, top - lift), pos2(c.x - r, body.top())], stroke);
    p.line_segment([pos2(c.x + r, top - lift), pos2(c.x + r, if closed { body.top() } else { top - lift + s * 0.06 })], stroke);
}

/// A set open in a window of its own (D-224), in the Setlist: two overlapping window
/// outlines, the front one with a filled title bar, centred on `c`.
pub fn paint_window_mark(p: &Painter, c: Pos2, size: f32, color: Color32) {
    let s = size;
    let stroke = Stroke::new((s * 0.09).max(1.2), color);
    let back = Rect::from_min_size(pos2(c.x - s * 0.5, c.y - s * 0.45), vec2(s * 0.7, s * 0.6));
    let front = back.translate(vec2(s * 0.3, s * 0.3));
    p.rect_stroke(back, s * 0.06, stroke, StrokeKind::Middle);
    p.rect_stroke(front, s * 0.06, stroke, StrokeKind::Middle);
    p.rect_filled(Rect::from_min_size(front.min, vec2(front.width(), s * 0.2)), s * 0.06, color);
}

/// A pad's badges (from [`badges`] or [`fit_badges`]: Mute, Loop, Hold, Pausable, One-shot,
/// those that are on, then the fade), drawn leftward from `right_top` in `size` squares.
/// Returns the width they took.
pub fn paint_badges(p: &Painter, right_top: Pos2, size: f32, badges: &[Badge], color: Color32) -> f32 {
    let gap = (size * 0.2).round();
    let mut x = right_top.x;
    for &badge in badges {
        let c = pos2(x - size / 2.0, right_top.y + size / 2.0);
        match badge {
            Badge::Option(opt) => paint_icon(p, c, size, opt, color),
            Badge::FadeOut => paint_fade_out(p, c, size, color),
        }
        x -= size + gap;
    }
    if x == right_top.x { 0.0 } else { right_top.x - x - gap }
}

/// A Solo pad's border (D-146): a ring in the theme's `mark/solo` inside the pad's edge,
/// clear of the selection's outline, with a thin dark line inside it so it shows on a pad
/// of a similar colour.
pub fn paint_solo_ring(p: &Painter, rect: Rect, corners: egui::CornerRadius, color: Color32) {
    let inset = |c: egui::CornerRadius, by: f32| {
        let f = |v: u8| (v as f32 - by).max(0.0) as u8;
        egui::CornerRadius { nw: f(c.nw), ne: f(c.ne), sw: f(c.sw), se: f(c.se) }
    };
    p.rect_stroke(rect.shrink(3.0), inset(corners, 3.0), Stroke::new(2.5_f32, color), StrokeKind::Inside);
    p.rect_stroke(rect.shrink(5.5), inset(corners, 5.5), Stroke::new(1.0_f32, theme::with_alpha(Color32::BLACK, 0.55)), StrokeKind::Inside);
}

/// One option as a toggle button: its icon over its name, lit in the accent when on.
/// `on` is `None` when a multiple selection is mixed. `small` is the Disabled toggle: the
/// icon beside the name, one line high.
pub fn option_button(ui: &mut Ui, opt: PadOption, on: Option<bool>, width: f32, small: bool) -> egui::Response {
    let size = if small { vec2(width, theme::TARGET.max(26.0)) } else { vec2(width, BIG_H) };
    option_button_sized(ui, opt, on, size, small)
}

/// The big option buttons' height in a row of five; three by three they're a little
/// shorter, so the two rows take about the room of the row and Disabled (D-211).
const BIG_H: f32 = 52.0;
const GRID_H: f32 = 46.0;

fn option_button_sized(ui: &mut Ui, opt: PadOption, on: Option<bool>, size: Vec2, small: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let lit = on == Some(true);
    let fill = if lit {
        t.accent
    } else if resp.hovered() {
        t.key_hover
    } else {
        t.key
    };
    let ink = if lit { theme::ink_on(t.accent) } else { theme::ink_on(fill) };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    if on.is_none() {
        // Mixed: some of the selection have it.
        p.rect_stroke(rect, 6.0, Stroke::new(1.5_f32, t.accent), StrokeKind::Inside);
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    // Mixed shows in the accent edge, the hover text and the accessible state; a mark
    // after the name crowded a narrow button (t-m22-panel-outline-rows).
    let label = opt.label().to_owned();
    if small {
        let icon = 15.0;
        let g = p.layout_no_wrap(label.clone(), font, ink);
        let left = rect.center().x - (icon + 6.0 + g.size().x) / 2.0;
        paint_icon(p, pos2(left + icon / 2.0, rect.center().y), icon, opt, ink);
        p.galley(pos2(left + icon + 6.0, rect.center().y - g.size().y / 2.0), g, ink);
    } else {
        // A narrow button (the inspector at its default width, D-203) shrinks a label
        // that wouldn't fit, a little.
        let at = |size: f32, text: &str| p.layout_no_wrap(text.to_owned(), egui::FontId::new(size, font.family.clone()), ink);
        let mut size = font.size * 0.92;
        let room = rect.width() - 6.0;
        let w = at(size, &label).size().x;
        if w > room {
            size = (size * room / w).max(font.size * 0.75);
        }
        let g = at(size, &label);
        // The icon over the name, a gap between them, the two centred together by their
        // ink: the line's ascender to its descender, the same for every name, so a row's
        // labels share a baseline. A tall face (Panel's) leaves the icon less room; one
        // too short to read is left out (t-m22-panel-outline-rows).
        let line = theme::galley_ink(&at(size, "Hp"));
        let (pad, gap) = (5.0, 4.0);
        let icon = (rect.height() - 2.0 * pad - gap - line.height()).min(20.0);
        let icon = if icon >= 11.0 { icon } else { 0.0 };
        let top = rect.center().y - (icon + if icon > 0.0 { gap } else { 0.0 } + line.height()) / 2.0;
        if icon > 0.0 {
            paint_icon(p, pos2(rect.center().x, top + icon / 2.0), icon, opt, ink);
        }
        let ink_top = top + if icon > 0.0 { icon + gap } else { 0.0 };
        p.galley(pos2(rect.center().x - g.size().x / 2.0, ink_top - line.top()), g, ink);
    }
    let resp = theme::role(resp, AxRole::Button, opt.label(), on);
    if on.is_none() {
        resp.ctx.accesskit_node_builder(resp.id, |n| n.set_toggled(Toggled::Mixed));
    }
    let tip = if on.is_none() { format!("{} (some of these have it, some don't)", opt.tip()) } else { opt.tip().to_owned() };
    theme::hint(resp, tip)
}

/// The speaker beside the inspector's volume slider (D-251): Mute, a square key that turns
/// the error red with a cross while muted. `on` is `None` when a selection is mixed.
pub fn mute_toggle(ui: &mut Ui, on: Option<bool>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let side = theme::TARGET.max(26.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let muted = on == Some(true);
    let fill = if resp.hovered() { t.key_hover } else { t.key };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    if muted || on.is_none() {
        p.rect_stroke(rect, 6.0, Stroke::new(1.5_f32, if muted { t.error } else { t.accent }), StrokeKind::Inside);
    }
    let ink = if muted { t.error } else { theme::ink_on(fill) };
    paint_speaker(p, rect.center(), side * 0.62, muted, ink);
    let resp = theme::role(resp, AxRole::Button, PadOption::Muted.label(), on);
    if on.is_none() {
        resp.ctx.accesskit_node_builder(resp.id, |n| n.set_toggled(Toggled::Mixed));
    }
    let tip = match on {
        None => format!("{} (some of these are muted, some aren't)", PadOption::Muted.tip()),
        Some(true) => "Muted: it plays at no level. Click to hear it again at its volume".to_owned(),
        Some(false) => PadOption::Muted.tip().to_owned(),
    };
    theme::hint(resp, tip)
}

/// The inspector's options: the big toggle buttons in a row, then a smaller Disabled; in
/// a column too narrow for five (the default width, D-211), three by three with Disabled
/// as the sixth big button. `state` gives each option's value (`None` when a selection
/// is mixed); returns the option clicked, if any.
pub fn option_row(ui: &mut Ui, state: impl Fn(PadOption) -> Option<bool>) -> Option<PadOption> {
    let mut clicked = None;
    let gap = 6.0;
    let avail = ui.available_width();
    // Five across when each gets at least 50 px (the labels shrink a little to fit).
    let five = (avail - 4.0 * gap) / 5.0 >= 50.0;
    let per_row = if five { 5 } else { 3 };
    let w = ((avail - (per_row as f32 - 1.0) * gap) / per_row as f32).floor();
    let big: Vec<PadOption> = if five { PadOption::ROW.to_vec() } else { PadOption::ROW.into_iter().chain([PadOption::Disabled]).collect() };
    for chunk in big.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let h = if five { BIG_H } else { GRID_H };
            for &opt in chunk {
                if option_button_sized(ui, opt, state(opt), vec2(w, h), false).clicked() {
                    clicked = Some(opt);
                }
            }
        });
    }
    if five {
        ui.horizontal(|ui| {
            if option_button(ui, PadOption::Disabled, state(PadOption::Disabled), (w * 1.6).min(avail), true).clicked() {
                clicked = Some(PadOption::Disabled);
            }
        });
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PressOptions;

    #[test]
    fn option_words_and_badges_follow_the_tile() {
        let mut t = Tile::default();
        assert!(option_words(&t).is_empty(), "a plain pad: nothing to say");
        PadOption::Hold.set(&mut t, true);
        PadOption::Pausable.set(&mut t, true);
        PadOption::Loop.set(&mut t, true);
        assert_eq!(option_words(&t), ["hold", "pausable", "loops"]);
        PadOption::OneShot.set(&mut t, true);
        assert_eq!(t.press, PressOptions { hold: true, pausable: false, one_shot: true }, "One-shot turns Pausable off");
        let on: Vec<_> = PadOption::BADGES.into_iter().filter(|o| o.get(&t)).collect();
        assert_eq!(on, [PadOption::Loop, PadOption::Hold, PadOption::OneShot]);
        PadOption::Muted.set(&mut t, true);
        assert!(t.muted && t.volume == 1.0, "Mute keeps the volume");
        assert_eq!(option_words(&t), ["muted", "hold", "one-shot", "loops"]);
        assert_eq!(badges(&t)[0], Badge::Option(PadOption::Muted), "Mute sits nearest the corner");
        assert_eq!(fit_badges(&t, 20.0, 17.0), [Badge::Option(PadOption::Muted)], "and is the last to go");
    }

    #[test]
    fn only_an_end_fade_out_shows_the_fade_badge() {
        let mut t = Tile { file: Some("a.wav".into()), ..Tile::default() };
        assert!(badges(&t).is_empty());
        t.fade_in = 1.0;
        t.stop_fade = 2.0;
        assert!(badges(&t).is_empty() && !option_words(&t).contains(&"fades out"), "the fade-in and the stop fade don't show it (D-168)");
        t.fade_out = 0.5;
        assert_eq!(badges(&t), [Badge::FadeOut]);
        assert_eq!(option_words(&t), ["fades out"]);
        PadOption::Hold.set(&mut t, true);
        PadOption::Pausable.set(&mut t, true);
        assert_eq!(badges(&t), [Badge::Option(PadOption::Hold), Badge::Option(PadOption::Pausable), Badge::FadeOut], "the fade goes left of the options");
        assert_eq!(option_words(&t), ["hold", "pausable", "fades out"]);
        assert_eq!(fit_badges(&t, 40.0, 17.0), [Badge::Option(PadOption::Hold), Badge::Option(PadOption::Pausable)], "a narrow pad drops the fade first");
        assert!(fit_badges(&t, 10.0, 17.0).is_empty());
        PadOption::Loop.set(&mut t, true);
        assert!(!fades_out(&t), "a loop never fades out on its own");
        PadOption::Loop.set(&mut t, false);
        t.action = Some(crate::model::TileAction::default());
        assert!(!fades_out(&t), "an action tile has no clip to fade");
    }
}
