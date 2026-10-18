//! The persisted library: sets of tiles. Every struct is `#[serde(default)]` so files
//! written by older or newer versions still load.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const LIBRARY_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    #[default]
    Toggle,
    Restart,
    Hold,
    Pause,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [PlayMode::Toggle, PlayMode::Restart, PlayMode::Hold, PlayMode::Pause];

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Toggle => "Play / Stop",
            PlayMode::Restart => "Always Restart",
            PlayMode::Hold => "Hold to Play",
            PlayMode::Pause => "Play / Pause",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            PlayMode::Toggle => "Press to start; press again to stop.",
            PlayMode::Restart => "Every press starts again from the beginning.",
            PlayMode::Hold => "Plays while the key or mouse button is held down.",
            PlayMode::Pause => "Press to start; press again to pause or resume.",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileColor {
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Mint,
    Teal,
    Cyan,
    #[default]
    Blue,
    Indigo,
    Purple,
    Pink,
    Rose,
    Crimson,
    Brown,
    Gray,
}

impl TileColor {
    /// Gray stays last: `nth` skips it.
    pub const ALL: [TileColor; 16] = [
        TileColor::Red,
        TileColor::Orange,
        TileColor::Yellow,
        TileColor::Lime,
        TileColor::Green,
        TileColor::Mint,
        TileColor::Teal,
        TileColor::Cyan,
        TileColor::Blue,
        TileColor::Indigo,
        TileColor::Purple,
        TileColor::Pink,
        TileColor::Rose,
        TileColor::Crimson,
        TileColor::Brown,
        TileColor::Gray,
    ];

    /// Cycle through the palette so newly added tiles get varied colours.
    pub fn nth(n: usize) -> TileColor {
        // Skip gray in the rotation; it reads as "disabled".
        TileColor::ALL[n % (TileColor::ALL.len() - 1)]
    }
}

/// Where a tile's picture sits relative to its waveform and progress fill.
/// The title and badges are always drawn on top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageLayer {
    #[default]
    Underlay,
    Overlay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    /// Cover the whole tile, cropping the picture if needed.
    #[default]
    Fill,
    /// Show the whole picture centred inside the tile (good for icons).
    Fit,
}

pub const DEFAULT_IMAGE_OPACITY: f32 = 0.35;

/// The app's visual design. Console is the SoundCraft-style mixing desk; Panel is a retro
/// sci-fi control-panel look (always on black).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    #[default]
    Console,
    /// Shown as [`crate::brand::PANEL_THEME`]. Stored as
    /// "padd", so libraries stay readable by older builds; "panel" is accepted too.
    #[serde(rename = "padd", alias = "panel")]
    Panel,
}

/// Light or dark UI. Dark is the default, as on a mixing desk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    /// Follow the operating system.
    System,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub id: Uuid,
    pub title: String,
    /// File name inside the library's `Media/` folder.
    pub file: Option<String>,
    pub color: TileColor,
    pub hotkey: Option<char>,
    pub mode: PlayMode,
    /// Linear gain, 0.0–1.0.
    pub volume: f32,
    pub looping: bool,
    /// Starting this tile fades out every other playing tile.
    pub solo: bool,
    /// Seconds.
    pub fade_in: f32,
    /// Seconds. Also used when the tile is stopped by the user.
    pub fade_out: f32,
    /// Trim start, seconds into the file.
    pub start: f64,
    /// Trim end, seconds into the file; `None` means the end of the file.
    pub end: Option<f64>,
    /// Cached file duration in seconds, filled in once decoded.
    pub duration: Option<f64>,
    /// Picture file inside the library's `Media/` folder.
    pub image: Option<String>,
    pub image_layer: ImageLayer,
    pub image_fit: ImageFit,
    /// 0.0–1.0.
    pub image_opacity: f32,
    /// The picture is cover art taken from the audio file (not chosen by the user).
    pub image_from_art: bool,
    /// The audio file has been checked for embedded cover art. Stays true after the
    /// user removes the art, so it isn't applied again.
    pub art_checked: bool,
}

impl Default for Tile {
    fn default() -> Self {
        Tile {
            id: Uuid::new_v4(),
            title: "Untitled".into(),
            file: None,
            color: TileColor::default(),
            hotkey: None,
            mode: PlayMode::default(),
            volume: 1.0,
            looping: false,
            solo: false,
            fade_in: 0.0,
            fade_out: 0.0,
            start: 0.0,
            end: None,
            duration: None,
            image: None,
            image_layer: ImageLayer::default(),
            image_fit: ImageFit::default(),
            image_opacity: DEFAULT_IMAGE_OPACITY,
            image_from_art: false,
            art_checked: false,
        }
    }
}

impl Tile {
    /// A copy with a fresh id (and no hotkey, which must stay unique within a set).
    pub fn duplicate(&self) -> Tile {
        Tile { id: Uuid::new_v4(), hotkey: None, ..self.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSet {
    pub id: Uuid,
    pub name: String,
    pub layout: Layout,
    /// Grid columns, 1–12.
    pub columns: u32,
    /// List view: start the next cue when one finishes on its own.
    pub auto_advance: bool,
    pub tiles: Vec<Tile>,
}

impl Default for SoundSet {
    fn default() -> Self {
        SoundSet {
            id: Uuid::new_v4(),
            name: "Untitled Set".into(),
            layout: Layout::default(),
            columns: 5,
            auto_advance: false,
            tiles: Vec::new(),
        }
    }
}

impl SoundSet {
    pub fn named(name: impl Into<String>) -> Self {
        SoundSet { name: name.into(), ..Default::default() }
    }

    /// A deep copy with fresh ids for the set and every tile; hotkeys are kept because
    /// they are unique per set.
    pub fn duplicate(&self) -> SoundSet {
        SoundSet {
            id: Uuid::new_v4(),
            name: format!("{} copy", self.name),
            tiles: self.tiles.iter().map(|t| Tile { id: Uuid::new_v4(), ..t.clone() }).collect(),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub version: u32,
    pub sets: Vec<SoundSet>,
    pub selected_set: Option<Uuid>,
    pub master_volume: f32,
    /// Output device name; `None` follows the system default.
    pub output_device: Option<String>,
    /// Seconds used by Stop All.
    pub stop_all_fade: f32,
    /// Draw each tile's waveform behind its label.
    pub show_waveforms: bool,
    pub appearance: Appearance,
    pub theme: ThemeKind,
    /// Linux: open the window through X11 (XWayland on a Wayland desktop), where file
    /// drops work. Read at launch; `LEADSTINGER_X11` overrides it.
    pub use_x11: bool,
}

impl Default for Library {
    fn default() -> Self {
        Library {
            version: LIBRARY_VERSION,
            sets: Vec::new(),
            selected_set: None,
            master_volume: 1.0,
            output_device: None,
            stop_all_fade: 1.0,
            show_waveforms: true,
            appearance: Appearance::default(),
            theme: ThemeKind::default(),
            use_x11: false,
        }
    }
}

impl Library {
    pub fn starter() -> Library {
        let set = SoundSet::named("Set 1");
        Library { selected_set: Some(set.id), sets: vec![set], ..Default::default() }
    }

    /// Repair invariants after loading: at least one set, a valid selection, sane ranges.
    pub fn normalize(&mut self) {
        if self.sets.is_empty() {
            self.sets.push(SoundSet::named("Set 1"));
        }
        if !self.sets.iter().any(|s| Some(s.id) == self.selected_set) {
            self.selected_set = Some(self.sets[0].id);
        }
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.stop_all_fade = self.stop_all_fade.clamp(0.0, 10.0);
        for set in &mut self.sets {
            set.columns = set.columns.clamp(1, 12);
            for t in &mut set.tiles {
                t.volume = t.volume.clamp(0.0, 1.0);
                t.fade_in = t.fade_in.max(0.0);
                t.fade_out = t.fade_out.max(0.0);
                t.start = t.start.max(0.0);
                t.image_opacity = t.image_opacity.clamp(0.0, 1.0);
            }
        }
    }

    pub fn set_index(&self, id: Uuid) -> Option<usize> {
        self.sets.iter().position(|s| s.id == id)
    }

    pub fn selected_index(&self) -> usize {
        self.selected_set.and_then(|id| self.set_index(id)).unwrap_or(0)
    }

    pub fn locate(&self, tile: Uuid) -> Option<(usize, usize)> {
        self.sets.iter().enumerate().find_map(|(si, s)| {
            s.tiles.iter().position(|t| t.id == tile).map(|ti| (si, ti))
        })
    }

    pub fn tile(&self, id: Uuid) -> Option<&Tile> {
        self.locate(id).map(|(s, t)| &self.sets[s].tiles[t])
    }

    pub fn tile_mut(&mut self, id: Uuid) -> Option<&mut Tile> {
        self.locate(id).map(move |(s, t)| &mut self.sets[s].tiles[t])
    }

    /// True if any tile uses `file` as its audio or its picture.
    pub fn file_in_use(&self, file: &str) -> bool {
        self.sets
            .iter()
            .flat_map(|s| &s.tiles)
            .any(|t| t.file.as_deref() == Some(file) || t.image.as_deref() == Some(file))
    }

    /// Tiles playing `audio_file` that haven't been checked for embedded art yet.
    pub fn needs_art(&self, audio_file: &str) -> bool {
        self.sets.iter().flat_map(|s| &s.tiles).any(|t| t.file.as_deref() == Some(audio_file) && !t.art_checked)
    }

    /// Record the result of checking `audio_file` for cover art: tiles without a
    /// picture get `art_file` as a transparent underlay. Every tile playing the file is
    /// marked checked, so removed art never comes back. Returns true if anything changed.
    pub fn apply_embedded_art(&mut self, audio_file: &str, art_file: Option<&str>) -> bool {
        let mut changed = false;
        for t in self.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
            if t.file.as_deref() != Some(audio_file) || t.art_checked {
                continue;
            }
            t.art_checked = true;
            changed = true;
            if let (Some(art), None) = (art_file, &t.image) {
                t.image = Some(art.to_owned());
                t.image_from_art = true;
                t.image_layer = ImageLayer::Underlay;
                t.image_fit = ImageFit::Fill;
                t.image_opacity = DEFAULT_IMAGE_OPACITY;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn panel_theme_stays_readable_by_older_builds() {
        assert_eq!(serde_json::to_value(ThemeKind::Panel).unwrap(), serde_json::json!("padd"));
        assert_eq!(serde_json::from_value::<ThemeKind>(serde_json::json!("panel")).unwrap(), ThemeKind::Panel);
        assert_eq!(serde_json::from_value::<ThemeKind>(serde_json::json!("padd")).unwrap(), ThemeKind::Panel);
    }

    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut lib = Library::starter();
        lib.sets[0].tiles.push(Tile {
            title: "Airhorn".into(),
            file: Some("a.wav".into()),
            hotkey: Some(';'),
            mode: PlayMode::Hold,
            end: Some(2.5),
            ..Default::default()
        });
        let json = serde_json::to_string(&lib).unwrap();
        let back: Library = serde_json::from_str(&json).unwrap();
        assert_eq!(lib, back);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let lib: Library =
            serde_json::from_str(r#"{"sets":[{"name":"Show","tiles":[{"title":"Hit"}]}],"future_field":1}"#)
                .unwrap();
        let tile = &lib.sets[0].tiles[0];
        assert_eq!(tile.title, "Hit");
        assert_eq!(tile.volume, 1.0);
        assert_eq!(tile.mode, PlayMode::Toggle);
        assert_eq!(lib.sets[0].columns, 5);
        assert_eq!(lib.stop_all_fade, 1.0);
    }

    #[test]
    fn normalize_repairs_selection_and_ranges() {
        let mut lib = Library { sets: vec![], master_volume: 4.0, ..Default::default() };
        lib.normalize();
        assert_eq!(lib.sets.len(), 1);
        assert_eq!(lib.selected_set, Some(lib.sets[0].id));
        assert_eq!(lib.master_volume, 1.0);
    }

    #[test]
    fn duplicate_set_gets_fresh_ids() {
        let mut set = SoundSet::named("A");
        set.tiles.push(Tile::default());
        let copy = set.duplicate();
        assert_ne!(copy.id, set.id);
        assert_ne!(copy.tiles[0].id, set.tiles[0].id);
        assert_eq!(copy.name, "A copy");
    }

    #[test]
    fn embedded_art_applies_once_and_respects_user_pictures() {
        let mut lib = Library::starter();
        let plain = Tile { file: Some("a.mp3".into()), ..Default::default() };
        let chosen = Tile { file: Some("a.mp3".into()), image: Some("mine.png".into()), ..Default::default() };
        let other = Tile { file: Some("b.mp3".into()), ..Default::default() };
        let (p, c, o) = (plain.id, chosen.id, other.id);
        lib.sets[0].tiles.extend([plain, chosen, other]);

        assert!(lib.needs_art("a.mp3"));
        assert!(lib.apply_embedded_art("a.mp3", Some("art.jpg")));
        let t = lib.tile(p).unwrap();
        assert_eq!(t.image.as_deref(), Some("art.jpg"));
        assert!(t.image_from_art && t.art_checked);
        assert_eq!(t.image_layer, ImageLayer::Underlay);
        assert_eq!(lib.tile(c).unwrap().image.as_deref(), Some("mine.png"), "a chosen picture wins");
        assert!(!lib.tile(o).unwrap().art_checked, "other files untouched");
        assert!(lib.file_in_use("art.jpg"));

        // The user removes the art; a later check must not bring it back.
        lib.tile_mut(p).unwrap().image = None;
        assert!(!lib.needs_art("a.mp3"));
        assert!(!lib.apply_embedded_art("a.mp3", Some("art.jpg")));
        assert_eq!(lib.tile(p).unwrap().image, None);

        // A file without art still gets marked checked.
        assert!(lib.apply_embedded_art("b.mp3", None));
        assert!(lib.tile(o).unwrap().art_checked);
        assert_eq!(lib.tile(o).unwrap().image, None);
    }

    #[test]
    fn older_libraries_load_with_picture_defaults() {
        let lib: Library = serde_json::from_str(r#"{"sets":[{"tiles":[{"title":"Hit"}]}]}"#).unwrap();
        let t = &lib.sets[0].tiles[0];
        assert_eq!(t.image, None);
        assert!(!t.art_checked, "existing tiles get checked for art on next load");
        assert_eq!(t.image_opacity, DEFAULT_IMAGE_OPACITY);
        assert!(lib.show_waveforms);
    }

    #[test]
    fn locate_finds_tiles_across_sets() {
        let mut lib = Library::starter();
        let mut second = SoundSet::named("B");
        let tile = Tile::default();
        let id = tile.id;
        second.tiles.push(tile);
        lib.sets.push(second);
        assert_eq!(lib.locate(id), Some((1, 0)));
    }
}
