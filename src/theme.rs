//! Look and feel: design tokens, fonts and small painting helpers.
//!
//! The palette and widget shapes follow SoundCraft, the audio app of the storytold
//! "Craft" family (MIT OR Apache-2.0; see ATTRIBUTION.md). The values are defined once
//! as FigCraft variables (docs/design/mockup.py) and exported to
//! docs/design/tokens.json; a test keeps the constants below equal to that file.

use crate::model::{Appearance, ThemeKind, TileColor};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2,
    Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Theme, ThemePreference, Ui,
};
use std::sync::Arc;

/// Every colour the UI uses. Names match the FigCraft variables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub window: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub bar: Color32,
    pub group: Color32,
    pub border: Color32,
    pub border_light: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_dark: Color32,
    pub key: Color32,
    pub key_hover: Color32,
    pub key_border: Color32,
    pub selected: Color32,
    pub counter_bg: Color32,
    pub counter_text: Color32,
    pub playhead: Color32,
    pub meter_green: Color32,
    pub meter_yellow: Color32,
    pub meter_red: Color32,
    pub meter_off: Color32,
    pub stop: Color32,
    pub playing: Color32,
    pub solo: Color32,
}

/// Status colours that read well in both themes.
pub const ERROR: Color32 = DARK.meter_red;
pub const ACCENT: Color32 = DARK.accent;
pub const TRIM: Color32 = DARK.solo;

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const DARK: Tokens = Tokens {
    window: hex(0x1E1E1E),
    panel: hex(0x252526),
    panel2: hex(0x2C2C2D),
    bar: hex(0x161617),
    group: hex(0x181819),
    border: hex(0x0E0E0E),
    border_light: hex(0x404042),
    text: hex(0xD6D6D6),
    text_dim: hex(0x969698),
    accent: hex(0x4084C4),
    accent_dark: hex(0x285C92),
    key: hex(0x3A3A3C),
    key_hover: hex(0x4E4E50),
    key_border: hex(0x141414),
    selected: hex(0x344660),
    counter_bg: hex(0x040404),
    counter_text: hex(0x68DC78),
    playhead: hex(0xE83C32),
    meter_green: hex(0x3CC850),
    meter_yellow: hex(0xE6D23C),
    meter_red: hex(0xE63228),
    meter_off: hex(0x080808),
    stop: hex(0x6E1E1E),
    playing: hex(0x285A32),
    solo: hex(0xDEC434),
};

pub const LIGHT: Tokens = Tokens {
    window: hex(0xB8BABE),
    panel: hex(0xCCCED2),
    panel2: hex(0xC2C4C8),
    bar: hex(0xAEB0B4),
    group: hex(0xC4C6CA),
    border: hex(0x8E9094),
    border_light: hex(0x9EA0A4),
    text: hex(0x18191C),
    text_dim: hex(0x4A4C50),
    accent: hex(0x3E74B4),
    key: hex(0xDADCE0),
    key_hover: hex(0xE6E8EC),
    key_border: hex(0x8E9094),
    selected: hex(0xA8BCD8),
    meter_off: hex(0x34363A),
    meter_green: hex(0x24A03A),
    meter_yellow: hex(0xBEA214),
    meter_red: hex(0xC32A23),
    ..DARK
};

/// The Panel theme's palette: pastels on black. Names match the FigCraft
/// variables in docs/design/panel-tokens.json (made by docs/design/panel.py).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelPalette {
    pub black: Color32,
    pub amber: Color32,
    pub peach: Color32,
    pub gold: Color32,
    pub lilac: Color32,
    pub violet: Color32,
    pub periwinkle: Color32,
    pub sky: Color32,
    pub blue: Color32,
    pub salmon: Color32,
    pub red: Color32,
    pub tan: Color32,
    pub text: Color32,
    pub dim: Color32,
}

pub const PANEL: PanelPalette = PanelPalette {
    black: hex(0x000000),
    amber: hex(0xFF9C3A),
    peach: hex(0xFFC999),
    gold: hex(0xFFCC66),
    lilac: hex(0xCC99CC),
    violet: hex(0x9977CC),
    periwinkle: hex(0x9999FF),
    sky: hex(0x99CCFF),
    blue: hex(0x5588DD),
    salmon: hex(0xE2786A),
    red: hex(0xDD4444),
    tan: hex(0xCC9966),
    text: hex(0xFFC999),
    dim: hex(0xB07A4E),
};

/// The generic tokens when the Panel theme is on, so dialogs, the inspector and
/// anything not drawn by the Panel painters still match it.
pub const PANEL_TOKENS: Tokens = Tokens {
    window: PANEL.black,
    panel: PANEL.black,
    panel2: PANEL.black,
    bar: PANEL.black,
    group: PANEL.black,
    border: hex(0x3A2A18),
    border_light: PANEL.dim,
    text: PANEL.peach,
    text_dim: PANEL.dim,
    accent: PANEL.amber,
    accent_dark: PANEL.amber,
    key: PANEL.lilac,
    key_hover: PANEL.peach,
    key_border: PANEL.black,
    selected: hex(0x3A2A18),
    counter_bg: PANEL.black,
    counter_text: PANEL.peach,
    playhead: PANEL.red,
    meter_green: PANEL.gold,
    meter_yellow: PANEL.amber,
    meter_red: PANEL.red,
    meter_off: hex(0x2A1A0E),
    stop: PANEL.red,
    playing: PANEL.gold,
    solo: PANEL.gold,
};

/// Tile colours in the Panel theme, one pastel hue per TileColor.
pub fn panel_tile_fill(c: TileColor) -> Color32 {
    match c {
        TileColor::Red => PANEL.red,
        TileColor::Orange => PANEL.amber,
        TileColor::Yellow => PANEL.gold,
        TileColor::Lime => hex(0xCCCC66),
        TileColor::Green => hex(0x99CC99),
        TileColor::Mint => hex(0x88BBAA),
        TileColor::Teal => hex(0x66AAAA),
        TileColor::Cyan => PANEL.sky,
        TileColor::Blue => PANEL.blue,
        TileColor::Indigo => PANEL.periwinkle,
        TileColor::Purple => PANEL.violet,
        TileColor::Pink => PANEL.lilac,
        TileColor::Rose => hex(0xCC6699),
        TileColor::Crimson => PANEL.salmon,
        TileColor::Brown => PANEL.tan,
        TileColor::Gray => hex(0x9999AA),
    }
}

fn theme_kind_id() -> egui::Id {
    egui::Id::new("leadstinger-theme-kind")
}

/// Which design is showing (set by [`apply_look`]).
pub fn kind(ctx: &egui::Context) -> ThemeKind {
    ctx.data(|d| d.get_temp::<ThemeKind>(theme_kind_id())).unwrap_or_default()
}

pub fn is_panel(ctx: &egui::Context) -> bool {
    kind(ctx) == ThemeKind::Panel
}

impl Tokens {
    /// The tokens for the design and theme egui is currently showing.
    pub fn get(ctx: &egui::Context) -> &'static Tokens {
        if is_panel(ctx) {
            &PANEL_TOKENS
        } else if ctx.style().visuals.dark_mode {
            &DARK
        } else {
            &LIGHT
        }
    }

    /// (FigCraft variable name, value) pairs, for the consistency test.
    #[cfg(test)]
    fn named(&self) -> Vec<(&'static str, Color32)> {
        vec![
            ("window", self.window),
            ("panel", self.panel),
            ("panel2", self.panel2),
            ("bar", self.bar),
            ("group", self.group),
            ("border", self.border),
            ("borderLight", self.border_light),
            ("text", self.text),
            ("textDim", self.text_dim),
            ("accent", self.accent),
            ("accentDark", self.accent_dark),
            ("key", self.key),
            ("keyHover", self.key_hover),
            ("keyBorder", self.key_border),
            ("selected", self.selected),
            ("counterBg", self.counter_bg),
            ("counterText", self.counter_text),
            ("playhead", self.playhead),
            ("meterGreen", self.meter_green),
            ("meterYellow", self.meter_yellow),
            ("meterRed", self.meter_red),
            ("meterOff", self.meter_off),
            ("stop", self.stop),
            ("playing", self.playing),
            ("solo", self.solo),
        ]
    }
}

/// Tile colours: SoundCraft's 16 track colours.
pub fn tile_fill(c: TileColor) -> Color32 {
    hex(match c {
        TileColor::Blue => 0x5684D6,
        TileColor::Cyan => 0x5EAAD6,
        TileColor::Teal => 0x50BEA8,
        TileColor::Green => 0x68BA62,
        TileColor::Lime => 0xA8C44C,
        TileColor::Yellow => 0xDEC448,
        TileColor::Orange => 0xE6983E,
        TileColor::Red => 0xDE6848,
        TileColor::Crimson => 0xD6526E,
        TileColor::Pink => 0xC456A8,
        TileColor::Purple => 0x9662CE,
        TileColor::Indigo => 0x6E68D6,
        TileColor::Gray => 0x8296AA,
        TileColor::Brown => 0xB28C6E,
        TileColor::Mint => 0x78AA8C,
        TileColor::Rose => 0xBE7896,
    })
}

/// Scale a colour's channels (SoundCraft derives clip bodies and name bars this way).
pub fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Move a colour toward white by `amount` (0..1).
pub fn lighten(c: Color32, amount: f32) -> Color32 {
    let mix = |v: u8| (v as f32 + (255.0 - v as f32) * amount).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// A colour at `alpha` (0..1), premultiplied in gamma space as egui blends.
/// (Color32::from_rgba_unmultiplied premultiplies in linear light, which makes
/// translucent colours look too strong.)
pub fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    let a = alpha.clamp(0.0, 1.0);
    let m = |v: u8| (v as f32 * a).round() as u8;
    Color32::from_rgba_premultiplied(m(c.r()), m(c.g()), m(c.b()), (a * 255.0).round() as u8)
}

// ── Fonts ─────────────────────────────────────────────────────────────────────────

pub const SEMIBOLD: &str = "Inter SemiBold";
pub const BOLD: &str = "Inter Bold";
pub const CONDENSED: &str = "Antonio Bold";
pub const CONDENSED_SEMI: &str = "Antonio SemiBold";

/// Inter (OFL 1.1, from its upstream release; see assets/fonts) for all UI text, with
/// egui's bundled fonts kept as fallbacks for symbols and emoji.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 5] = [
        ("Inter Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (SEMIBOLD, include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
        (BOLD, include_bytes!("../assets/fonts/Inter-Bold.ttf")),
        // Antonio (OFL 1.1): the condensed face for the Panel theme's lettering.
        (CONDENSED, include_bytes!("../assets/fonts/Antonio-Bold.ttf")),
        (CONDENSED_SEMI, include_bytes!("../assets/fonts/Antonio-SemiBold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |face: &str| std::iter::once(face.to_owned()).chain(fallbacks.iter().cloned()).collect::<Vec<_>>();
    fonts.families.insert(FontFamily::Proportional, family("Inter Regular"));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), family(SEMIBOLD));
    fonts.families.insert(FontFamily::Name(BOLD.into()), family(BOLD));
    // Antonio lacks some symbols (e.g. U+2212 minus); Inter then egui's fonts fill in.
    let condensed_family = |face: &str| {
        std::iter::once(face.to_owned()).chain(std::iter::once("Inter Regular".to_owned())).chain(fallbacks.iter().cloned()).collect::<Vec<_>>()
    };
    fonts.families.insert(FontFamily::Name(CONDENSED.into()), condensed_family(CONDENSED));
    fonts.families.insert(FontFamily::Name(CONDENSED_SEMI.into()), condensed_family(CONDENSED_SEMI));
    ctx.set_fonts(fonts);
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

/// Antonio Bold: tall condensed lettering (Panel theme).
pub fn condensed(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(CONDENSED.into()))
}

pub fn condensed_semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(CONDENSED_SEMI.into()))
}

/// Paint `text` with fixed-width digits, so counting times don't jitter. Inter's
/// digits are proportional and egui 0.31 can't turn on its `tnum` feature, so each
/// digit is centred in a cell as wide as the widest digit. Returns the painted rect.
pub fn paint_tabular(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let widths: Vec<f32> = p.ctx().fonts(|f| {
        let cell = ('0'..='9').map(|d| f.glyph_width(&font, d)).fold(0.0, f32::max);
        text.chars().map(|c| if c.is_ascii_digit() { cell } else { f.glyph_width(&font, c) }).collect()
    });
    let total: f32 = widths.iter().sum();
    let height = p.ctx().fonts(|f| f.row_height(&font));
    let rect = anchor.anchor_size(pos, vec2(total, height));
    let mut x = rect.left();
    for (c, w) in text.chars().zip(widths) {
        let center = pos2(x + w / 2.0, rect.center().y);
        p.text(center, Align2::CENTER_CENTER, c, font.clone(), color);
        x += w;
    }
    rect
}

// ── Widgets ───────────────────────────────────────────────────────────────────────

/// A Pro Tools-style key: flat, with a light along its top edge, a dark 1 px border
/// and radius 3. It darkens while pressed. `on` fills it with the accent.
pub fn key(ui: &mut Ui, label: &str, size: egui::Vec2, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (fill, text) = if on { (t.accent_dark, Color32::WHITE) } else { (t.key, t.text) };
    key_with(ui, label, size, fill, text, semibold(12.5))
}

pub fn key_with(ui: &mut Ui, label: &str, size: egui::Vec2, fill: Color32, text: Color32, font: FontId) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if resp.is_pointer_button_down_on() {
            shade(fill, 0.8)
        } else if resp.hovered() {
            lighten(fill, 0.08)
        } else {
            fill
        };
        p.rect_filled(rect, 3.0, fill);
        p.line_segment(
            [rect.left_top() + vec2(2.0, 1.5), rect.right_top() + vec2(-2.0, 1.5)],
            Stroke::new(1.0_f32, with_alpha(Color32::WHITE, 0.12)),
        );
        p.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, t.key_border), StrokeKind::Inside);
        p.text(rect.center(), Align2::CENTER_CENTER, label, font, text);
    }
    resp
}

/// A toolbar group: a recessed box that holds related keys.
pub fn group_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(t.group)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(4)
        .inner_margin(egui::Margin::same(4))
}

// ── Style ─────────────────────────────────────────────────────────────────────────

fn style_for(style: &mut egui::Style, t: &Tokens, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0_f32, t.border);
    v.extreme_bg_color = if dark { t.bar } else { lighten(t.panel, 0.6) };
    v.faint_bg_color = t.panel2;
    v.code_bg_color = t.group;
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent_dark;
    v.selection.stroke = Stroke::new(1.0_f32, if dark { Color32::WHITE } else { t.text });
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: with_alpha(Color32::BLACK, 0.45) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: with_alpha(Color32::BLACK, 0.35) };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.panel;
    w.noninteractive.weak_bg_fill = t.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
    for (wv, fill) in [(&mut w.inactive, t.key), (&mut w.hovered, t.key_hover), (&mut w.active, t.accent_dark), (&mut w.open, t.key_hover)] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0_f32, t.key_border);
        wv.fg_stroke = Stroke::new(1.0_f32, t.text);
        wv.corner_radius = CornerRadius::same(3);
        wv.expansion = 0.0;
    }
    w.noninteractive.corner_radius = CornerRadius::same(3);
    style.visuals = v;
    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(10.0, 4.0);
    style.spacing.interact_size.y = 24.0;
    style.text_styles = [
        (TextStyle::Small, regular(10.5)),
        (TextStyle::Body, regular(13.0)),
        (TextStyle::Button, regular(13.0)),
        (TextStyle::Heading, semibold(16.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
}

/// The Panel widget style: black ground, pill-shaped controls in the theme's colours with
/// black lettering, and Antonio for all text.
fn style_for_panel(style: &mut egui::Style) {
    let t = &PANEL_TOKENS;
    style_for(style, t, true);
    let v = &mut style.visuals;
    v.window_stroke = Stroke::new(2.0_f32, PANEL.amber);
    v.extreme_bg_color = hex(0x1A120A);
    v.selection.bg_fill = PANEL.amber;
    v.selection.stroke = Stroke::new(1.0_f32, PANEL.black);
    v.window_corner_radius = CornerRadius::same(18);
    v.menu_corner_radius = CornerRadius::same(12);
    let w = &mut v.widgets;
    // Idle controls are dim lilac with peach lettering, so the labels of checkboxes and
    // unselected choices (drawn without a fill) stay readable on black; they light up to
    // full colour with black lettering when hovered, pressed or selected.
    let idle = shade(PANEL.lilac, 0.42);
    for (wv, fill, ink) in [
        (&mut w.inactive, idle, PANEL.peach),
        (&mut w.hovered, PANEL.peach, PANEL.black),
        (&mut w.active, PANEL.amber, PANEL.black),
        // "Open" also fills window title bars, whose title is peach.
        (&mut w.open, shade(PANEL.amber, 0.38), PANEL.peach),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.5_f32, ink);
        wv.corner_radius = CornerRadius::same(12);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, hex(0x3A2A18));
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, PANEL.peach);
    style.spacing.item_spacing = vec2(8.0, 7.0);
    style.spacing.button_padding = vec2(12.0, 4.0);
    style.spacing.interact_size.y = 26.0;
    style.text_styles = [
        (TextStyle::Small, condensed_semi(13.0)),
        (TextStyle::Body, condensed_semi(16.0)),
        (TextStyle::Button, condensed(16.0)),
        (TextStyle::Heading, condensed(22.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
}

/// Install fonts and both themes, and show the design and appearance asked for.
pub fn apply(ctx: &egui::Context, appearance: Appearance, kind: ThemeKind) {
    install_fonts(ctx);
    apply_look(ctx, appearance, kind);
}

/// Switch design (Console or PANEL) and, for Console, light or dark.
pub fn apply_look(ctx: &egui::Context, appearance: Appearance, kind: ThemeKind) {
    ctx.data_mut(|d| d.insert_temp(theme_kind_id(), kind));
    match kind {
        ThemeKind::Console => {
            ctx.style_mut_of(Theme::Dark, |s| style_for(s, &DARK, true));
            ctx.style_mut_of(Theme::Light, |s| style_for(s, &LIGHT, false));
            ctx.set_theme(match appearance {
                Appearance::Dark => ThemePreference::Dark,
                Appearance::Light => ThemePreference::Light,
                Appearance::System => ThemePreference::System,
            });
        }
        ThemeKind::Panel => {
            // Panel is always on black, whatever the system appearance.
            ctx.style_mut_of(Theme::Dark, style_for_panel);
            ctx.style_mut_of(Theme::Light, style_for_panel);
            ctx.set_theme(ThemePreference::Dark);
        }
    }
}

/// A procedurally drawn window icon: a 3×2 grid of tiles on a dark rounded square.
pub fn app_icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let colors = [TileColor::Red, TileColor::Orange, TileColor::Green, TileColor::Blue, TileColor::Purple, TileColor::Teal];
    for y in 0..N {
        for x in 0..N {
            let i = (y * N + x) * 4;
            let (cx, cy) = ((x as f32 - 31.5).abs(), (y as f32 - 31.5).abs());
            let r = 12.0;
            let (dx, dy) = ((cx - (32.0 - r)).max(0.0), (cy - (32.0 - r)).max(0.0));
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let mut px = [0x1E, 0x1E, 0x1E, 0xFF];
            let (gx, gy) = (x as i32 - 8, y as i32 - 14);
            if (0..48).contains(&gx) && (0..36).contains(&gy) && gx % 16 < 13 && gy % 18 < 15 {
                let c = tile_fill(colors[(gy / 18 * 3 + gx / 16) as usize]);
                px = [c.r(), c.g(), c.b(), 0xFF];
            }
            rgba[i..i + 4].copy_from_slice(&px);
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The Rust tokens must equal the FigCraft variables exported to docs/design/tokens.json.
    #[test]
    fn tokens_match_the_figcraft_export() {
        let json: serde_json::Value = serde_json::from_str(include_str!("../docs/design/tokens.json")).unwrap();
        let meta = &json["meta"];
        let modes: HashMap<String, String> = meta["variableCollections"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|c| c["modes"].as_array().unwrap().iter())
            .map(|m| (m["modeId"].as_str().unwrap().to_owned(), m["name"].as_str().unwrap().to_owned()))
            .collect();
        let to_color = |v: &serde_json::Value| {
            let ch = |k: &str| (v[k].as_f64().unwrap() * 255.0).round() as u8;
            Color32::from_rgb(ch("r"), ch("g"), ch("b"))
        };
        let mut exported: HashMap<(String, String), Color32> = HashMap::new();
        for var in meta["variables"].as_object().unwrap().values() {
            for (mode_id, value) in var["valuesByMode"].as_object().unwrap() {
                let mode = modes.get(mode_id).cloned().unwrap_or_default();
                exported.insert((var["name"].as_str().unwrap().to_owned(), mode), to_color(value));
            }
        }
        for (mode, tokens) in [("Dark", &DARK), ("Light", &LIGHT)] {
            for (name, ours) in tokens.named() {
                let theirs = exported.get(&(name.to_owned(), mode.to_owned()));
                assert_eq!(theirs, Some(&ours), "{mode} token {name} differs from docs/design/tokens.json");
            }
        }
        // Tile palette: the mockup's tile/01..16 in SoundCraft's order.
        let order = [
            TileColor::Blue,
            TileColor::Cyan,
            TileColor::Teal,
            TileColor::Green,
            TileColor::Lime,
            TileColor::Yellow,
            TileColor::Orange,
            TileColor::Red,
            TileColor::Crimson,
            TileColor::Pink,
            TileColor::Purple,
            TileColor::Indigo,
            TileColor::Gray,
            TileColor::Brown,
            TileColor::Mint,
            TileColor::Rose,
        ];
        for (i, c) in order.iter().enumerate() {
            let name = format!("tile/{:02}", i + 1);
            let theirs = exported.iter().find(|((n, _), _)| *n == name).map(|(_, v)| *v);
            assert_eq!(theirs, Some(tile_fill(*c)), "{name} ({c:?})");
        }
        assert_eq!(order.len(), TileColor::ALL.len());
    }

    /// The Panel palette must equal the FigCraft variables in docs/design/panel-tokens.json.
    #[test]
    fn panel_palette_matches_the_figcraft_export() {
        let json: serde_json::Value = serde_json::from_str(include_str!("../docs/design/panel-tokens.json")).unwrap();
        let find = |name: &str| {
            json["meta"]["variables"].as_object().unwrap().values().find(|v| v["name"] == name).map(|v| {
                let c = v["valuesByMode"].as_object().unwrap().values().next().unwrap().clone();
                let ch = |k: &str| (c[k].as_f64().unwrap() * 255.0).round() as u8;
                Color32::from_rgb(ch("r"), ch("g"), ch("b"))
            })
        };
        let p = PANEL;
        for (name, ours) in [
            ("black", p.black), ("amber", p.amber), ("peach", p.peach), ("gold", p.gold), ("lilac", p.lilac),
            ("violet", p.violet), ("periwinkle", p.periwinkle), ("sky", p.sky), ("blue", p.blue),
            ("salmon", p.salmon), ("red", p.red), ("tan", p.tan), ("text", p.text), ("dim", p.dim),
        ] {
            assert_eq!(find(name), Some(ours), "Panel token {name} differs from docs/design/panel-tokens.json");
        }
    }

    #[test]
    fn colour_helpers() {
        assert_eq!(shade(hex(0x804020), 0.5), hex(0x402010));
        assert_eq!(lighten(hex(0x000000), 0.5), hex(0x808080));
        assert_eq!(with_alpha(Color32::WHITE, 0.35), Color32::from_rgba_premultiplied(89, 89, 89, 89));
    }
}
