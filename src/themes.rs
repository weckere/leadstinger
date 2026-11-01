//! User themes (t-m19-themes, D-105): the `.lstheme` format, Console and Panel written
//! in it, and the themes folder in the data directory.
//!
//! A `.lstheme` is a folder, or a zip of one, holding `theme.json` (name, base and
//! optional fonts) and `tokens.json`: a Figma REST variables export, exactly what
//! FigCraft's `variable.export` writes, so a theme is designed there and dropped in.
//! The variable names are the ones docs/design/*tokens.json already use.

use crate::model::ThemeKind;
use crate::theme::{self, panel_hc_tokens, panel_tokens, Look, PanelPalette, Tokens, FACE_KEYS};
use anyhow::{bail, Context, Result};
use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const EXTENSION: &str = "lstheme";
pub const THEME_FILE: &str = "theme.json";
pub const TOKENS_FILE: &str = "tokens.json";

/// The built-ins as files, as `theme.export` writes them (a test keeps them current).
pub const CONSOLE_TOKENS: &str = include_str!("../assets/themes/console.lstheme/tokens.json");
pub const PANEL_TOKENS: &str = include_str!("../assets/themes/panel.lstheme/tokens.json");

/// A theme file, a zip, or everything unpacked, may be no bigger than this.
const MAX_BYTES: u64 = 64 << 20;
const MAX_ENTRIES: usize = 64;

/// `theme.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    /// `console` or `panel`.
    pub base: String,
    /// Font files inside the theme by face: regular, semibold, bold, condensed,
    /// condensedSemi.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fonts: BTreeMap<String, String>,
}

impl Manifest {
    fn base(&self) -> Result<ThemeKind> {
        match self.base.to_ascii_lowercase().as_str() {
            "console" => Ok(ThemeKind::Console),
            "panel" => Ok(ThemeKind::Panel),
            other => bail!("theme.json: base must be console or panel (got {other:?})"),
        }
    }
}

pub fn base_name(kind: ThemeKind) -> &'static str {
    match kind {
        ThemeKind::Console => "console",
        ThemeKind::Panel => "panel",
    }
}

// ── Reading tokens.json ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Raw {
    Color(Color32, f64),
    Float(f64),
    Alias(String),
    Other,
}

struct Var {
    /// By mode name (lowercase, letters and digits only: "highcontrast").
    values: HashMap<String, Raw>,
    default: Option<Raw>,
}

/// The variables of a Figma REST variables export, by name and mode name.
pub struct Variables {
    vars: HashMap<String, Var>,
    /// Variable id → name, for aliases.
    ids: HashMap<String, String>,
    modes: HashSet<String>,
}

/// "High Contrast" → "highcontrast", so mode names match however they're written.
fn mode_key(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

impl Variables {
    /// Read the export: the whole REST response (`{meta: {…}}`) or just its `meta`.
    pub fn parse(text: &str) -> Result<Variables> {
        let root: Value = serde_json::from_str(text).context("tokens.json isn't JSON")?;
        let meta = root.get("meta").unwrap_or(&root);
        let (Some(collections), Some(variables)) =
            (meta.get("variableCollections").and_then(Value::as_object), meta.get("variables").and_then(Value::as_object))
        else {
            bail!("tokens.json isn't a variables export: it needs variableCollections and variables (FigCraft's variable.export writes one)");
        };
        // Mode id → name, and each collection's default mode.
        let mut mode_names = HashMap::new();
        let mut defaults = HashMap::new();
        let mut modes = HashSet::new();
        for (cid, c) in collections {
            for m in c.get("modes").and_then(Value::as_array).into_iter().flatten() {
                if let (Some(id), Some(name)) = (m.get("modeId").and_then(Value::as_str), m.get("name").and_then(Value::as_str)) {
                    mode_names.insert(id.to_owned(), mode_key(name));
                    modes.insert(mode_key(name));
                }
            }
            if let Some(d) = c.get("defaultModeId").and_then(Value::as_str) {
                defaults.insert(c.get("id").and_then(Value::as_str).unwrap_or(cid).to_owned(), d.to_owned());
            }
        }
        let mut vars = HashMap::new();
        let mut ids = HashMap::new();
        for (vid, v) in variables {
            let Some(name) = v.get("name").and_then(Value::as_str) else { continue };
            let kind = v.get("resolvedType").and_then(Value::as_str).unwrap_or("");
            let default_mode = v.get("variableCollectionId").and_then(Value::as_str).and_then(|c| defaults.get(c));
            let mut var = Var { values: HashMap::new(), default: None };
            for (mode_id, value) in v.get("valuesByMode").and_then(Value::as_object).into_iter().flatten() {
                let raw = raw_value(kind, value);
                if Some(mode_id) == default_mode || var.default.is_none() && default_mode.is_none() {
                    var.default = Some(raw.clone());
                }
                if let Some(mode) = mode_names.get(mode_id) {
                    var.values.insert(mode.clone(), raw);
                }
            }
            ids.insert(v.get("id").and_then(Value::as_str).unwrap_or(vid).to_owned(), name.to_owned());
            vars.insert(name.to_owned(), var);
        }
        Ok(Variables { vars, ids, modes })
    }

    pub fn has_mode(&self, mode: &str) -> bool {
        self.modes.contains(mode)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.vars.keys().map(String::as_str)
    }

    /// `name`'s value in `mode` (`None`: its collection's default mode), aliases followed.
    /// Unless `strict`, a variable with no value in that mode gives its default one.
    fn value(&self, name: &str, mode: Option<&str>, strict: bool) -> Option<Raw> {
        let mut name = name.to_owned();
        for _ in 0..8 {
            let var = self.vars.get(&name)?;
            let raw = match mode {
                Some(m) => var.values.get(m).or(if strict { None } else { var.default.as_ref() }),
                None => var.default.as_ref(),
            }?;
            match raw {
                Raw::Alias(id) => name = self.ids.get(id)?.clone(),
                other => return Some(other.clone()),
            }
        }
        None
    }

    fn color(&self, name: &str, mode: Option<&str>, strict: bool, warnings: &mut Vec<String>) -> Option<Color32> {
        match self.value(name, mode, strict)? {
            Raw::Color(c, a) => {
                if a < 0.999 {
                    warnings.push(format!("{name} is {:.0} % opaque; themes are drawn opaque", a * 100.0));
                }
                Some(c)
            }
            _ => {
                warnings.push(format!("{name} should be a colour"));
                None
            }
        }
    }

    /// A translucent colour (the disabled veil): its alpha counts, premultiplied in gamma
    /// space as egui blends ([`theme::with_alpha`]).
    fn color_alpha(&self, name: &str, warnings: &mut Vec<String>) -> Option<Color32> {
        match self.value(name, None, false)? {
            Raw::Color(c, a) => Some(theme::with_alpha(c, a as f32)),
            _ => {
                warnings.push(format!("{name} should be a colour"));
                None
            }
        }
    }

    fn float(&self, name: &str, warnings: &mut Vec<String>) -> Option<f32> {
        match self.value(name, None, false)? {
            Raw::Float(f) => Some(f as f32),
            _ => {
                warnings.push(format!("{name} should be a number"));
                None
            }
        }
    }
}

fn raw_value(kind: &str, v: &Value) -> Raw {
    if v.get("type").and_then(Value::as_str) == Some("VARIABLE_ALIAS") {
        return v.get("id").and_then(Value::as_str).map_or(Raw::Other, |id| Raw::Alias(id.to_owned()));
    }
    match kind {
        "COLOR" => {
            let ch = |k: &str| v.get(k).and_then(Value::as_f64);
            match (ch("r"), ch("g"), ch("b")) {
                (Some(r), Some(g), Some(b)) => {
                    let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    Raw::Color(Color32::from_rgb(byte(r), byte(g), byte(b)), ch("a").unwrap_or(1.0))
                }
                _ => Raw::Other,
            }
        }
        "FLOAT" => v.as_f64().map_or(Raw::Other, Raw::Float),
        _ => Raw::Other,
    }
}

// ── Variables → a Look ────────────────────────────────────────────────────────────

/// The `size/*`, `radius/*` and `space/*` variables, their limits, and where they go.
type MetricField = fn(&mut theme::Metrics) -> &mut f32;
const METRICS: [(&str, f32, f32, MetricField); 12] = [
    ("size/small", 6.0, 48.0, |m| &mut m.text_small),
    ("size/body", 6.0, 48.0, |m| &mut m.text_body),
    ("size/button", 6.0, 48.0, |m| &mut m.text_button),
    ("size/heading", 6.0, 64.0, |m| &mut m.text_heading),
    ("radius/control", 0.0, 40.0, |m| &mut m.radius_control),
    ("radius/window", 0.0, 40.0, |m| &mut m.radius_window),
    ("radius/menu", 0.0, 40.0, |m| &mut m.radius_menu),
    ("space/itemX", 0.0, 40.0, |m| &mut m.item_spacing.x),
    ("space/itemY", 0.0, 40.0, |m| &mut m.item_spacing.y),
    ("space/buttonX", 0.0, 40.0, |m| &mut m.button_padding.x),
    ("space/buttonY", 0.0, 40.0, |m| &mut m.button_padding.y),
    ("space/control", 12.0, 64.0, |m| &mut m.control_height),
];

fn tile_var(i: usize) -> String {
    format!("tile/{:02}", i + 1)
}

fn set_var(i: usize) -> String {
    format!("set/{}", i + 1)
}

const INK_VARS: [&str; 4] = ["ink/tile", "ink/stop", "ink/go", "ink/empty"];

const VEIL_VARS: [&str; 2] = ["disabled/veil", "disabled/stripes"];

/// A generic token's variable name: `prefix` (`ui/` on the Panel base) before the plain
/// names; the status colours' grouped names (`status/error`) go as they are (D-120).
fn token_var(prefix: &str, name: &str) -> String {
    if name.contains('/') { name.to_owned() } else { format!("{prefix}{name}") }
}

/// Read the generic tokens in `mode` over `fallback`, with `prefix` before each plain
/// name ([`token_var`]). Says whether any were given.
fn read_tokens(vars: &Variables, prefix: &str, mode: Option<&str>, strict: bool, fallback: Tokens, warnings: &mut Vec<String>) -> (Tokens, bool) {
    let mut t = fallback;
    let mut any = false;
    for (name, slot) in t.fields_mut() {
        if let Some(c) = vars.color(&token_var(prefix, name), mode, strict, warnings) {
            *slot = c;
            any = true;
        }
    }
    (t, any)
}

/// Build a theme from its variables over the built-in of its base. Missing values fall
/// back variable by variable (D-105); warnings say what was ignored or clamped.
pub fn look_from(id: &str, name: &str, base: ThemeKind, vars: &Variables) -> (Look, Vec<String>) {
    let mut w = Vec::new();
    let builtin = Look::builtin(base);
    let mut look = Look { id: id.to_owned(), name: name.to_owned(), ..builtin.clone() };
    let dark_mode = vars.has_mode("dark").then_some("dark");
    let hc = vars.has_mode("highcontrast");
    let mut known: HashSet<String> = HashSet::new();
    match base {
        ThemeKind::Console => {
            let (dark, any) = read_tokens(vars, "", dark_mode, false, builtin.dark, &mut w);
            look.dark = dark;
            look.light = if vars.has_mode("light") {
                read_tokens(vars, "", Some("light"), false, builtin.light, &mut w).0
            } else if any {
                // A dark-only theme: Light and System show it too.
                dark
            } else {
                builtin.light
            };
            if hc {
                look.high_contrast = read_tokens(vars, "", Some("highcontrast"), true, builtin.high_contrast, &mut w).0;
            }
            known.extend(Tokens::named(&builtin.dark).iter().map(|(n, _)| n.to_string()));
            for (i, slot) in look.tiles.iter_mut().enumerate() {
                *slot = vars.color(&tile_var(i), None, false, &mut w).unwrap_or(*slot);
            }
        }
        ThemeKind::Panel => {
            let mut p: PanelPalette = builtin.panel;
            for (n, slot) in p.fields_mut() {
                *slot = vars.color(n, dark_mode, false, &mut w).unwrap_or(*slot);
                known.insert(n.to_owned());
            }
            look.panel = p;
            let derived = panel_tokens(p);
            look.dark = read_tokens(vars, "ui/", dark_mode, false, derived, &mut w).0;
            look.light = look.dark;
            let hc_derived = panel_hc_tokens(look.dark, p);
            look.high_contrast = if hc { read_tokens(vars, "ui/", Some("highcontrast"), true, hc_derived, &mut w).0 } else { hc_derived };
            known.extend(Tokens::named(&builtin.dark).iter().map(|(n, _)| token_var("ui/", n)));
            for (i, slot) in look.panel_tiles.iter_mut().enumerate() {
                *slot = vars.color(&tile_var(i), None, false, &mut w).unwrap_or(*slot);
            }
            for (i, slot) in look.sets.iter_mut().enumerate() {
                *slot = vars.color(&set_var(i), None, false, &mut w).unwrap_or(*slot);
                known.insert(set_var(i));
            }
        }
    }
    known.extend((0..16).map(tile_var));
    let inks = &mut look.ink;
    for (name, slot) in INK_VARS.iter().zip([&mut inks.tile, &mut inks.stop, &mut inks.go, &mut inks.empty]) {
        *slot = vars.color(name, None, false, &mut w).unwrap_or(*slot);
        known.insert(name.to_string());
    }
    let veil = &mut look.veil;
    for (name, slot) in VEIL_VARS.iter().zip([&mut veil.fill, &mut veil.stripes]) {
        *slot = vars.color_alpha(name, &mut w).unwrap_or(*slot);
        known.insert(name.to_string());
    }
    for (name, lo, hi, field) in METRICS {
        known.insert(name.to_owned());
        if let Some(v) = vars.float(name, &mut w) {
            let clamped = v.clamp(lo, hi);
            if clamped != v {
                w.push(format!("{name} {v} is outside {lo}–{hi}; using {clamped}"));
            }
            *field(&mut look.metrics) = clamped;
        }
    }
    let mut unknown: Vec<&str> = vars.names().filter(|n| !known.contains(*n)).collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        let hint = if base == ThemeKind::Panel { " (Panel's generic tokens take a ui/ prefix)" } else { "" };
        w.push(format!("not used by a {} theme: {}{hint}", base_name(base), unknown.join(", ")));
    }
    (look, w)
}

// ── Writing tokens.json ───────────────────────────────────────────────────────────

/// A colour channel as FigCraft writes it: the byte over 255 in f32, widened.
fn channel(v: u8) -> f64 {
    (v as f32 / 255.0) as f64
}

fn color_value(c: Color32) -> Value {
    json!({"a": 1.0, "b": channel(c.b()), "g": channel(c.g()), "r": channel(c.r())})
}

/// A translucent colour, unmultiplied in gamma space (the inverse of
/// [`Variables::color_alpha`]).
fn alpha_value(c: Color32) -> Value {
    let a = c.a();
    let un = |v: u8| if a == 0 { 0 } else { (v as f32 * 255.0 / a as f32).round().min(255.0) as u8 };
    json!({"a": channel(a), "b": channel(un(c.b())), "g": channel(un(c.g())), "r": channel(un(c.r()))})
}

/// The theme's variables as a Figma REST variables export, laid out the way FigCraft's
/// `variable.export` writes one. Only built-ins are written this way; a user theme
/// exports the files it came with.
pub fn tokens_json(look: &Look) -> String {
    let cid = "VariableCollectionId:1";
    let mode_names: &[&str] = match look.base {
        ThemeKind::Console => &["Dark", "Light", "High Contrast"],
        ThemeKind::Panel => &["Default"],
    };
    let mode_ids: Vec<String> = (1..=mode_names.len()).map(|i| format!("{cid}/mode{i}")).collect();
    // (name, COLOR|FLOAT, a value per mode)
    let mut vars: Vec<(String, &str, Vec<Value>)> = Vec::new();
    let same = |v: Value| vec![v; mode_names.len()];
    match look.base {
        ThemeKind::Console => {
            for (i, (name, _)) in look.dark.named().iter().enumerate().filter(|(_, (n, _))| !n.contains('/')) {
                let per_mode = [look.dark, look.light, look.high_contrast].iter().map(|t| color_value(t.named()[i].1)).collect();
                vars.push((name.to_string(), "COLOR", per_mode));
            }
            for (i, c) in look.tiles.iter().enumerate() {
                vars.push((tile_var(i), "COLOR", same(color_value(*c))));
            }
            for (name, c) in INK_VARS.iter().zip([look.ink.tile, look.ink.stop, look.ink.go]) {
                vars.push((name.to_string(), "COLOR", same(color_value(c))));
            }
        }
        ThemeKind::Panel => {
            for (name, c) in look.panel.named() {
                vars.push((name.to_owned(), "COLOR", same(color_value(c))));
            }
            for (i, c) in look.panel_tiles.iter().enumerate() {
                vars.push((tile_var(i), "COLOR", same(color_value(*c))));
            }
            for (i, c) in look.sets.iter().enumerate() {
                vars.push((set_var(i), "COLOR", same(color_value(*c))));
            }
            vars.push(("ink/empty".to_owned(), "COLOR", same(color_value(look.ink.empty))));
        }
    }
    let mut metrics = look.metrics;
    for (name, _, _, field) in METRICS {
        vars.push((name.to_owned(), "FLOAT", same(json!(*field(&mut metrics) as f64))));
    }
    // The status colours and the veil came later (D-120), so they go last and the others
    // keep their ids. Panel writes them out too: they aren't derived from its palette.
    let looks: &[Tokens] = match look.base {
        ThemeKind::Console => &[look.dark, look.light, look.high_contrast],
        ThemeKind::Panel => &[look.dark],
    };
    for (i, (name, _)) in look.dark.named().iter().enumerate().filter(|(_, (n, _))| n.contains('/')) {
        vars.push((name.to_string(), "COLOR", looks.iter().map(|t| color_value(t.named()[i].1)).collect()));
    }
    for (name, c) in VEIL_VARS.iter().zip([look.veil.fill, look.veil.stripes]) {
        vars.push((name.to_string(), "COLOR", same(alpha_value(c))));
    }
    let var_ids: Vec<String> = (1..=vars.len()).map(|i| format!("VariableID:{i}")).collect();
    // Keys in sorted order, so the output is the same with or without serde_json's
    // preserve_order (FigCraft's export is sorted too).
    let mut variables: Vec<(String, Value)> = vars
        .into_iter()
        .zip(&var_ids)
        .map(|((name, kind, values), id)| {
            let by_mode: Map<String, Value> = mode_ids.iter().cloned().zip(values).collect();
            let v = json!({
                "codeSyntax": {},
                "description": "",
                "hiddenFromPublishing": false,
                "id": id,
                "key": id,
                "name": name,
                "remote": false,
                "resolvedType": kind,
                "scopes": ["ALL_SCOPES"],
                "valuesByMode": sorted(by_mode),
                "variableCollectionId": cid,
            });
            (id.clone(), v)
        })
        .collect();
    variables.sort_by(|a, b| a.0.cmp(&b.0));
    let collection = json!({
        "defaultModeId": mode_ids[0],
        "hiddenFromPublishing": false,
        "id": cid,
        "key": cid,
        "modes": mode_ids.iter().zip(mode_names).map(|(id, n)| json!({"modeId": id, "name": n})).collect::<Vec<_>>(),
        "name": match look.base { ThemeKind::Console => "Console", ThemeKind::Panel => "Panel" },
        "remote": false,
        "variableIds": var_ids,
    });
    let out = json!({
        "error": false,
        "meta": {
            "variableCollections": {cid: collection},
            "variables": Value::Object(variables.into_iter().collect()),
        },
        "status": 200,
    });
    serde_json::to_string_pretty(&out).expect("JSON") + "\n"
}

fn sorted(m: Map<String, Value>) -> Value {
    let mut v: Vec<(String, Value)> = m.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Object(v.into_iter().collect())
}

/// `theme.json` for a theme.
pub fn manifest_json(look: &Look) -> String {
    let m = Manifest { name: look.name.clone(), base: base_name(look.base).to_owned(), fonts: BTreeMap::new() };
    serde_json::to_string_pretty(&m).expect("JSON") + "\n"
}

// ── Theme folders ─────────────────────────────────────────────────────────────────

/// A path inside the theme, with nothing that climbs out of it.
fn inside(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components().all(|c| matches!(c, Component::Normal(_))).then(|| dir.join(rel))
}

/// A font egui can use: it panics on one it can't parse, so check with the same parser.
fn check_font(bytes: &[u8]) -> Result<()> {
    let font = skrifa::FontRef::new(bytes).map_err(|e| anyhow::anyhow!("not a font egui can read ({e})"))?;
    use skrifa::MetadataProvider;
    if font.charmap().map('A').is_none() {
        bail!("it has no Latin letters");
    }
    Ok(())
}

/// Read a theme folder. Errors are what stops it loading; the warnings are about what
/// was ignored, clamped or falls short of WCAG AA (D-107).
pub fn load_dir(dir: &Path, id: &str) -> Result<(Look, Vec<String>)> {
    let manifest: Manifest = serde_json::from_slice(&read_capped(&dir.join(THEME_FILE))?).context("theme.json")?;
    let base = manifest.base()?;
    let name = if manifest.name.trim().is_empty() { id.to_owned() } else { manifest.name.trim().to_owned() };
    let tokens = String::from_utf8(read_capped(&dir.join(TOKENS_FILE))?).context("tokens.json isn't text")?;
    let vars = Variables::parse(&tokens)?;
    let (mut look, mut warnings) = look_from(id, &name, base, &vars);
    for (key, rel) in &manifest.fonts {
        let Some(slot) = FACE_KEYS.iter().position(|(k, _)| k == key) else {
            warnings.push(format!("theme.json: no font face called {key:?} (regular, semibold, bold, condensed, condensedSemi)"));
            continue;
        };
        let Some(path) = inside(dir, rel) else {
            warnings.push(format!("theme.json: font {rel:?} must be inside the theme"));
            continue;
        };
        match read_capped(&path).and_then(|b| check_font(&b).map(|_| b)) {
            Ok(bytes) => look.fonts[slot] = Some(Arc::from(bytes)),
            Err(e) => warnings.push(format!("font {rel}: {e:#}; using the built-in face")),
        }
    }
    warnings.extend(theme::contrast::warnings(&look).into_iter().map(|w| format!("below WCAG AA: {w}")));
    Ok((look, warnings))
}

fn read_capped(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("no {}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()))?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("{} is over {} MB", path.display(), MAX_BYTES >> 20);
    }
    Ok(bytes)
}

/// Where imported themes live: `<data dir>/themes/<id>/`.
pub fn themes_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("themes")
}

/// A folder name for a theme: its name in lowercase ASCII with dashes. The built-ins'
/// ids are kept for them.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_owned();
    match s.as_str() {
        "" => "theme".to_owned(),
        "console" | "panel" => format!("my-{s}"),
        _ => s,
    }
}

/// An installed theme, from its folder's theme.json.
#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub base: &'static str,
    pub path: PathBuf,
}

/// The themes in the data directory, by name.
pub fn installed(data_dir: &Path) -> Vec<Installed> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(themes_dir(data_dir)) else { return out };
    for e in entries.flatten() {
        let id = e.file_name().to_string_lossy().into_owned();
        if id.starts_with('.') || !e.path().is_dir() {
            continue;
        }
        let Ok(bytes) = read_capped(&e.path().join(THEME_FILE)) else { continue };
        let Ok(m) = serde_json::from_slice::<Manifest>(&bytes) else { continue };
        let Ok(base) = m.base() else { continue };
        let name = if m.name.trim().is_empty() { id.clone() } else { m.name.trim().to_owned() };
        out.push(Installed { id, name, base: base_name(base), path: e.path() });
    }
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

/// Where a theme's files are in a folder that is one, or that holds one (a zip often
/// wraps everything in a folder).
fn theme_root(dir: &Path) -> Result<PathBuf> {
    if dir.join(THEME_FILE).is_file() {
        return Ok(dir.to_owned());
    }
    let subs: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.join(THEME_FILE).is_file()).collect();
    match subs.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("there's no theme.json in it"),
        _ => bail!("it holds more than one theme"),
    }
}

/// Copy a theme's files from `src` to `dest`, at most [`MAX_ENTRIES`] of them.
fn copy_tree(src: &Path, dest: &Path, count: &mut usize) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let ty = e.file_type()?;
        if ty.is_dir() {
            copy_tree(&e.path(), &dest.join(&name), count)?;
        } else if ty.is_file() {
            *count += 1;
            if *count > MAX_ENTRIES {
                bail!("it holds more than {MAX_ENTRIES} files");
            }
            std::fs::copy(e.path(), dest.join(&name))?;
        }
    }
    Ok(())
}

/// Unpack a zip into `dir`, keeping every entry inside it.
fn unzip(file: &Path, dir: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(file)?).context("it isn't a folder or a zip")?;
    if zip.len() > MAX_ENTRIES {
        bail!("it holds {} files; at most {MAX_ENTRIES} are allowed", zip.len());
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let skip = rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "__MACOSX" || s.starts_with('.')
        });
        if skip || entry.is_dir() || entry.is_symlink() {
            continue;
        }
        let out = dir.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        total += std::io::copy(&mut (&mut entry).take(MAX_BYTES - total + 1), &mut File::create(&out)?)?;
        if total > MAX_BYTES {
            bail!("it unpacks to more than {} MB", MAX_BYTES >> 20);
        }
    }
    Ok(())
}

/// What `theme.import` installed.
pub struct Imported {
    pub theme: Installed,
    pub look: Look,
    pub warnings: Vec<String>,
    /// A theme with the same id was there and has been replaced.
    pub replaced: bool,
}

/// Install a `.lstheme` folder or zip into the data directory. It's read first, so a
/// theme that doesn't load is never installed; one with the same name is replaced (a
/// designer re-importing after an edit).
pub fn import(data_dir: &Path, src: &Path) -> Result<Imported> {
    let themes = themes_dir(data_dir);
    std::fs::create_dir_all(&themes)?;
    let staging = themes.join(format!(".import-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        if src.is_dir() {
            copy_tree(&theme_root(src)?, &staging, &mut 0)?;
        } else if src.is_file() {
            let unpacked = staging.with_extension("zip");
            let r = unzip(src, &unpacked).and_then(|_| copy_tree(&theme_root(&unpacked)?, &staging, &mut 0));
            std::fs::remove_dir_all(&unpacked).ok();
            r?;
        } else {
            bail!("there's nothing at {}", src.display());
        }
        let (look, _) = load_dir(&staging, "import")?;
        let id = slug(&look.name);
        let dest = themes.join(&id);
        let replaced = dest.exists();
        if replaced {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::rename(&staging, &dest)?;
        let (look, warnings) = load_dir(&dest, &id)?;
        let theme = Installed { id, name: look.name.clone(), base: base_name(look.base), path: dest };
        Ok(Imported { theme, look, warnings, replaced })
    })();
    std::fs::remove_dir_all(&staging).ok();
    result
}

/// Write a theme to `dest`: a zip when it ends in `.lstheme` or `.zip`, otherwise a
/// folder (inside `dest` when that's an existing folder). A built-in is written from
/// its tokens; a user theme's own files are copied as they are.
pub fn export(look: &Look, dir: Option<&Path>, dest: &Path) -> Result<PathBuf> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    match dir {
        Some(dir) => collect(dir, "", &mut files)?,
        None => {
            files.push((THEME_FILE.to_owned(), manifest_json(look).into_bytes()));
            files.push((TOKENS_FILE.to_owned(), tokens_json(look).into_bytes()));
        }
    }
    let zipped = dest.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case("zip"));
    if zipped && !dest.is_dir() {
        let part = dest.with_extension("part");
        let write = || -> Result<()> {
            let mut zip = zip::ZipWriter::new(File::create(&part).with_context(|| format!("creating {}", part.display()))?);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, bytes) in &files {
                zip.start_file(name.as_str(), options)?;
                zip.write_all(bytes)?;
            }
            zip.finish()?;
            Ok(())
        };
        if let Err(e) = write() {
            std::fs::remove_file(&part).ok();
            return Err(e);
        }
        std::fs::rename(&part, dest).with_context(|| format!("saving {}", dest.display()))?;
        return Ok(dest.to_owned());
    }
    let out = if dest.is_dir() { dest.join(format!("{}.{EXTENSION}", slug(&look.name))) } else { dest.to_owned() };
    for (name, bytes) in &files {
        let path = out.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(out)
}

/// A theme folder's files as (path inside it, bytes), by name: what a set file carries
/// of the theme in use (src/setfile.rs).
pub fn files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    collect(dir, "", &mut files)?;
    Ok(files)
}

fn collect(dir: &Path, prefix: &str, files: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if e.file_type()?.is_dir() {
            collect(&e.path(), &rel, files)?;
        } else {
            files.push((rel, read_capped(&e.path())?));
        }
    }
    Ok(())
}

/// Delete an installed theme's folder.
pub fn remove(data_dir: &Path, id: &str) -> Result<()> {
    let dir = themes_dir(data_dir).join(id);
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\']) || !dir.is_dir() {
        bail!("no theme {id:?} is installed");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// A fingerprint of a theme folder's files (names, sizes, modified times), for hot
/// reload: it changes whenever a file is saved.
pub fn stamp(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    fn walk(dir: &Path, h: &mut std::collections::hash_map::DefaultHasher, depth: u8) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(meta) = e.metadata() else { continue };
            e.file_name().hash(h);
            if meta.is_dir() {
                if depth < 3 {
                    walk(&e.path(), h, depth + 1);
                }
            } else {
                meta.len().hash(h);
                meta.modified().ok().hash(h);
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    walk(dir, &mut h, 0);
    h.finish()
}

/// For tests and the metric limits' doc: the defaults are inside the limits.
#[cfg(test)]
fn metrics_in_limits(m: &theme::Metrics) -> bool {
    let mut m = *m;
    METRICS.iter().all(|(_, lo, hi, f)| (*lo..=*hi).contains(f(&mut m)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::TILE_ORDER;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("leadstinger-themes-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Console and Panel written as theme files read back token for token (D-105).
    #[test]
    fn builtins_round_trip() {
        for builtin in [Look::console(), Look::panel()] {
            let vars = Variables::parse(&tokens_json(&builtin)).unwrap();
            let (back, warnings) = look_from(&builtin.id, &builtin.name, builtin.base, &vars);
            assert_eq!(back, builtin, "{} doesn't round-trip", builtin.id);
            assert!(warnings.is_empty(), "{}: {warnings:?}", builtin.id);
            assert!(metrics_in_limits(&builtin.metrics));
        }
    }

    /// The bundled files are what the built-ins write (regenerate with UPDATE_THEMES=1),
    /// and they parse back to the built-ins exactly.
    #[test]
    fn bundled_builtin_files_are_current() {
        for (look, bundled, dir) in [(Look::console(), CONSOLE_TOKENS, "console"), (Look::panel(), PANEL_TOKENS, "panel")] {
            let ours = tokens_json(&look);
            if std::env::var_os("UPDATE_THEMES").is_some() {
                let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("assets/themes/{dir}.lstheme/{TOKENS_FILE}"));
                std::fs::write(path, &ours).unwrap();
                continue;
            }
            assert_eq!(ours, bundled, "assets/themes/{dir}.lstheme/tokens.json is stale: run with UPDATE_THEMES=1");
            let (back, _) = look_from(&look.id, &look.name, look.base, &Variables::parse(bundled).unwrap());
            assert_eq!(back, look);
        }
    }

    /// The design exports in docs/design are valid tokens files as they stand, and give
    /// the built-ins.
    #[test]
    fn design_exports_are_themes() {
        let vars = Variables::parse(include_str!("../docs/design/tokens.json")).unwrap();
        let (look, warnings) = look_from("x", "x", ThemeKind::Console, &vars);
        assert_eq!((look.dark, look.light, look.tiles), (theme::DARK, theme::LIGHT, Look::console().tiles));
        assert!(warnings.is_empty(), "{warnings:?}");
        let vars = Variables::parse(include_str!("../docs/design/panel-tokens.json")).unwrap();
        let (look, warnings) = look_from("x", "x", ThemeKind::Panel, &vars);
        assert_eq!((look.panel, look.dark, look.high_contrast), (theme::PANEL, theme::PANEL_TOKENS, theme::PANEL_HC_TOKENS));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    fn var(id: u32, name: &str, kind: &str, values: Value) -> (String, Value) {
        let id = format!("VariableID:{id}");
        (id.clone(), json!({"id": id, "name": name, "resolvedType": kind, "variableCollectionId": "C", "valuesByMode": values}))
    }

    fn variables_json(modes: &[&str], vars: Vec<(String, Value)>) -> String {
        let modes: Vec<Value> = modes.iter().enumerate().map(|(i, n)| json!({"modeId": format!("m{i}"), "name": n})).collect();
        json!({"meta": {"variableCollections": {"C": {"id": "C", "defaultModeId": "m0", "modes": modes}}, "variables": Value::Object(vars.into_iter().collect())}}).to_string()
    }

    fn rgb(r: u8, g: u8, b: u8) -> Value {
        json!({"r": r as f64 / 255.0, "g": g as f64 / 255.0, "b": b as f64 / 255.0, "a": 1.0})
    }

    /// A small theme: what it names changes, the rest is the base's; a dark-only theme
    /// shows its dark in Light too; high contrast stays the tested built-in; aliases,
    /// metrics and unknown names.
    #[test]
    fn partial_theme_falls_back() {
        let text = variables_json(
            &["Dark"],
            vec![
                var(1, "window", "COLOR", json!({"m0": rgb(0x10, 0x20, 0x30)})),
                var(2, "accent", "COLOR", json!({"m0": {"type": "VARIABLE_ALIAS", "id": "VariableID:3"}})),
                var(3, "brand/orange", "COLOR", json!({"m0": rgb(0xFF, 0x80, 0x00)})),
                var(4, "tile/08", "COLOR", json!({"m0": rgb(1, 2, 3)})),
                var(5, "size/body", "FLOAT", json!({"m0": 99.0})),
                var(6, "radius/control", "FLOAT", json!({"m0": 6.0})),
            ],
        );
        let (look, warnings) = look_from("t", "T", ThemeKind::Console, &Variables::parse(&text).unwrap());
        assert_eq!(look.dark.window, Color32::from_rgb(0x10, 0x20, 0x30));
        assert_eq!(look.dark.accent, Color32::from_rgb(0xFF, 0x80, 0x00), "aliases are followed");
        assert_eq!(look.dark.text, theme::DARK.text, "unnamed tokens are the base's");
        assert_eq!(look.light, look.dark, "a dark-only theme is dark in Light too");
        assert_eq!(look.high_contrast, theme::HIGH_CONTRAST);
        assert_eq!(look.tiles[tile_index_of(crate::model::TileColor::Red)], Color32::from_rgb(1, 2, 3));
        assert_eq!(look.metrics.text_body, 48.0, "clamped");
        assert_eq!(look.metrics.radius_control, 6.0);
        assert!(warnings.iter().any(|w| w.contains("size/body 99")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("brand/orange")), "unknown names are listed: {warnings:?}");
    }

    /// The status colours (D-120): named ones change in their mode, the rest stay the
    /// base's in every look; the veil keeps its alpha; Panel names them without ui/.
    #[test]
    fn status_colours_are_themeable() {
        let text = variables_json(
            &["Dark", "Light"],
            vec![
                var(1, "status/error", "COLOR", json!({"m0": rgb(0xFF, 0x60, 0x50), "m1": rgb(0x90, 0x10, 0x10)})),
                var(2, "mark/vamp", "COLOR", json!({"m0": rgb(0x20, 0xE0, 0xC0)})),
                var(3, "disabled/veil", "COLOR", json!({"m0": {"r": 0.0, "g": 0.0, "b": 0.2, "a": 0.5}})),
            ],
        );
        let (look, warnings) = look_from("t", "T", ThemeKind::Console, &Variables::parse(&text).unwrap());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((look.dark.error, look.light.error), (Color32::from_rgb(0xFF, 0x60, 0x50), Color32::from_rgb(0x90, 0x10, 0x10)));
        assert_eq!((look.dark.vamp, look.light.vamp), (Color32::from_rgb(0x20, 0xE0, 0xC0), Color32::from_rgb(0x20, 0xE0, 0xC0)), "a mode left out takes the default");
        assert_eq!(look.high_contrast, theme::HIGH_CONTRAST, "high contrast stays the tested one");
        for t in [look.dark, look.light] {
            assert_eq!((t.trim, t.marker, t.drop, t.input_voice, t.input_level), (theme::DARK.trim, theme::DARK.marker, theme::DARK.drop, theme::DARK.input_voice, theme::DARK.input_level));
        }
        assert_eq!(look.veil.fill, Color32::from_rgba_premultiplied(0, 0, 26, 128), "premultiplied as egui blends");
        assert_eq!(look.veil.stripes, theme::CONSOLE_VEIL.stripes);

        let text = variables_json(&["Default"], vec![var(1, "status/trim", "COLOR", json!({"m0": rgb(1, 2, 3)}))]);
        let (look, warnings) = look_from("t", "T", ThemeKind::Panel, &Variables::parse(&text).unwrap());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((look.dark.trim, look.high_contrast.trim), (Color32::from_rgb(1, 2, 3), Color32::from_rgb(1, 2, 3)));
        assert_eq!((look.dark.error, look.veil), (theme::DARK.error, theme::PANEL_VEIL));
    }

    fn tile_index_of(c: crate::model::TileColor) -> usize {
        TILE_ORDER.iter().position(|&o| o == c).unwrap()
    }

    /// Panel's generic tokens follow its palette, and ui/ overrides one.
    #[test]
    fn panel_tokens_follow_the_palette() {
        let text = variables_json(
            &["Default"],
            vec![var(1, "amber", "COLOR", json!({"m0": rgb(0x20, 0xC0, 0x40)})), var(2, "ui/border", "COLOR", json!({"m0": rgb(9, 9, 9)}))],
        );
        let (look, _) = look_from("t", "T", ThemeKind::Panel, &Variables::parse(&text).unwrap());
        assert_eq!(look.dark.accent, Color32::from_rgb(0x20, 0xC0, 0x40));
        assert_eq!(look.dark.border, Color32::from_rgb(9, 9, 9));
        assert_eq!(look.high_contrast.border, theme::PANEL.peach, "high contrast derives from the palette");
    }

    /// Import a folder and a zip, list, replace, export and remove; a bad font is skipped
    /// with a warning; low contrast warns but still imports.
    #[test]
    fn import_export_remove() {
        let data = scratch("data");
        let src = scratch("src").join("Night Shift.lstheme");
        std::fs::create_dir_all(src.join("fonts")).unwrap();
        std::fs::write(src.join(THEME_FILE), r#"{"name": "Night Shift", "base": "console", "fonts": {"bold": "fonts/broken.ttf"}}"#).unwrap();
        std::fs::write(src.join("fonts/broken.ttf"), b"not a font").unwrap();
        // Text the colour of the window: below AA everywhere.
        let text = variables_json(&["Dark"], vec![var(1, "text", "COLOR", json!({"m0": rgb(0x1E, 0x1E, 0x1E)}))]);
        std::fs::write(src.join(TOKENS_FILE), &text).unwrap();
        let imported = import(&data, &src).unwrap();
        assert_eq!(imported.theme.id, "night-shift");
        assert!(!imported.replaced);
        assert!(imported.look.fonts[2].is_none());
        assert!(imported.warnings.iter().any(|w| w.contains("broken.ttf")), "{:?}", imported.warnings);
        assert!(imported.warnings.iter().any(|w| w.contains("WCAG") && w.contains("text on window")), "{:?}", imported.warnings);
        assert_eq!(installed(&data).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["night-shift"]);

        let zip = data.join("out.lstheme");
        assert_eq!(super::export(&imported.look, Some(&imported.theme.path), &zip).unwrap(), zip);
        let again = import(&data, &zip).unwrap();
        assert!(again.replaced, "the same name replaces it");
        assert_eq!(again.look, imported.look);

        let folder = super::export(&Look::panel(), None, &data).unwrap();
        assert!(folder.ends_with("my-panel.lstheme") || folder.file_name().is_some());
        let (panel, _) = load_dir(&folder, "panel").unwrap();
        assert_eq!(panel.panel, theme::PANEL);

        remove(&data, "night-shift").unwrap();
        assert!(installed(&data).is_empty());
        assert!(remove(&data, "../x").is_err());
        assert!(import(&data, &data.join("nothing")).is_err());
        std::fs::remove_dir_all(&data).ok();
        std::fs::remove_dir_all(src.parent().unwrap()).ok();
    }

    /// The example theme (examples/themes, made with FigCraft) loads without a warning.
    #[test]
    fn example_theme_is_clean() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
        let (look, warnings) = load_dir(&dir, "harbor").unwrap();
        assert_eq!((look.name.as_str(), look.base), ("Harbor", ThemeKind::Console));
        assert_ne!(look.dark.window, theme::DARK.window);
        assert_eq!((look.dark.error, look.light.marker), (Color32::from_rgb(0xFF, 0x8A, 0x7A), Color32::from_rgb(0x5E, 0x10, 0x50)), "its status colours");
        assert_eq!(look.tiles, Look::console().tiles, "tiles it doesn't name are Console's");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Night Shift!"), "night-shift");
        assert_eq!(slug("Console"), "my-console");
        assert_eq!(slug("  "), "theme");
    }
}
