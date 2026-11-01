//! Every colour pair a theme draws, checked against WCAG 2.2 AA (M20, D-085). The test
//! holds the built-ins to it, and an imported theme is checked the same way: what falls
//! short comes back as warnings, never as a refusal (D-107).

use super::{contrast_ratio, lighten, luminance, over, pad_face, shade, Look, Tokens, PAD_BODY_HOVER_SHADE};
use crate::model::ThemeKind;
use eframe::egui::Color32;

/// What a pair must reach: text 4.5:1 (1.4.3), a control's edge or a focus indicator 3:1
/// against what's around it (1.4.11).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Need {
    Text,
    Control,
}

impl Need {
    pub fn min(self) -> f32 {
        match self {
            Need::Text => 4.5,
            Need::Control => 3.0,
        }
    }
}

/// A foreground on a background. A control passes when any of `fg` (its fill or its
/// outline) stands out enough.
pub struct Pair {
    pub name: String,
    pub need: Need,
    pub fg: Vec<Color32>,
    pub bg: Color32,
}

impl Pair {
    pub fn ratio(&self) -> f32 {
        self.fg.iter().map(|&f| contrast_ratio(f, self.bg)).fold(0.0, f32::max)
    }
}

fn pair(name: impl Into<String>, need: Need, fg: &[Color32], bg: Color32) -> Pair {
    Pair { name: name.into(), need, fg: fg.to_vec(), bg }
}

/// Every text-on-background and control-on-background pair the Console theme draws.
pub fn console_pairs(look: &Look, t: &Tokens, high_contrast: bool) -> Vec<Pair> {
    use Need::*;
    let mut out = Vec::new();
    for (bg_name, bg) in [("window", t.window), ("panel", t.panel), ("panel2", t.panel2), ("bar", t.bar), ("group", t.group)] {
        out.push(pair(format!("text on {bg_name}"), Text, &[t.text], bg));
        out.push(pair(format!("textDim on {bg_name}"), Text, &[t.text_dim], bg));
        out.push(pair(format!("accent text on {bg_name}"), Text, &[t.accent_text()], bg));
        out.push(pair(format!("key|keyBorder on {bg_name}"), Control, &[t.key, t.key_border], bg));
        out.push(pair(format!("focus on {bg_name}"), Control, &[t.focus], bg));
    }
    for (bg_name, bg) in [("window", t.window), ("panel", t.panel), ("panel2", t.panel2)] {
        out.push(pair(format!("accent mark on {bg_name}"), Control, &[t.accent], bg));
    }
    for (bg_name, bg) in [("key", t.key), ("keyHover", t.key_hover), ("selected", t.selected)] {
        out.push(pair(format!("text on {bg_name}"), Text, &[if bg == t.key { t.key_text() } else { t.text }], bg));
        out.push(pair(format!("focus on {bg_name}"), Control, &[t.focus], bg));
    }
    out.push(pair("white on accentDark (a key that's on)", Text, &[t.key_on_text()], t.accent_dark));
    out.push(pair("counterText on counterBg", Text, &[t.counter_text], t.counter_bg));
    out.push(pair("Stop All ink on stop", Text, &[look.ink.stop], t.stop));
    out.push(pair("GO ink on playing", Text, &[look.ink.go], t.playing));
    out.push(pair("accent outline (selected tile) on window", Control, &[t.accent], t.window));
    out.push(pair("counterText outline (playing tile) on window", Control, &[t.counter_text], t.window));
    out.push(pair("solo outline (paused tile) on window", Control, &[t.solo], t.window));
    for (name, c) in [("meterGreen", t.meter_green), ("meterYellow", t.meter_yellow), ("meterRed", t.meter_red)] {
        out.push(pair(format!("{name} on meterOff"), Control, &[c], t.meter_off));
    }
    // The input level's track is egui's extreme background (theme::style_for).
    out.extend(status_pairs(t, if luminance(t.panel) > 0.18 { lighten(t.panel, 0.6) } else { t.bar }));
    for (i, c) in super::TILE_ORDER.iter().enumerate() {
        let base = look.tiles[i];
        // The default pad (D-152): a dark body with the title in the tile's colour on it
        // (its plates over the waveform are the body, opaque); checked hovered, its lightest.
        let dark = pad_face(base, false, high_contrast, look.ink.tile, true, false);
        let body = dark.body;
        out.push(pair(format!("title on {c:?} pad"), Text, &[dark.title], body));
        out.push(pair(format!("key cap on {c:?} pad"), Text, &[dark.cap_ink], over(body, Color32::BLACK, super::PAD_CAP_ALPHA)));
        out.push(pair(format!("white time on playing {c:?} pad"), Text, &[Color32::WHITE], over(body, Color32::WHITE, 0.06)));
        out.push(pair(format!("playing mark on {c:?} pad"), Control, &[t.counter_text], body));
        out.push(pair(format!("paused mark on {c:?} pad"), Control, &[t.solo], body));
        out.push(pair(format!("{c:?} tile on window"), Control, &[base, shade(base, PAD_BODY_HOVER_SHADE)], t.window));
        // Coloured pads (D-146): the tile's colour all over, in the theme's tile ink.
        let colored = pad_face(base, true, high_contrast, look.ink.tile, false, false);
        let pad = colored.body;
        out.push(pair(format!("title on coloured {c:?} pad"), Text, &[colored.title], pad));
        out.push(pair(format!("badges on coloured {c:?} pad"), Text, &[over(pad, colored.title, if high_contrast { 1.0 } else { 0.6 })], pad));
        out.push(pair(format!("white key cap on coloured {c:?} pad"), Text, &[colored.cap_ink], over(pad, Color32::BLACK, super::tile_cap_alpha(high_contrast))));
    }
    out
}

/// The same for the Panel theme: black lettering on pastels, peach on black.
pub fn panel_pairs(look: &Look, t: &Tokens, high_contrast: bool) -> Vec<Pair> {
    use Need::*;
    let p = &look.panel;
    let ink = p.black;
    // Idle controls' lettering, and the outline high contrast gives them.
    let (lettering, outline) = if high_contrast { (Color32::WHITE, vec![shade(p.lilac, 0.42), p.lilac]) } else { (p.peach, vec![shade(p.lilac, 0.42)]) };
    let mut out = vec![
        pair("text (peach) on black", Text, &[t.text], p.black),
        pair("textDim on black", Text, &[t.text_dim], p.black),
        pair("palette text on black", Text, &[p.text], p.black),
        pair("palette dim on black", Text, &[p.dim], p.black),
        pair("lettering on idle control", Text, &[lettering], shade(p.lilac, 0.42)),
        pair("lettering on title bar", Text, &[lettering], shade(p.amber, 0.38)),
        pair("black on hovered control", Text, &[ink], p.peach),
        pair("black on active control", Text, &[ink], p.amber),
        pair("black on frame (amber)", Text, &[ink], p.amber),
        pair("black on frame (violet)", Text, &[ink], p.violet),
        pair("black on status cap (lilac)", Text, &[ink], p.lilac),
        // theme::key (Grid|List, Sidebar, a hotkey cap): lettered black on the pastels.
        pair("lettering on an unlit key", Text, &[t.key_text()], t.key),
        pair("lettering on a lit key", Text, &[t.key_on_text()], t.accent_dark),
        pair("idle control on black", Control, &outline, p.black),
        pair("selection outline on black", Control, &[super::PANEL_SELECT_RING], p.black),
        pair("focus on black", Control, &[t.focus], p.black),
        pair("meter segment on unlit", Control, &[p.gold], t.meter_off),
    ];
    out.extend(status_pairs(t, super::PANEL_FIELD));
    for (i, c) in look.sets.iter().enumerate() {
        out.push(pair(format!("black on set block {i}"), Text, &[ink], *c));
    }
    for (name, c) in [("peach", p.peach), ("lilac", p.lilac), ("periwinkle", p.periwinkle), ("tan", p.tan), ("sky", p.sky), ("red", p.red), ("gold", p.gold)] {
        out.push(pair(format!("black on {name} pill"), Text, &[ink], c));
    }
    for (i, c) in super::TILE_ORDER.iter().enumerate() {
        let base = look.panel_tiles[i];
        out.push(pair(format!("black on {c:?} tile"), Text, &[ink], base));
        out.push(pair(format!("lettering on empty {c:?} tile"), Text, &[look.ink.empty], shade(base, super::PANEL_EMPTY_SHADE)));
    }
    out
}

/// The status colours (D-120) where they're drawn, in either design: error text on
/// every surface, the trim handles, markers and vamp on a waveform's backdrop, a marker's
/// name, the file-drop frame on the dimmed window, and the input level on its track
/// (`field`).
fn status_pairs(t: &Tokens, field: Color32) -> Vec<Pair> {
    use Need::*;
    let mut out = Vec::new();
    for (bg_name, bg) in [("window", t.window), ("panel", t.panel), ("panel2", t.panel2), ("bar", t.bar)] {
        out.push(pair(format!("status/error text on {bg_name}"), Text, &[t.error], bg));
    }
    let wave = over(t.panel, Color32::BLACK, 90.0 / 255.0);
    for (name, c) in [("status/trim", t.trim), ("mark/marker", t.marker), ("mark/vamp", t.vamp)] {
        out.push(pair(format!("{name} on a waveform"), Control, &[c], wave));
    }
    out.push(pair("mark/marker text on panel", Text, &[t.marker], t.panel));
    out.push(pair("status/drop frame on the dimmed window", Control, &[t.drop], over(t.window, Color32::BLACK, 140.0 / 255.0)));
    for (name, c) in [("input/voice", t.input_voice), ("input/level", t.input_level)] {
        out.push(pair(format!("{name} on its track"), Control, &[c], field));
    }
    out
}

/// Pairs below WCAG AA in the built-in designed (not high-contrast) looks, left as
/// designed and recorded in D-086. The high-contrast appearance has none. A theme on the
/// same base isn't warned about these either (D-107).
pub const KNOWN_LOW: &[(&str, &str)] = &[
    // Flat keys: told apart by their labels, as on the desk the design copies.
    ("Dark", "key|keyBorder on "),
    ("Light", "key|keyBorder on "),
    // Coloured pads (D-146, now an option, D-152): black titles on the five darkest
    // SoundCraft colours (3.6 to 4.4:1), and the detail line at 60 % ink on every colour
    // (2.4 to 3.9:1). The default dark pads have none.
    ("Dark", "title on coloured Blue "),
    ("Dark", "title on coloured Crimson "),
    ("Dark", "title on coloured Indigo "),
    ("Dark", "title on coloured Pink "),
    ("Dark", "title on coloured Purple "),
    ("Dark", "badges on coloured "),
    ("Light", "title on coloured Blue "),
    ("Light", "title on coloured Crimson "),
    ("Light", "title on coloured Indigo "),
    ("Light", "title on coloured Pink "),
    ("Light", "title on coloured Purple "),
    ("Light", "badges on coloured "),
    // Light's outlines on its grey window: selected (with the check badge), playing
    // and paused (with ▶/⏸ and the words), and the lit red meter segment (with CLIP).
    ("Light", "accent mark on "),
    ("Light", "accent outline "),
    ("Light", "counterText outline "),
    ("Light", "solo outline "),
    ("Light", "meterRed on meterOff"),
    // Idle Panel controls are dim lilac on black; their peach lettering is 6:1.
    ("Panel", "idle control on black"),
    // The status colours as they were before themes could change them (D-120), first
    // measured then: Console's error red is 3.2 to 4.2:1 in Dark, and in Light the error
    // red, the waveform marks, the drop frame and the input level are 1.4 to 2.8:1.
    // Errors also say so in words (often with ⚠); high contrast and Panel pass.
    ("Dark", "status/error text on "),
    ("Light", "status/error text on "),
    ("Light", "status/trim on "),
    ("Light", "mark/marker "),
    ("Light", "mark/vamp on "),
    ("Light", "status/drop frame "),
    ("Light", "input/"),
];

/// A theme's looks by the names [`KNOWN_LOW`] uses, each with its pairs and whether it's
/// the high-contrast one. Console has Dark, Light and High Contrast; Panel has Panel and
/// High Contrast.
pub fn looks(look: &Look) -> Vec<(&'static str, Vec<Pair>, bool)> {
    match look.base {
        ThemeKind::Console => vec![
            ("Dark", console_pairs(look, &look.dark, false), false),
            ("Light", console_pairs(look, &look.light, false), false),
            ("High Contrast", console_pairs(look, &look.high_contrast, true), true),
        ],
        ThemeKind::Panel => vec![
            ("Panel", panel_pairs(look, &look.dark, false), false),
            ("High Contrast", panel_pairs(&look.panel_high_contrast(), &look.high_contrast, true), true),
        ],
    }
}

/// The pairs below their minimum, as "name 2.31:1 (needs 4.5:1)".
pub fn failures(pairs: &[Pair]) -> Vec<String> {
    pairs.iter().filter(|p| p.ratio() < p.need.min()).map(|p| format!("{} {:.2}:1 (needs {}:1)", p.name, p.ratio(), p.need.min())).collect()
}

/// Is this failure one the built-in look of that name already has?
pub fn is_known(look_name: &str, failure: &str) -> bool {
    KNOWN_LOW.iter().any(|(l, n)| *l == look_name && failure.starts_with(n))
}

/// What a theme should be warned about: every pair below AA that its base's built-in
/// doesn't already have, as "Light: text on window 3.10:1 (needs 4.5:1)".
pub fn warnings(look: &Look) -> Vec<String> {
    let mut out = Vec::new();
    for (name, pairs, high_contrast) in looks(look) {
        for f in failures(&pairs) {
            if high_contrast || !is_known(name, &f) {
                out.push(format!("{name}: {f}"));
            }
        }
    }
    out
}
