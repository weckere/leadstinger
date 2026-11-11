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

pub mod contrast;

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
    /// The keyboard focus ring (M20). Not a FigCraft variable yet; at least 3:1 against
    /// every surface it's drawn on (see the contrast test).
    pub focus: Color32,
    /// Status colours (D-120), the same in every built-in look: errors and warnings in
    /// text (`status/error`), the frame while files are dragged over the window
    /// (`status/drop`), the trim handles (`status/trim`), markers and the vamp section on
    /// waveforms (`mark/marker`, `mark/vamp`), a Solo pad's border (`mark/solo`, D-146), and the input level under the voice
    /// threshold while beds duck and while they don't (`input/voice`, `input/level`).
    pub error: Color32,
    pub drop: Color32,
    pub trim: Color32,
    pub marker: Color32,
    pub vamp: Color32,
    /// A Solo pad's border (`mark/solo`, D-146): drawn on the pad's own colour, with a
    /// dark line inside it.
    pub solo_mark: Color32,
    pub input_voice: Color32,
    pub input_level: Color32,
}

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
    focus: hex(0x6CB4FF),
    error: hex(0xE63228),
    drop: hex(0x4084C4),
    trim: hex(0xDEC434),
    marker: hex(0xE87AD0),
    vamp: hex(0x5CC8E6),
    solo_mark: hex(0xF2F2F2),
    input_voice: hex(0xE08A2E),
    input_level: hex(0x4CAF50),
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
    // 0x4A4C50 until M20: 0x404246 reaches 4.5:1 on the window and the status bar.
    text_dim: hex(0x404246),
    accent: hex(0x3E74B4),
    key: hex(0xDADCE0),
    key_hover: hex(0xE6E8EC),
    key_border: hex(0x8E9094),
    selected: hex(0xA8BCD8),
    meter_off: hex(0x34363A),
    meter_green: hex(0x24A03A),
    meter_yellow: hex(0xBEA214),
    meter_red: hex(0xC32A23),
    focus: hex(0x1A4FA0),
    ..DARK
};

/// The high-contrast appearance (M20, D-085): white on black, light blue for the accent,
/// yellow for the focus ring, and every control outlined. Every text pair reaches 4.5:1
/// and every control and focus indicator 3:1 (the contrast test checks them).
pub const HIGH_CONTRAST: Tokens = Tokens {
    window: hex(0x000000),
    panel: hex(0x000000),
    panel2: hex(0x0A0A0A),
    bar: hex(0x000000),
    group: hex(0x000000),
    border: hex(0xA0A0A0),
    border_light: hex(0xC8C8C8),
    text: hex(0xFFFFFF),
    text_dim: hex(0xDCDCDC),
    accent: hex(0x5CB8FF),
    accent_dark: hex(0x0A4FA8),
    key: hex(0x1A1A1A),
    key_hover: hex(0x3A3A3A),
    key_border: hex(0xE0E0E0),
    selected: hex(0x003C80),
    counter_bg: hex(0x000000),
    counter_text: hex(0x7CFF8A),
    playhead: hex(0xFF5A4A),
    meter_green: hex(0x3CE650),
    meter_yellow: hex(0xFFE640),
    meter_red: hex(0xFF4A3A),
    meter_off: hex(0x2A2A2A),
    stop: hex(0x8C1C1C),
    playing: hex(0x1C6A2C),
    solo: hex(0xFFE040),
    focus: hex(0xFFD60A),
    solo_mark: hex(0xFFFFFF),
    ..STATUS
};

/// The status colours of every built-in look (D-120): Dark's.
const STATUS: Tokens = DARK;

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

impl PanelPalette {
    /// Each colour by its FigCraft variable name (docs/design/panel-tokens.json).
    pub fn fields_mut(&mut self) -> [(&'static str, &mut Color32); 14] {
        [
            ("black", &mut self.black),
            ("amber", &mut self.amber),
            ("peach", &mut self.peach),
            ("gold", &mut self.gold),
            ("lilac", &mut self.lilac),
            ("violet", &mut self.violet),
            ("periwinkle", &mut self.periwinkle),
            ("sky", &mut self.sky),
            ("blue", &mut self.blue),
            ("salmon", &mut self.salmon),
            ("red", &mut self.red),
            ("tan", &mut self.tan),
            ("text", &mut self.text),
            ("dim", &mut self.dim),
        ]
    }

    pub fn named(&self) -> [(&'static str, Color32); 14] {
        let mut copy = *self;
        copy.fields_mut().map(|(n, v)| (n, *v))
    }
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
pub const PANEL_TOKENS: Tokens = panel_tokens(PANEL);

/// The Panel theme's generic tokens in high contrast (D-085, t-m22-panel-hc): white
/// lettering on black, peach for the dim lettering and the borders, white focus. The
/// Panel painters' colours come from [`Look::panel_high_contrast`].
pub const PANEL_HC_TOKENS: Tokens = panel_hc_tokens(PANEL_TOKENS, PANEL);

/// The outline of a selected Panel block (t-m22-panel-select-ring): white, which is
/// none of the blocks' colours.
pub const PANEL_SELECT_RING: Color32 = Color32::WHITE;

/// What black lettering reaches on every Panel colour in high contrast: WCAG's enhanced
/// 7:1 rather than AA's 4.5:1 (t-m22-panel-hc).
pub const PANEL_HC_INK: f32 = 7.0;

/// Panel's generic tokens from its palette. A user theme on the Panel base gets its own
/// this way, so changing `amber` changes the accent too (D-105).
pub const fn panel_tokens(p: PanelPalette) -> Tokens {
    Tokens {
        window: p.black,
        panel: p.black,
        panel2: p.black,
        bar: p.black,
        group: p.black,
        border: hex(0x3A2A18),
        border_light: p.dim,
        text: p.peach,
        text_dim: p.dim,
        accent: p.amber,
        accent_dark: p.amber,
        key: p.lilac,
        key_hover: p.peach,
        key_border: p.black,
        selected: hex(0x3A2A18),
        counter_bg: p.black,
        counter_text: p.peach,
        playhead: p.red,
        meter_green: p.gold,
        meter_yellow: p.amber,
        meter_red: p.red,
        meter_off: hex(0x2A1A0E),
        stop: p.red,
        playing: p.gold,
        solo: p.gold,
        focus: p.sky,
        error: STATUS.error,
        drop: STATUS.drop,
        trim: STATUS.trim,
        marker: STATUS.marker,
        vamp: STATUS.vamp,
        solo_mark: p.black,
        input_voice: STATUS.input_voice,
        input_level: STATUS.input_level,
    }
}

/// Panel's high-contrast tokens from its designed ones and its palette.
pub const fn panel_hc_tokens(t: Tokens, p: PanelPalette) -> Tokens {
    Tokens {
        border: p.peach,
        border_light: hex(0xFFFFFF),
        text: hex(0xFFFFFF),
        text_dim: p.peach,
        counter_text: hex(0xFFFFFF),
        selected: hex(0x5A3A18),
        meter_off: hex(0x3A2A1C),
        focus: hex(0xFFFFFF),
        ..t
    }
}

/// `c` lightened in 5 % steps until black lettering on it reaches `ratio`.
pub fn lift_for_black(c: Color32, ratio: f32) -> Color32 {
    (0..=20).map(|i| lighten(c, i as f32 * 0.05)).find(|&l| contrast_ratio(Color32::BLACK, l) >= ratio).unwrap_or(Color32::WHITE)
}

/// The Panel theme's text fields and level tracks (egui's extreme background).
pub const PANEL_FIELD: Color32 = hex(0x1A120A);

/// Lettering on a Console tile's name bar.
pub const TILE_INK: Color32 = hex(0x0C0C0C);
/// Stop All's lettering on the `stop` key, and GO's on the `playing` key.
pub const STOP_INK: Color32 = hex(0xFFD6D2);
pub const GO_INK: Color32 = hex(0x78FF8C);

/// The Panel theme's set blocks, in turn down the column.
pub const PANEL_SET_COLORS: [Color32; 8] =
    [PANEL.peach, PANEL.lilac, PANEL.periwinkle, PANEL.tan, PANEL.sky, PANEL.gold, PANEL.salmon, PANEL.violet];

/// A Panel tile with nothing to play is its colour darkened this much, lettered in peach
/// (black until M20, which was under 3:1 on every colour).
pub const PANEL_EMPTY_SHADE: f32 = 0.4;
pub const PANEL_EMPTY_INK: Color32 = PANEL.peach;

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

/// The 16 tile colours in SoundCraft's order: the order of `tile/01`..`tile/16` in a
/// theme's tokens.json.
pub const TILE_ORDER: [TileColor; 16] = [
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

/// Where `c` is in [`TILE_ORDER`].
pub fn tile_index(c: TileColor) -> usize {
    TILE_ORDER.iter().position(|&o| o == c).unwrap_or(0)
}

/// Lettering drawn on fills the tokens don't name (`ink/*` in a theme).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inks {
    /// A Console tile's title on its name bar.
    pub tile: Color32,
    /// Stop All's lettering on `stop`, and GO's on `playing`.
    pub stop: Color32,
    pub go: Color32,
    /// A Panel tile with nothing to play.
    pub empty: Color32,
}

pub const INKS: Inks = Inks { tile: TILE_INK, stop: STOP_INK, go: GO_INK, empty: PANEL_EMPTY_INK };

/// What a disabled tile is laid over with (`disabled/veil`, `disabled/stripes` in a
/// theme; translucent, so their alpha counts, D-120): dimmed, then striped so it doesn't
/// rely on being darker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Veil {
    pub fill: Color32,
    pub stripes: Color32,
}

pub const CONSOLE_VEIL: Veil = Veil { fill: Color32::from_black_alpha(150), stripes: Color32::from_rgba_premultiplied(36, 36, 36, 36) };
/// Panel stripes in black.
pub const PANEL_VEIL: Veil = Veil { stripes: Color32::from_black_alpha(110), ..CONSOLE_VEIL };

/// The sizes a theme can change (`size/*`, `radius/*`, `space/*`): egui's text styles,
/// corner radii and spacing. Everything else about layout stays in code (D-105).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub text_small: f32,
    pub text_body: f32,
    pub text_button: f32,
    pub text_heading: f32,
    pub radius_control: f32,
    pub radius_window: f32,
    pub radius_menu: f32,
    pub item_spacing: egui::Vec2,
    pub button_padding: egui::Vec2,
    /// The height of a control (egui's `interact_size.y`).
    pub control_height: f32,
}

pub const CONSOLE_METRICS: Metrics = Metrics {
    text_small: 10.5,
    text_body: 13.0,
    text_button: 13.0,
    text_heading: 16.0,
    radius_control: 3.0,
    radius_window: 6.0,
    radius_menu: 4.0,
    item_spacing: vec2(8.0, 6.0),
    button_padding: vec2(10.0, 4.0),
    control_height: 24.0,
};

pub const PANEL_METRICS: Metrics = Metrics {
    text_small: 13.0,
    text_body: 16.0,
    text_button: 16.0,
    text_heading: 22.0,
    radius_control: 12.0,
    radius_window: 18.0,
    radius_menu: 12.0,
    item_spacing: vec2(8.0, 7.0),
    button_padding: vec2(12.0, 4.0),
    control_height: 26.0,
};

/// The font faces a theme can replace, as theme.json's `fonts` keys and the families
/// they go first in.
pub const FACE_KEYS: [(&str, &str); 5] =
    [("regular", "Inter Regular"), ("semibold", SEMIBOLD), ("bold", BOLD), ("condensed", CONDENSED), ("condensedSemi", CONDENSED_SEMI)];

/// A whole theme: the built-in Console or Panel, or a user theme (src/themes.rs reads
/// them from .lstheme files, D-105). The window keeps the one in use in its context.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// `console`, `panel`, or the user theme's folder name.
    pub id: String,
    pub name: String,
    /// Which design draws it.
    pub base: ThemeKind,
    /// The generic tokens: Console's Dark, Light and High Contrast; Panel's designed
    /// look in `dark` and `light` alike, and its high contrast.
    pub dark: Tokens,
    pub light: Tokens,
    pub high_contrast: Tokens,
    /// What the Panel painters draw with.
    pub panel: PanelPalette,
    /// Tile colours by [`TILE_ORDER`]: Console's, and Panel's.
    pub tiles: [Color32; 16],
    pub panel_tiles: [Color32; 16],
    /// Panel's set blocks, in turn down the column.
    pub sets: [Color32; 8],
    pub ink: Inks,
    /// Over a disabled tile.
    pub veil: Veil,
    pub metrics: Metrics,
    /// Replacement faces by [`FACE_KEYS`]; `None` keeps the built-in one.
    pub fonts: [Option<Arc<[u8]>>; 5],
}

impl Look {
    pub fn console() -> Look {
        Look {
            id: "console".into(),
            name: "Console".into(),
            base: ThemeKind::Console,
            dark: DARK,
            light: LIGHT,
            high_contrast: HIGH_CONTRAST,
            panel: PANEL,
            tiles: TILE_ORDER.map(tile_fill),
            panel_tiles: TILE_ORDER.map(panel_tile_fill),
            sets: PANEL_SET_COLORS,
            ink: INKS,
            veil: CONSOLE_VEIL,
            metrics: CONSOLE_METRICS,
            fonts: Default::default(),
        }
    }

    pub fn panel() -> Look {
        Look {
            id: "panel".into(),
            name: crate::brand::PANEL_THEME.into(),
            base: ThemeKind::Panel,
            dark: PANEL_TOKENS,
            light: PANEL_TOKENS,
            high_contrast: PANEL_HC_TOKENS,
            veil: PANEL_VEIL,
            metrics: PANEL_METRICS,
            ..Look::console()
        }
    }

    pub fn builtin(kind: ThemeKind) -> Look {
        match kind {
            ThemeKind::Console => Look::console(),
            ThemeKind::Panel => Look::panel(),
        }
    }

    /// What the Panel painters draw with in high contrast (t-m22-panel-hc): every
    /// pastel lightened until black lettering on it reaches [`PANEL_HC_INK`] (most
    /// already do; violet, blue, red and rose get paler), white lettering on black, peach
    /// for the dim lettering, and white on an empty pad. [`apply_look`] shows this in
    /// place of the look, so a theme on the Panel base gets the same.
    pub fn panel_high_contrast(&self) -> Look {
        let lift = |c| lift_for_black(c, PANEL_HC_INK);
        let mut look = self.clone();
        for (name, c) in look.panel.fields_mut() {
            *c = match name {
                "black" => *c,
                "text" => Color32::WHITE,
                "dim" => lift(self.panel.peach),
                _ => lift(*c),
            };
        }
        look.panel_tiles = self.panel_tiles.map(lift);
        look.sets = self.sets.map(lift);
        look.ink.empty = Color32::WHITE;
        look
    }

    pub fn is_builtin(&self) -> bool {
        self.id == "console" || self.id == "panel"
    }

    /// The generic tokens for an appearance.
    pub fn tokens(&self, high_contrast: bool, dark: bool) -> Tokens {
        if high_contrast {
            self.high_contrast
        } else if dark || self.base == ThemeKind::Panel {
            self.dark
        } else {
            self.light
        }
    }
}

fn look_id() -> egui::Id {
    egui::Id::unique("leadstinger-look")
}

fn high_contrast_id() -> egui::Id {
    egui::Id::unique("leadstinger-high-contrast")
}

static CONSOLE: std::sync::LazyLock<Arc<Look>> = std::sync::LazyLock::new(|| Arc::new(Look::console()));

/// The theme showing (set by [`apply_look`]).
pub fn look(ctx: &egui::Context) -> Arc<Look> {
    ctx.data(|d| d.get_temp::<Arc<Look>>(look_id())).unwrap_or_else(|| CONSOLE.clone())
}

/// The high-contrast appearance is on (set by [`apply_look`]), in either design.
pub fn is_high_contrast(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(high_contrast_id())).unwrap_or(false)
}

/// What the Panel painters draw with.
pub fn palette(ctx: &egui::Context) -> PanelPalette {
    look(ctx).panel
}

/// The Panel theme's dim lettering: peach in high contrast.
pub fn panel_dim(ctx: &egui::Context) -> Color32 {
    let p = palette(ctx);
    if is_high_contrast(ctx) { p.peach } else { p.dim }
}

/// A tile colour as the Console painters draw it, in the theme showing.
pub fn console_tile(ctx: &egui::Context, c: TileColor) -> Color32 {
    look(ctx).tiles[tile_index(c)]
}

/// A tile colour as the Panel painters draw it.
pub fn panel_tile(ctx: &egui::Context, c: TileColor) -> Color32 {
    look(ctx).panel_tiles[tile_index(c)]
}

/// Lettering colours of the theme showing.
pub fn inks(ctx: &egui::Context) -> Inks {
    look(ctx).ink
}

/// Which design is showing (set by [`apply_look`]).
pub fn kind(ctx: &egui::Context) -> ThemeKind {
    look(ctx).base
}

pub fn is_panel(ctx: &egui::Context) -> bool {
    kind(ctx) == ThemeKind::Panel
}

impl Tokens {
    /// The tokens for the theme and appearance egui is currently showing.
    pub fn get(ctx: &egui::Context) -> Tokens {
        look(ctx).tokens(is_high_contrast(ctx), ctx.global_style().visuals.dark_mode)
    }

    /// Lettering on an unlit key (`theme::key`, a hotkey cap): the text colour, or black
    /// or white where that doesn't read on the key, as on Panel's lilac.
    pub fn key_text(&self) -> Color32 {
        legible(self.text, self.key)
    }

    /// Lettering on a lit key (`accent_dark`): white, or black on Panel's amber.
    pub fn key_on_text(&self) -> Color32 {
        legible(Color32::WHITE, self.accent_dark)
    }

    /// The accent for text and links: the accent, lightened (dark looks) or darkened
    /// (light ones) until it reaches 4.5:1 on every surface.
    pub fn accent_text(&self) -> Color32 {
        let light = luminance(self.panel) > 0.18;
        let mut c = self.accent;
        for _ in 0..20 {
            if [self.window, self.panel, self.panel2, self.bar, self.group].iter().all(|&bg| contrast_ratio(c, bg) >= 4.5) {
                break;
            }
            c = if light { shade(c, 0.94) } else { lighten(c, 0.06) };
        }
        c
    }

    /// Each token by its FigCraft variable name (a theme's tokens.json), for reading and
    /// writing themes.
    /// The status colours' names have a group (`status/`, `mark/`, `input/`), and a
    /// Panel theme gives them without the `ui/` prefix.
    pub fn fields_mut(&mut self) -> [(&'static str, &mut Color32); 34] {
        [
            ("window", &mut self.window),
            ("panel", &mut self.panel),
            ("panel2", &mut self.panel2),
            ("bar", &mut self.bar),
            ("group", &mut self.group),
            ("border", &mut self.border),
            ("borderLight", &mut self.border_light),
            ("text", &mut self.text),
            ("textDim", &mut self.text_dim),
            ("accent", &mut self.accent),
            ("accentDark", &mut self.accent_dark),
            ("key", &mut self.key),
            ("keyHover", &mut self.key_hover),
            ("keyBorder", &mut self.key_border),
            ("selected", &mut self.selected),
            ("counterBg", &mut self.counter_bg),
            ("counterText", &mut self.counter_text),
            ("playhead", &mut self.playhead),
            ("meterGreen", &mut self.meter_green),
            ("meterYellow", &mut self.meter_yellow),
            ("meterRed", &mut self.meter_red),
            ("meterOff", &mut self.meter_off),
            ("stop", &mut self.stop),
            ("playing", &mut self.playing),
            ("solo", &mut self.solo),
            ("focus", &mut self.focus),
            ("status/error", &mut self.error),
            ("status/drop", &mut self.drop),
            ("status/trim", &mut self.trim),
            ("mark/marker", &mut self.marker),
            ("mark/vamp", &mut self.vamp),
            ("mark/solo", &mut self.solo_mark),
            ("input/voice", &mut self.input_voice),
            ("input/level", &mut self.input_level),
        ]
    }

    /// (FigCraft variable name, value) pairs.
    pub fn named(&self) -> [(&'static str, Color32); 34] {
        let mut copy = *self;
        copy.fields_mut().map(|(n, v)| (n, *v))
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

// ── Contrast and state markers (M20) ──────────────────────────────────────────────

/// A colour's relative luminance (WCAG 2.2), 0 for black to 1 for white.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// The WCAG contrast ratio of two opaque colours, from 1 (the same) to 21 (black on white).
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` at `alpha` over the opaque `under`, as it shows on screen.
pub fn over(under: Color32, top: Color32, alpha: f32) -> Color32 {
    let mix = |u: u8, t: u8| (u as f32 + (t as f32 - u as f32) * alpha).round() as u8;
    Color32::from_rgb(mix(under.r(), top.r()), mix(under.g(), top.g()), mix(under.b(), top.b()))
}

/// Black or white, whichever reads better on `bg`.
pub fn ink_on(bg: Color32) -> Color32 {
    if contrast_ratio(Color32::WHITE, bg) >= contrast_ratio(Color32::BLACK, bg) { Color32::WHITE } else { Color32::BLACK }
}

/// `ink` where it reaches 4.5:1 on `bg`, otherwise black or white ([`ink_on`]).
pub fn legible(ink: Color32, bg: Color32) -> Color32 {
    if contrast_ratio(ink, bg) >= 4.5 { ink } else { ink_on(bg) }
}

/// A Console tile's name bar: its colour ×0.9, or the colour itself in high contrast,
/// where every colour then has a title ink that reaches 4.5:1.
pub fn tile_bar_fill(base: Color32, high_contrast: bool) -> Color32 {
    if high_contrast { base } else { shade(base, 0.9) }
}

/// How dark the hotkey cap on a Console tile's name bar is (black at this alpha).
pub fn tile_cap_alpha(high_contrast: bool) -> f32 {
    if high_contrast { 0.55 } else { 0.35 }
}

/// A Console tile's title on its name bar: the theme's tile ink ([`TILE_INK`] in the
/// built-ins), or in high contrast whichever of it and white reads better.
pub fn tile_ink(bar: Color32, high_contrast: bool, ink: Color32) -> Color32 {
    if high_contrast && contrast_ratio(Color32::WHITE, bar) > contrast_ratio(ink, bar) { Color32::WHITE } else { ink }
}

/// The mode badges on a Console tile's name bar: the title's ink at 60 %, or full
/// strength in high contrast.
pub fn tile_badge_ink(bar: Color32, high_contrast: bool, ink: Color32) -> Color32 {
    let ink = tile_ink(bar, high_contrast, ink);
    if high_contrast { ink } else { with_alpha(ink, 0.6) }
}

/// The default pad's body (D-152): the tile colour this dark, as the body under the name
/// bar was before M22, and a little lighter while the pointer is over it.
pub const PAD_BODY_SHADE: f32 = 0.34;
pub const PAD_BODY_HOVER_SHADE: f32 = 0.40;

/// How opaque the plates under a coloured pad's title lines and badges are: a little
/// waveform shows through, except in high contrast. A dark pad has no plates: its title
/// is brighter than its waveform and haloed instead ([`pad_title_tone`], [`PadFace::halo`]).
pub fn pad_plate_alpha(high_contrast: bool) -> f32 {
    if high_contrast { 1.0 } else { 0.78 }
}

/// What a dark pad's title reaches on its body, at its brightest (hovered, under the
/// played wash): AA, or 7:1 in high contrast.
pub fn pad_title_contrast(high_contrast: bool) -> f32 {
    if high_contrast { 7.0 } else { 4.5 }
}

/// A default pad's waveform and border: the tile colour, a touch lighter (as the
/// inspector's waveform).
pub fn pad_wave(base: Color32) -> Color32 {
    lighten(base, 0.15)
}

/// How much brighter than its waveform a dark pad's title is at least, as a contrast
/// ratio: a little, so the title reads as lying over the waveform without a plate.
pub const PAD_TITLE_OVER_WAVE: f32 = 1.3;

/// A default pad's title (D-152): its waveform's colour ([`pad_wave`]), lightened only as
/// far as it takes to be [`PAD_TITLE_OVER_WAVE`] brighter than it and to reach
/// [`pad_title_contrast`] on [`pad_title_ground`], so it keeps its hue.
pub fn pad_title_tone(base: Color32, high_contrast: bool) -> Color32 {
    let (body, wave) = (pad_title_ground(base), pad_wave(base));
    (1..=PAD_TITLE_STEPS)
        .map(|n| lighten(wave, n as f32 / PAD_TITLE_STEPS as f32))
        .find(|&c| contrast_ratio(c, body) >= pad_title_contrast(high_contrast) && contrast_ratio(c, wave) >= PAD_TITLE_OVER_WAVE)
        .unwrap_or(Color32::WHITE)
}

/// [`pad_title_tone`] tries the waveform's colour this many steps of the way to white.
pub const PAD_TITLE_STEPS: u32 = 40;

/// The brightest a dark pad's body gets under its title: hovered, with the played wash
/// over it (a paused pad is both).
pub fn pad_title_ground(base: Color32) -> Color32 {
    over(shade(base, PAD_BODY_HOVER_SHADE), Color32::WHITE, PAD_WASH_ALPHA)
}

/// The white wash over a dark pad's played part.
pub const PAD_WASH_ALPHA: f32 = 0.06;

/// The halo round a dark pad's lettering: the tile colour almost black, at this alpha,
/// so the title reads where it crosses the waveform. Even one layer of it over the
/// waveform keeps the title's own contrast (theme test `pad_faces_by_mode`).
pub const PAD_HALO_SHADE: f32 = 0.12;
pub const PAD_HALO_ALPHA: f32 = 0.75;

/// The colours a Console pad is painted with: by default a dark body with the tile's
/// colour on the title, the waveform and the border (D-152); with Settings ▸ Coloured
/// pads, the tile's colour all over (D-146).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadFace {
    /// The pad's fill, hover included.
    pub body: Color32,
    /// Under each title line, the badges and the detail line of a coloured pad, so they
    /// read over the waveform; clear on a dark pad.
    pub plate: Color32,
    /// Round the title, the badges and the detail line of a dark pad, a soft dark edge
    /// so they read where they cross the waveform; clear on a coloured pad.
    pub halo: Color32,
    /// The title, the option badges and a caption.
    pub title: Color32,
    /// The detail line (VAMP, PRE 2s…).
    pub detail: Color32,
    /// The waveform left of the playhead, and the rest of it.
    pub wave_played: Color32,
    pub wave_rest: Color32,
    /// Laid over the played part.
    pub wash: Color32,
    /// The hotkey cap, its letter, and its edge.
    pub cap: Color32,
    pub cap_ink: Color32,
    pub cap_edge: Option<Color32>,
    /// The time.
    pub time: Color32,
    /// The border in the tile's colour; `None` keeps the thin dark outline.
    pub border: Option<Color32>,
}

/// A pad's colours from its tile colour `base` (already shaded when it's empty) and the
/// theme's tile ink. `active`: it's playing or paused.
pub fn pad_face(base: Color32, colored: bool, high_contrast: bool, ink: Color32, hovered: bool, active: bool) -> PadFace {
    let plate_alpha = pad_plate_alpha(high_contrast);
    if colored {
        // The old name bar's fill, so the theme's tile ink still reads on it.
        let pad = tile_bar_fill(base, high_contrast);
        let body = if hovered { lighten(pad, 0.1) } else { pad };
        let title = tile_ink(pad, high_contrast, ink);
        return PadFace {
            body,
            plate: with_alpha(body, plate_alpha),
            halo: Color32::TRANSPARENT,
            title,
            detail: tile_badge_ink(pad, high_contrast, ink),
            wave_played: shade(pad, 0.32),
            wave_rest: with_alpha(shade(pad, 0.55), if active { 0.6 } else { 0.85 }),
            wash: with_alpha(Color32::BLACK, 0.10),
            cap: with_alpha(Color32::BLACK, tile_cap_alpha(high_contrast)),
            cap_ink: Color32::WHITE,
            cap_edge: None,
            time: title,
            border: None,
        };
    }
    let body = shade(base, if hovered { PAD_BODY_HOVER_SHADE } else { PAD_BODY_SHADE });
    let title = pad_title_tone(base, high_contrast);
    let wave = pad_wave(base);
    PadFace {
        body,
        plate: Color32::TRANSPARENT,
        halo: with_alpha(shade(base, PAD_HALO_SHADE), PAD_HALO_ALPHA),
        title,
        detail: title,
        wave_played: wave,
        wave_rest: with_alpha(wave, if active { 0.55 } else { 0.85 }),
        wash: with_alpha(Color32::WHITE, PAD_WASH_ALPHA),
        cap: with_alpha(Color32::BLACK, PAD_CAP_ALPHA),
        cap_ink: title,
        cap_edge: Some(with_alpha(title, 0.5)),
        time: if active { Color32::WHITE } else { title },
        border: Some(wave),
    }
}

/// How dark the hotkey cap on a default (dark) pad is: black at this alpha, lettered in
/// the title's tone.
pub const PAD_CAP_ALPHA: f32 = 0.4;

/// Diagonal stripes across `rect`: the disabled pattern, so a disabled tile reads as
/// disabled without telling shades apart. Pass a rect inside any rounded corners.
pub fn paint_hatch(p: &Painter, rect: Rect, color: Color32) {
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    let h = rect.height();
    let mut x = rect.left() - h;
    while x < rect.right() {
        p.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], Stroke::new(2.0_f32, color));
        x += 9.0;
    }
}

/// A check mark in a disc: the selected marker, so a selected tile doesn't rely on its
/// outline's colour alone. Drawn with lines, so no font needs the glyph.
pub fn paint_check_badge(p: &Painter, center: Pos2, fill: Color32, ink: Color32) {
    p.circle_filled(center, 8.0, fill);
    p.circle_stroke(center, 8.0, Stroke::new(1.0_f32, ink));
    let s = Stroke::new(2.0_f32, ink);
    p.line_segment([center + vec2(-4.0, 0.5), center + vec2(-1.0, 3.5)], s);
    p.line_segment([center + vec2(-1.0, 3.5), center + vec2(4.0, -3.0)], s);
}

// ── Fonts ─────────────────────────────────────────────────────────────────────────

pub const SEMIBOLD: &str = "Inter SemiBold";
pub const BOLD: &str = "Inter Bold";
pub const CONDENSED: &str = "Antonio Bold";
pub const CONDENSED_SEMI: &str = "Antonio SemiBold";
const ANTONIO_BOLD: &[u8] = include_bytes!("../assets/fonts/Antonio-Bold.ttf");

/// Inter (OFL 1.1, from its upstream release; see assets/fonts) for all UI text, with
/// egui's bundled fonts kept as fallbacks for symbols and emoji. A theme's own faces
/// (`fonts`, already checked by src/themes.rs) go first in their families, with the
/// built-in face behind them for anything they lack.
pub fn install_fonts(ctx: &egui::Context, theme_faces: &[Option<Arc<[u8]>>; 5]) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 5] = [
        ("Inter Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (SEMIBOLD, include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
        (BOLD, include_bytes!("../assets/fonts/Inter-Bold.ttf")),
        // Antonio (OFL 1.1): the condensed face for the Panel theme's lettering.
        (CONDENSED, ANTONIO_BOLD),
        (CONDENSED_SEMI, include_bytes!("../assets/fonts/Antonio-SemiBold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes).tweak(caps_centred(bytes))));
    }
    let mut fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // egui's monospace face (Hack) goes last: it's the only bundled face with some
    // symbols the UI writes, such as U+25B8 ▸ in "Settings ▸ QuipClipper".
    for face in fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default() {
        if !fallbacks.contains(&face) {
            fallbacks.push(face);
        }
    }
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
    // Monospace (Hack) borrows Inter's symbols it lacks, such as U+2325 ⌥ in a key.
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.push("Inter Regular".to_owned());
    }
    for ((key, face), bytes) in FACE_KEYS.iter().zip(theme_faces) {
        let Some(bytes) = bytes else { continue };
        let name = format!("theme {key}");
        fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes.to_vec()).tweak(caps_centred(bytes))));
        let family = if *face == "Inter Regular" { FontFamily::Proportional } else { FontFamily::Name((*face).into()) };
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, name);
        }
    }
    ctx.set_fonts(fonts);
}

/// How far a face's capitals sit below the middle of the line egui gives it (ascent,
/// descent and leading), in em. Antonio's line has room for accents above its capitals
/// and little below, so they sat 0.08 em low, and lettering centred in a block or button
/// by its line looked low (t-m22-panel-polish). Inter's are already centred.
pub fn caps_offset(bytes: &[u8]) -> f32 {
    use skrifa::{instance::LocationRef, instance::Size, MetadataProvider};
    let Ok(font) = skrifa::FontRef::new(bytes) else { return 0.0 };
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    let cap = m.cap_height.filter(|c| *c > 0.0).unwrap_or(0.7 * m.units_per_em as f32);
    // From the line's top: the capitals' middle is at ascent - cap / 2, the line's at
    // (ascent - descent + leading) / 2 (descent is negative).
    (m.ascent - cap + m.descent - m.leading) / 2.0 / m.units_per_em.max(1) as f32
}

/// How far below the top of its line Antonio Bold's capitals start, in em, once
/// [`caps_centred`] has moved them: Panel hangs its compact title from it.
pub fn condensed_cap_top() -> f32 {
    use skrifa::{instance::LocationRef, instance::Size, MetadataProvider};
    let font = skrifa::FontRef::new(ANTONIO_BOLD).expect("the bundled Antonio");
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    (m.ascent - m.cap_height.unwrap_or_default()) / m.units_per_em as f32 - caps_offset(ANTONIO_BOLD)
}

/// Every face we install is moved so its capitals sit in the middle of the line: then
/// anything centred by its line (egui's buttons, `Align2::*_CENTER`) is centred by eye.
fn caps_centred(bytes: &[u8]) -> egui::FontTweak {
    egui::FontTweak { y_offset_factor: -caps_offset(bytes), ..Default::default() }
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
/// digits are proportional and egui can't turn on its `tnum` feature, so each
/// digit is centred in a cell as wide as the widest digit. Returns the painted rect.
pub fn paint_tabular(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let (widths, height) = p.ctx().fonts_mut(|f| {
        let cell = ('0'..='9').map(|d| f.glyph_width(&font, d)).fold(0.0, f32::max);
        let widths: Vec<f32> = text.chars().map(|c| if c.is_ascii_digit() { cell } else { f.glyph_width(&font, c) }).collect();
        (widths, f.row_height(&font))
    });
    let total: f32 = widths.iter().sum();
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
    let (fill, text) = if on { (t.accent_dark, t.key_on_text()) } else { (t.key, t.key_text()) };
    let resp = key_with(ui, label, size, fill, text, semibold(12.5));
    if on {
        // A lit bar along the bottom, so "on" isn't told by the fill colour alone.
        let r = resp.rect;
        ui.painter().line_segment([r.left_bottom() + vec2(6.0, -3.5), r.right_bottom() + vec2(-6.0, -3.5)], Stroke::new(2.0_f32, text));
    }
    resp
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

/// The smallest target a pointer should have to hit (M20): about 24 px at 100 % zoom.
pub const TARGET: f32 = 24.0;

/// A compact button that's still a [`TARGET`] square at least.
pub fn small(button: egui::Button<'_>) -> egui::Button<'_> {
    button.small().min_size(vec2(TARGET, TARGET))
}

/// `ui.small_button`, at least a [`TARGET`] square.
pub fn small_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    ui.add(small(egui::Button::new(text)))
}

/// A checkbox with its label in the Button text style. egui 0.32 moved checkbox labels
/// to Body, which in the Panel theme is the lighter face; this keeps them as they were.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: &str) -> Response {
    ui.checkbox(checked, egui::RichText::new(text).text_style(TextStyle::Button))
}

/// A text field as egui 0.36 and earlier drew it: 4×2 px around the text and no minimum
/// height. egui 0.37 pads text fields like buttons and makes them at least a button tall.
pub fn text_field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    ui.scope(|ui| {
        let spacing = ui.spacing_mut();
        spacing.button_padding = vec2(4.0, 2.0);
        spacing.interact_size.y = 0.0;
        ui.add(edit)
    })
    .inner
}

// ── Hover text ────────────────────────────────────────────────────────────────────

/// Every control's hover text (M20): what it does, then its keys after a dash, e.g.
/// `hint_keys(resp, "Stop everything that's playing", "Esc fades; Shift+Esc stops at once")`.
/// egui also makes the text the control's accessible description (and its name, when it
/// has none). The accessibility test (src/app/a11y_tests.rs) checks every control went
/// through one of these.
pub fn hint(resp: Response, text: impl Into<String>) -> Response {
    let text = platform_keys(&text.into());
    record_hint(&resp, true);
    level(&resp);
    resp.on_hover_text(text)
}

/// The command key by the platform's name: "⌘/Ctrl" in a hint (written once for both
/// platforms) reads "⌘" on macOS and "Ctrl" elsewhere. Every hover text goes through it;
/// the CLI docs keep the double form, since they serve both.
pub fn platform_keys(text: &str) -> String {
    let mac = cfg!(target_os = "macos");
    text.replace("⌘/Ctrl", if mac { "⌘" } else { "Ctrl" }).replace("Option/Alt", if mac { "Option" } else { "Alt" })
}

/// [`hint`] with the control's keys: "what it does — keys".
pub fn hint_keys(resp: Response, what: &str, keys: &str) -> Response {
    hint(resp, format!("{what} — {keys}"))
}

/// [`hint`] for a control that can be disabled: `why` says what makes it available, and
/// shows under `text` while it isn't.
pub fn hint_or(resp: Response, text: impl Into<String>, why: &str) -> Response {
    let text = platform_keys(&text.into());
    let why = platform_keys(why);
    record_hint(&resp, !why.is_empty());
    level(&resp);
    if resp.enabled() {
        resp.on_hover_text(text)
    } else {
        let both = format!("{text}\n{why}");
        resp.ctx.accesskit_node_builder(resp.id, |n| n.set_description(both.as_str()));
        resp.on_disabled_hover_text(both)
    }
}

// ── Screen readers ────────────────────────────────────────────────────────────────

/// What a screen reader says a custom-drawn control is (M20): its role, a short label
/// that can change every frame ("Air Horn, tile 5, key 5, hold, playing, 1.2
/// seconds left"), and for a toggle whether it's on. Call it before [`hint`], so the
/// hover text becomes the description and not the name.
pub fn role(resp: Response, role: egui::accesskit::Role, label: impl Into<String>, on: Option<bool>) -> Response {
    let label = label.into();
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(role, resp.enabled(), &label);
        info.selected = on;
        info
    });
    resp
}

/// A custom-drawn slider: its label, its value, and the value as words ("−6.0 dB").
/// A screen reader's increment and decrement come back as `steps`: +1 or −1 each.
pub fn slider_info(resp: Response, label: &str, value: f64, text: String) -> (Response, i32) {
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::accesskit::Role::Slider, resp.enabled(), label);
        info.value = Some(value);
        info.current_text_value = Some(text.clone());
        info
    });
    level(&resp);
    use egui::accesskit::Action;
    let steps = resp.ctx.input(|i| {
        i.num_accesskit_action_requests(resp.id, Action::Increment) as i32 - i.num_accesskit_action_requests(resp.id, Action::Decrement) as i32
    });
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        n.add_action(Action::Increment);
        n.add_action(Action::Decrement);
    });
    (resp, steps)
}

/// egui leaves a slider's orientation unset, and VoiceOver calls a slider without one a
/// "circular slider". Every slider here is horizontal; [`hint`] and [`slider_info`] say so.
fn level(resp: &Response) {
    use egui::accesskit::{Orientation, Role};
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        if n.role() == Role::Slider && n.orientation().is_none() {
            n.set_orientation(Orientation::Horizontal);
        }
    });
}

/// Name the panel `ui` is drawn in, for screen readers: "Toolbar", "Sets", "Board"…
/// egui gives its panels the role Pane, which AccessKit hands VoiceOver as an unknown
/// element, and VoiceOver's cursor never goes into one, so everything in the panel could
/// only be reached with Tab. A toolbar or a named region is a group it reads and enters
/// (and lists as a landmark); Orca reads both too.
pub fn name_panel(ui: &Ui, role: egui::accesskit::Role, label: &str) {
    let Some(panel) = ui.stack().iter().find(|s| s.kind().is_some_and(|k| k.is_panel())) else { return };
    ui.ctx().accesskit_node_builder(panel.unique_id, |n| {
        n.set_role(role);
        n.set_label(label);
    });
}

/// Name the modal `ui` is drawn in ("Import this set file?"), which egui leaves a
/// dialog with no name, so a screen reader says only "dialog".
pub fn name_dialog(ui: &Ui, label: &str) {
    let Some(modal) = ui.stack().iter().find(|s| s.kind() == Some(egui::UiKind::Modal)) else { return };
    ui.ctx().accesskit_node_builder(modal.unique_id, |n| n.set_label(label));
}

/// Give one of egui's own widgets a short name, where it has none or a poor one (a
/// slider beside its row's label, a field named by its suffix). Call before [`hint`].
pub fn name(resp: Response, label: &str) -> Response {
    resp.ctx.accesskit_node_builder(resp.id, |n| n.set_label(label));
    resp
}

/// The focus ring's colour: [`Tokens::focus`] for the look that's showing.
fn focus_color(ctx: &egui::Context) -> Color32 {
    Tokens::get(ctx).focus
}

/// The focused widget drew its own focus cue this pass (a Panel pad's white outline),
/// so [`focus_ring`] leaves it.
pub fn own_focus_cue(ctx: &egui::Context, id: egui::Id) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("leadstinger-own-focus-cue"), (pass, id)));
}

/// What the focus ring went round in the last pass, for tests.
#[cfg(test)]
mod platform_keys_tests {
    #[test]
    fn hover_text_names_this_platforms_command_key() {
        let s = super::platform_keys("Undo — ⌘/Ctrl+Z; ⌘/Ctrl-click adds, Option/Alt-click removes");
        assert!(!s.contains("⌘/Ctrl") && !s.contains("Option/Alt"), "{s}");
        if cfg!(target_os = "macos") {
            assert_eq!(s, "Undo — ⌘+Z; ⌘-click adds, Option-click removes");
        } else {
            assert_eq!(s, "Undo — Ctrl+Z; Ctrl-click adds, Alt-click removes");
        }
    }
}

#[cfg(test)]
pub(crate) fn focus_ring_drawn(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|d| d.get_temp::<Option<egui::Id>>(egui::Id::unique("leadstinger-focus-ring-drawn"))).flatten()
}

/// A custom control's shape, so the focus ring around it follows its corners (a pill's
/// are half its height, a block's square).
pub fn focus_corners(resp: &Response, corners: CornerRadius) {
    if resp.has_focus() {
        resp.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("leadstinger-focus-corners"), (resp.id, corners)));
    }
}

/// The widget the focus ring goes round this pass: the focused one, unless it drew its
/// own cue ([`own_focus_cue`]).
fn focus_ring_target(ctx: &egui::Context) -> Option<Response> {
    let id = ctx.memory(|m| m.focused())?;
    let own = ctx.data(|d| d.get_temp::<(u64, egui::Id)>(egui::Id::unique("leadstinger-own-focus-cue")));
    if own == Some((ctx.cumulative_pass_nr(), id)) {
        return None;
    }
    ctx.read_response(id).filter(|r| r.rect.is_positive())
}

/// A clear ring around whatever has keyboard focus (M20), egui's widgets and the
/// custom-drawn ones alike, drawn on the widget's own layer so windows above cover it.
/// Panel's follows the control's shape, outside it with a black gap, like its selection
/// outline; a Panel pad draws its own (D-215).
pub fn focus_ring(ctx: &egui::Context) {
    let target = focus_ring_target(ctx);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("leadstinger-focus-ring-drawn"), target.as_ref().map(|r| r.id)));
    let Some(r) = target else { return };
    let painter = ctx.layer_painter(r.layer_id).with_clip_rect(r.interact_rect.expand(7.0));
    if is_panel(ctx) {
        let corners = ctx
            .data(|d| d.get_temp::<(egui::Id, CornerRadius)>(egui::Id::unique("leadstinger-focus-corners")))
            .filter(|(at, _)| *at == r.id)
            .map_or(ctx.global_style().visuals.widgets.inactive.corner_radius, |(_, c)| c);
        let width = if is_high_contrast(ctx) { 3.0 } else { 2.0 };
        painter.rect_stroke(r.rect.expand(3.0), corners + 3, Stroke::new(width, focus_color(ctx)), StrokeKind::Outside);
    } else {
        painter.rect_stroke(r.rect.expand(2.0), 5.0, Stroke::new(2.5_f32, focus_color(ctx)), StrokeKind::Outside);
    }
}

/// Keep Left/Right on a focused custom slider (a trim handle, the master meter) rather
/// than letting them move the focus.
pub fn lock_arrows(resp: &Response) {
    let filter = egui::EventFilter { horizontal_arrows: true, ..Default::default() };
    resp.ctx.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
}

/// The controls given hover text this pass, and whether each says why when disabled.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct HintLog(pub std::collections::HashMap<egui::Id, bool>);

#[cfg(test)]
fn hint_log_id() -> egui::Id {
    egui::Id::unique("theme-hint-log")
}

#[cfg(test)]
fn record_hint(resp: &Response, says_why: bool) {
    resp.ctx.data_mut(|d| d.get_temp_mut_or_default::<HintLog>(hint_log_id()).0.insert(resp.id, says_why));
}

#[cfg(not(test))]
fn record_hint(_: &Response, _: bool) {}

/// Forget the last pass's hints; the app calls this at the start of each ui pass.
#[cfg(test)]
pub(crate) fn hint_log_clear(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<HintLog>(hint_log_id()));
}

#[cfg(test)]
pub(crate) fn hint_log(ctx: &egui::Context) -> HintLog {
    ctx.data(|d| d.get_temp::<HintLog>(hint_log_id())).unwrap_or_default()
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

fn style_for(style: &mut egui::Style, t: &Tokens, dark: bool, m: &Metrics) {
    let radius = |r: f32| CornerRadius::same(r.round().clamp(0.0, 255.0) as u8);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0_f32, t.border);
    v.extreme_bg_color = if dark { t.bar } else { lighten(t.panel, 0.6) };
    v.faint_bg_color = t.panel2;
    v.code_bg_color = t.group;
    v.hyperlink_color = t.accent_text();
    v.selection.bg_fill = t.accent_dark;
    v.selection.stroke = Stroke::new(1.0_f32, if dark { Color32::WHITE } else { t.text });
    v.window_corner_radius = radius(m.radius_window);
    // Round slider knobs, as in egui 0.32 and earlier (0.33 made rectangles the default).
    v.handle_shape = egui::style::HandleShape::Circle;
    v.menu_corner_radius = radius(m.radius_menu);
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
        wv.corner_radius = radius(m.radius_control);
        wv.expansion = 0.0;
    }
    w.noninteractive.corner_radius = radius(m.radius_control);
    style.visuals = v;
    style.spacing.item_spacing = m.item_spacing;
    style.spacing.button_padding = m.button_padding;
    style.spacing.interact_size.y = m.control_height;
    style.text_styles = [
        (TextStyle::Small, regular(m.text_small)),
        (TextStyle::Body, regular(m.text_body)),
        (TextStyle::Button, regular(m.text_button)),
        (TextStyle::Heading, semibold(m.text_heading)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
}

/// The Panel widget style: black ground, pill-shaped controls in the theme's colours with
/// black lettering, and Antonio for all text.
fn style_for_panel(style: &mut egui::Style, look: &Look, high_contrast: bool) {
    let t = look.tokens(high_contrast, true);
    let (p, m) = (&look.panel, &look.metrics);
    style_for(style, &t, true, m);
    let v = &mut style.visuals;
    v.window_stroke = Stroke::new(2.0_f32, p.amber);
    v.extreme_bg_color = PANEL_FIELD;
    v.selection.bg_fill = p.amber;
    v.selection.stroke = Stroke::new(1.0_f32, p.black);
    let w = &mut v.widgets;
    // Idle controls are dim lilac with peach lettering (white in high contrast), so the
    // labels of checkboxes and unselected choices (drawn without a fill) stay readable on
    // black; they light up to full colour with black lettering when hovered, pressed or
    // selected.
    let idle = shade(p.lilac, 0.42);
    let lettering = if high_contrast { Color32::WHITE } else { p.peach };
    for (wv, fill, ink) in [
        (&mut w.inactive, idle, lettering),
        (&mut w.hovered, p.peach, p.black),
        (&mut w.active, p.amber, p.black),
        // "Open" also fills window title bars, whose title is peach.
        (&mut w.open, shade(p.amber, 0.38), lettering),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.5_f32, ink);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, lettering);
    // Frames and separators keep Console's small radius; only controls are pills.
    w.noninteractive.corner_radius = CornerRadius::same(CONSOLE_METRICS.radius_control as u8);
    if high_contrast {
        // Dim lilac on black is under 3:1, so idle controls get a full lilac outline,
        // and every control keeps an outline when hovered or pressed; fields are black.
        w.inactive.bg_stroke = Stroke::new(2.0_f32, p.lilac);
        w.hovered.bg_stroke = Stroke::new(2.0_f32, Color32::WHITE);
        w.active.bg_stroke = Stroke::new(2.0_f32, Color32::WHITE);
        v.extreme_bg_color = p.black;
        v.window_stroke = Stroke::new(2.0_f32, Color32::WHITE);
        // Small print in peach, not white faded to grey.
        v.weak_text_color = Some(p.peach);
    }
    style.text_styles = [
        (TextStyle::Small, condensed_semi(m.text_small)),
        (TextStyle::Body, condensed_semi(m.text_body)),
        (TextStyle::Button, condensed(m.text_button)),
        (TextStyle::Heading, condensed(m.text_heading)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
}

/// The theme faces installed last, so restyling only rebuilds the font atlas when they
/// change.
#[derive(Clone)]
struct InstalledFaces([Option<Arc<[u8]>>; 5]);

fn same_faces(a: &[Option<Arc<[u8]>>; 5], b: &[Option<Arc<[u8]>>; 5]) -> bool {
    a.iter().zip(b).all(|(a, b)| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    })
}

/// Install fonts and the theme, and show the appearance asked for.
pub fn apply(ctx: &egui::Context, appearance: Appearance, look: &Arc<Look>) {
    install_fonts(ctx, &look.fonts);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("leadstinger-faces"), InstalledFaces(look.fonts.clone())));
    apply_look(ctx, appearance, look);
}

/// Switch theme (Console, Panel or a user theme) and, for a Console-based one, light or
/// dark.
pub fn apply_look(ctx: &egui::Context, appearance: Appearance, look: &Arc<Look>) {
    let high_contrast = appearance == Appearance::HighContrast;
    let faces_id = egui::Id::unique("leadstinger-faces");
    let installed = ctx.data(|d| d.get_temp::<InstalledFaces>(faces_id));
    if !installed.is_some_and(|f| same_faces(&f.0, &look.fonts)) {
        install_fonts(ctx, &look.fonts);
        ctx.data_mut(|d| d.insert_temp(faces_id, InstalledFaces(look.fonts.clone())));
    }
    // Panel's painters read their colours from the look showing: in high contrast, its
    // high-contrast colours.
    let look = if high_contrast && look.base == ThemeKind::Panel { &Arc::new(look.panel_high_contrast()) } else { look };
    ctx.data_mut(|d| {
        d.insert_temp(look_id(), look.clone());
        d.insert_temp(high_contrast_id(), high_contrast);
    });
    let m = look.metrics;
    match look.base {
        ThemeKind::Console if high_contrast => {
            ctx.style_mut_of(Theme::Dark, |s| style_for(s, &look.high_contrast, true, &m));
            ctx.style_mut_of(Theme::Light, |s| style_for(s, &look.high_contrast, true, &m));
            ctx.set_theme(ThemePreference::Dark);
        }
        ThemeKind::Console => {
            ctx.style_mut_of(Theme::Dark, |s| style_for(s, &look.dark, true, &m));
            ctx.style_mut_of(Theme::Light, |s| style_for(s, &look.light, false, &m));
            ctx.set_theme(match appearance {
                Appearance::Dark | Appearance::HighContrast => ThemePreference::Dark,
                Appearance::Light => ThemePreference::Light,
                Appearance::System => ThemePreference::System,
            });
        }
        ThemeKind::Panel => {
            // Panel is always on black, whatever the system appearance.
            ctx.style_mut_of(Theme::Dark, |s| style_for_panel(s, look, high_contrast));
            ctx.style_mut_of(Theme::Light, |s| style_for_panel(s, look, high_contrast));
            ctx.set_theme(ThemePreference::Dark);
        }
    }
}

/// The window icon: the brand's icon (src/brand.rs `ICON_PNG`).
pub fn app_icon() -> egui::IconData {
    let img = image::load_from_memory_with_format(crate::brand::ICON_PNG, image::ImageFormat::Png)
        .expect("the brand's icon is a PNG (tested)")
        .to_rgba8();
    egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

/// The bars of the waveform along the top of the cassette's label (docs/design/logo.py
/// `WAVES["bars"]`, D-128): each bar's height as a share of its greatest.
const CASSETTE_WAVE: [f32; 11] = [0.42, 0.78, 1.00, 0.58, 0.90, 0.48, 0.74, 0.40, 0.60, 0.34, 0.26];

/// The app's mark, a compact cassette (docs/design/logo.py, D-257), drawn to fit `rect`
/// and centred in it. Its colours are the icon's, the same in every theme. Under 48 px
/// wide it is the icon's small twin (the shell, the trapezoid, the window and two hubs);
/// from there it has the label, the waveform along its top and the tape round the hubs.
pub fn paint_cassette(p: &egui::Painter, rect: Rect) {
    // The design's units: the shell spans x 10..90 and y 24..75.
    let k = (rect.width() / 80.0).min(rect.height() / 51.0);
    let o = rect.center() - vec2(50.0, 49.5) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let (edge, tape, ink) = (hex(0xC46E1E), hex(0x6A3E1E), hex(0x1E1E1E));
    let small = k * 80.0 < 48.0;
    p.rect_filled(r(10.0, 24.0, 90.0, 75.0), 4.5 * k, PANEL.amber);
    let trapezoid = [at(29.0, 63.0), at(71.0, 63.0), at(76.0, 75.0), at(24.0, 75.0)];
    p.add(egui::Shape::convex_polygon(trapezoid.to_vec(), edge, egui::Stroke::NONE));
    let (hubs, hub_y, hub_r) = if small { ([35.0, 65.0], 46.5, 6.8) } else { ([36.5, 63.5], 47.0, 4.4) };
    if small {
        p.rect_filled(r(22.0, 34.0, 78.0, 59.0), 12.5 * k, ink);
    } else {
        p.rect_filled(r(15.0, 28.0, 85.0, 59.0), 2.6 * k, PANEL.peach);
        // The waveform about y = 33.4, at most 4 above and below it, across x 20..80.
        let pitch = 60.0 / CASSETTE_WAVE.len() as f32;
        let w = pitch * 0.56;
        for (i, a) in CASSETTE_WAVE.iter().enumerate() {
            let (x, half) = (20.0 + (i as f32 + 0.5) * pitch, (a * 4.0).max(w / 2.0));
            p.rect_filled(r(x - w / 2.0, 33.4 - half, x + w / 2.0, 33.4 + half), w / 2.0 * k, ink);
        }
        p.rect_filled(r(28.0, 39.0, 72.0, 55.0), 8.0 * k, ink);
        p.circle_filled(at(hubs[0], hub_y), 7.4 * k, tape);
        p.circle_filled(at(hubs[1], hub_y), 5.4 * k, tape);
    }
    for x in hubs {
        p.circle_filled(at(x, hub_y), hub_r * k, PANEL.peach);
        if !small {
            p.circle_filled(at(x, hub_y), 2.5 * k, ink);
        }
    }
}

/// The ink a galley's capitals cover, in the galley's own coordinates (the tail of a Q,
/// which hangs below the line, left out of its height).
pub(crate) fn galley_ink(galley: &egui::Galley) -> Rect {
    let mut ink = Rect::NOTHING;
    for placed in &galley.rows {
        for g in placed.row.glyphs.iter().filter(|g| g.uv_rect.size.x > 0.0) {
            let r = Rect::from_min_size(placed.pos + g.pos.to_vec2() + g.uv_rect.offset, g.uv_rect.size);
            let r = if g.chr == 'Q' { Rect::from_x_y_ranges(r.x_range(), ink.y_range()) } else { r };
            ink = ink.union(r);
        }
    }
    ink
}

/// A context with the app's fonts loaded, and `f` run on its fonts in a pass.
#[cfg(test)]
pub(crate) fn with_fonts<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
    let ctx = egui::Context::default();
    install_fonts(&ctx, &Default::default());
    ctx.begin_pass(Default::default());
    let r = f(&ctx);
    ctx.end_pass().textures_delta.clear();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Antonio's capitals sat 0.08 em low in their line, so lettering centred by its line
    /// looked low (t-m22-panel-polish); the install moves them up by that, Inter's not at
    /// all. Laid out, a line of capitals is centred in its row within a pixel at every
    /// size Panel uses, in both condensed faces.
    #[test]
    fn capitals_sit_in_the_middle_of_their_line() {
        let antonio = caps_offset(ANTONIO_BOLD);
        assert!((antonio - 0.078).abs() < 0.002, "{antonio}");
        assert!(caps_offset(include_bytes!("../assets/fonts/Inter-Regular.ttf")).abs() < 0.002);
        let top = condensed_cap_top();
        assert!((top - 0.217).abs() < 0.002, "{top}");
        with_fonts(|ctx| {
            for size in [13.0, 16.0, 17.0, 22.0, 26.0, 40.0, 54.0] {
                for font in [condensed(size), condensed_semi(size)] {
                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap("SIDEBAR HOTKEYS".into(), font.clone(), Color32::WHITE));
                    let (ink, row) = (galley_ink(&galley), galley.rect);
                    assert!((ink.center().y - row.center().y).abs() <= 1.0, "{font:?}: ink {ink:?} in row {row:?}");
                }
            }
        });
    }

    /// The window icon decodes: 512 px of macOS artwork on macOS (also the running Dock
    /// icon, D-129), the 256 px launcher icon elsewhere.
    #[test]
    fn app_icon_decodes() {
        let icon = app_icon();
        let n = if cfg!(target_os = "macos") { 512 } else { 256 };
        assert_eq!((icon.width, icon.height), (n, n));
        assert_eq!(icon.rgba.len(), (n * n * 4) as usize);
    }

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
            // focus came with M20, after the mockup; the built-in theme files have it.
            // Nor are the status colours (D-120) in it.
            for (name, ours) in tokens.named().into_iter().filter(|(n, _)| *n != "focus" && !n.contains('/')) {
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

    /// Every built-in theme and appearance's text and control colours against WCAG AA:
    /// nothing below it but the pairs D-086 accepted, and none of those in high contrast.
    #[test]
    fn contrast_meets_wcag_aa() {
        let mut unexpected = Vec::new();
        for look in [Look::console(), Look::panel()] {
            if std::env::var_os("CONTRAST_REPORT").is_some() {
                for (name, pairs, _) in contrast::looks(&look) {
                    for f in contrast::failures(&pairs) {
                        eprintln!("{} {name}: {f}{}", look.id, if contrast::is_known(name, &f) { "  (known)" } else { "" });
                    }
                }
            }
            unexpected.extend(contrast::warnings(&look).into_iter().map(|w| format!("{}: {w}", look.id)));
        }
        assert!(unexpected.is_empty(), "pairs below WCAG AA:\n{}", unexpected.join("\n"));
        // An entry that no longer fails is stale: take it off the list.
        for (name, prefix) in contrast::KNOWN_LOW {
            let base = if *name == "Panel" { Look::panel() } else { Look::console() };
            let looks = contrast::looks(&base);
            let pairs = &looks.iter().find(|(l, _, _)| l == name).expect("a look").1;
            assert!(contrast::failures(pairs).iter().any(|f| f.starts_with(prefix)), "{name}: {prefix:?} now passes; remove it from KNOWN_LOW");
        }
    }

    /// Panel's high contrast (t-m22-panel-hc) differs from its designed look where it
    /// counts: black lettering reaches 7:1 on every block, set, pill and key colour; the
    /// lettering on black is white and the dim lettering peach; an empty pad is lettered
    /// white. Colours that already reach 7:1 keep their exact pastel.
    #[test]
    fn panel_high_contrast_is_clearly_higher() {
        let panel = Look::panel();
        let hc = panel.panel_high_contrast();
        let black = Color32::BLACK;
        let mut colours: Vec<(String, Color32, Color32)> = hc.panel.named().into_iter().zip(panel.panel.named()).filter(|((n, _), _)| !["black", "text", "dim"].contains(n)).map(|((n, c), (_, was))| (n.to_owned(), c, was)).collect();
        colours.extend(hc.panel_tiles.iter().zip(panel.panel_tiles).enumerate().map(|(i, (c, was))| (format!("tile {i}"), *c, was)));
        colours.extend(hc.sets.iter().zip(panel.sets).enumerate().map(|(i, (c, was))| (format!("set {i}"), *c, was)));
        for (name, c, was) in &colours {
            assert!(contrast_ratio(black, *c) >= PANEL_HC_INK, "black on {name} {:.2}:1", contrast_ratio(black, *c));
            if contrast_ratio(black, *was) >= PANEL_HC_INK {
                assert_eq!(c, was, "{name} already reached 7:1");
            }
        }
        for name in ["violet", "blue", "red"] {
            let (n, c) = hc.panel.named().into_iter().find(|(n, _)| *n == name).unwrap();
            assert_ne!(Some(c), panel.panel.named().into_iter().find(|(m, _)| *m == n).map(|(_, c)| c), "{name} is paler");
        }
        assert_eq!((hc.panel.text, hc.panel.dim, hc.ink.empty), (Color32::WHITE, PANEL.peach, Color32::WHITE));
        assert_eq!((panel.high_contrast.text, panel.high_contrast.text_dim), (Color32::WHITE, PANEL.peach));
        // What the painters read once it's applied, and back again.
        let ctx = egui::Context::default();
        let look = Arc::new(Look::panel());
        apply_look(&ctx, Appearance::HighContrast, &look);
        assert_eq!((palette(&ctx).text, palette(&ctx).blue), (Color32::WHITE, hc.panel.blue));
        apply_look(&ctx, Appearance::Dark, &look);
        assert_eq!(palette(&ctx), PANEL);
    }

    /// theme::key's lettering (D-185): Panel letters its lilac and amber keys in black,
    /// 4.5:1 and 7:1 in high contrast; Console keeps its text colour and white.
    #[test]
    fn keys_read_in_every_look() {
        let panel = Look::panel();
        for (t, need) in [(panel.dark, 4.5), (panel.high_contrast, 7.0)] {
            assert_eq!((t.key_text(), t.key_on_text()), (Color32::BLACK, Color32::BLACK), "Panel keys are lettered black");
            for (ink, bg) in [(t.key_text(), t.key), (t.key_on_text(), t.accent_dark)] {
                assert!(contrast_ratio(ink, bg) >= need, "{ink:?} on {bg:?}: {:.2}:1", contrast_ratio(ink, bg));
            }
        }
        let console = Look::console();
        for t in [console.dark, console.light, console.high_contrast] {
            assert_eq!((t.key_text(), t.key_on_text()), (t.text, Color32::WHITE), "Console keys are unchanged");
        }
    }

    /// The default pad is dark, with the tile's colour on the title, the waveform and the
    /// border; Coloured pads are the colour all over, in the tile ink (D-152). Each title
    /// keeps its colour's hue, a little brighter than the waveform and lightened only as
    /// far as AA needs, in every appearance. A dark pad has no plates: the title's halo
    /// keeps it legible where it crosses the waveform.
    #[test]
    fn pad_faces_by_mode() {
        let look = Look::console();
        for (i, c) in TILE_ORDER.iter().enumerate() {
            let base = look.tiles[i];
            for hc in [false, true] {
                let dark = pad_face(base, false, hc, TILE_INK, false, false);
                assert_eq!(dark.body, shade(base, PAD_BODY_SHADE), "{c:?}: the old body");
                assert_eq!(pad_face(base, false, hc, TILE_INK, true, false).body, shade(base, PAD_BODY_HOVER_SHADE));
                assert_eq!((dark.border, dark.wave_played), (Some(pad_wave(base)), pad_wave(base)), "{c:?}: border and waveform in its colour");
                assert_eq!((dark.detail, dark.cap_ink, dark.time), (dark.title, dark.title, dark.title));
                assert_eq!(pad_face(base, false, hc, TILE_INK, false, true).time, Color32::WHITE, "the time is white while it plays");
                assert_eq!((dark.plate, dark.halo.a() > 0), (Color32::TRANSPARENT, true), "{c:?}: no plates, a halo");
                let (ground, wave, need) = (pad_title_ground(base), pad_wave(base), if hc { 7.0 } else { 4.5 });
                assert!(contrast_ratio(ground, shade(base, PAD_BODY_HOVER_SHADE)) < contrast_ratio(ground, dark.body), "the wash and hover make it brighter");
                let ratio = contrast_ratio(dark.title, ground);
                assert!(ratio >= need, "{c:?} hc={hc}: {ratio:.2}:1");
                // A little brighter than the waveform, so it reads as lying over it.
                let over_wave = contrast_ratio(dark.title, wave);
                assert!(luminance(dark.title) > luminance(wave) && over_wave >= PAD_TITLE_OVER_WAVE, "{c:?}: {over_wave:.2}:1 over the waveform");
                // Where it crosses the waveform, even one layer of halo keeps its contrast.
                let halo = over(wave, shade(base, PAD_HALO_SHADE), PAD_HALO_ALPHA);
                let crossing = contrast_ratio(dark.title, halo);
                assert!(crossing >= need, "{c:?} hc={hc}: {crossing:.2}:1 on the halo over the waveform");
                // Lightened from the waveform only as far as it takes: one step less would fall short.
                assert_ne!(dark.title, wave);
                let less = (1..=PAD_TITLE_STEPS).map(|n| lighten(wave, n as f32 / PAD_TITLE_STEPS as f32)).take_while(|&c| c != dark.title).last();
                if let Some(less) = less {
                    assert!(contrast_ratio(less, ground) < need || contrast_ratio(less, wave) < PAD_TITLE_OVER_WAVE, "{c:?}: lightened too far");
                }
                if std::env::var_os("CONTRAST_REPORT").is_some() {
                    let hexs = |c: Color32| format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
                    eprintln!("| {c:?} | {hc} | {} | {} | {ratio:.2}:1 | {over_wave:.2}:1 | {crossing:.2}:1 |", hexs(base), hexs(dark.title));
                }

                let colored = pad_face(base, true, hc, TILE_INK, false, false);
                assert_eq!(colored.body, tile_bar_fill(base, hc), "{c:?}: coloured all over");
                assert_eq!(colored.title, tile_ink(colored.body, hc, TILE_INK));
                assert_eq!((colored.border, colored.cap_edge), (None, None));
                assert_eq!((colored.plate, colored.halo), (with_alpha(colored.body, pad_plate_alpha(hc)), Color32::TRANSPARENT), "{c:?}: plates, no halo");
            }
        }
    }

    #[test]
    fn colour_helpers() {
        assert_eq!(shade(hex(0x804020), 0.5), hex(0x402010));
        assert_eq!(lighten(hex(0x000000), 0.5), hex(0x808080));
        assert_eq!(with_alpha(Color32::WHITE, 0.35), Color32::from_rgba_premultiplied(89, 89, 89, 89));
    }
}
