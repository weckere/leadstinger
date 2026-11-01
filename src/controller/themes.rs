//! User themes as commands: `theme.list`, `theme.use`, `theme.import`, `theme.export`
//! and `theme.remove`, and hot reload of the theme in use (t-m19-themes, D-105–D-107).
//! The file format is src/themes.rs; the window restyles on [`Effect::ApplyLook`].

use super::{bad, CmdError, Controller, Effect, Res, P};
use crate::model::ThemeKind;
use crate::theme::Look;
use crate::themes::{self, Installed};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the theme in use is checked for edits.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// How long the list of installed themes (Settings, View ▸ Theme) is kept before the
/// folder is read again. An import, a removal or a reload reads it at once.
const LIST_FOR: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct State {
    watch: Option<Watch>,
    /// The installed themes and when they were read, for Settings and the menus.
    listed: Option<(Instant, Vec<Installed>)>,
}

/// Hot reload: the user theme's folder and its last fingerprint.
struct Watch {
    dir: PathBuf,
    stamp: u64,
    checked: Instant,
}

/// "3 warnings, e.g. Dark: text on window 1.00:1 (needs 4.5:1)", for a notice.
fn warning_summary(name: &str, warnings: &[String]) -> String {
    match warnings {
        [] => String::new(),
        [one] => format!("Theme {name}: {one}"),
        [first, rest @ ..] => format!("Theme {name}: {first} (and {} more; theme.list shows them)", rest.len()),
    }
}

/// A theme found by name: the look, an installed theme's folder, and its warnings.
type Found = (Arc<Look>, Option<PathBuf>, Vec<String>);

/// A built-in by id or shown name.
fn builtin_named(want: &str) -> Option<Look> {
    let w = want.trim();
    [ThemeKind::Console, ThemeKind::Panel].into_iter().map(Look::builtin).find(|b| {
        w.eq_ignore_ascii_case(&b.id) || w.eq_ignore_ascii_case(&b.name)
    })
}

impl Controller {
    pub(super) fn theme_command(&mut self, id: &str, p: &P) -> Res {
        match id {
            "theme.list" => {
                let current = self.look.id.clone();
                let mut list: Vec<Value> = [Look::console(), Look::panel()]
                    .iter()
                    .map(|l| json!({"id": l.id, "name": l.name, "base": themes::base_name(l.base), "builtin": true, "current": l.id == current}))
                    .collect();
                list.extend(themes::installed(&self.store.data_dir).into_iter().map(|t| {
                    json!({"id": t.id, "name": t.name, "base": t.base, "builtin": false, "path": t.path, "current": t.id == current})
                }));
                Ok(json!({"themes": list, "current": current, "warnings": self.look_warnings}))
            }
            "theme.use" => {
                let want = p.str("theme")?.ok_or_else(|| bad("theme.use: theme is required (theme.list names them)"))?;
                self.use_theme(want)
            }
            "theme.import" => {
                let Some(path) = p.str("path")? else {
                    self.needs_window(id, "a path")?;
                    self.effects.push(Effect::PickTheme);
                    return super::ok();
                };
                self.theme_state.listed = None;
                let imported = themes::import(&self.store.data_dir, Path::new(path))
                    .map_err(|e| CmdError::new("failed", format!("Couldn't import the theme at {path}: {e:#}")))?;
                let used = p.bool("use")?.unwrap_or(true);
                let theme = imported.theme.clone();
                if used || self.look.id == theme.id {
                    self.set_look(imported.look, imported.warnings.clone(), Some(&theme));
                } else if imported.warnings.is_empty() {
                    self.notify(format!("Imported the theme {}", theme.name));
                }
                if !used && !imported.warnings.is_empty() {
                    let text = warning_summary(&theme.name, &imported.warnings);
                    self.notify_error(text);
                }
                Ok(json!({"theme": theme, "replaced": imported.replaced, "used": used, "warnings": imported.warnings}))
            }
            "theme.export" => {
                let (look, dir) = match p.str("theme")? {
                    Some(t) => self.find_theme(t)?,
                    None => (Arc::clone(&self.look), self.current_theme_dir()),
                };
                let Some(path) = p.str("path")? else {
                    self.needs_window(id, "a path")?;
                    self.effects.push(Effect::SaveTheme(look.id.clone()));
                    return super::ok();
                };
                let out = themes::export(&look, dir.as_deref(), Path::new(path)).map_err(|e| CmdError::new("failed", format!("Couldn't export the theme: {e:#}")))?;
                self.notify(format!("Exported the theme {} to {}", look.name, out.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
                Ok(json!({"path": out}))
            }
            "theme.remove" => {
                let want = p.str("theme")?.ok_or_else(|| bad("theme.remove: theme is required"))?;
                if let Some(b) = builtin_named(want) {
                    return Err(bad(format!("{} is built in and can't be removed", b.name)));
                }
                // Found by its folder alone, so a theme too broken to read can still go.
                let t = self.installed_named(want)?;
                self.theme_state.listed = None;
                themes::remove(&self.store.data_dir, &t.id).map_err(|e| CmdError::new("failed", format!("{e:#}")))?;
                if self.look.id == t.id {
                    self.use_builtin(self.look.base);
                }
                self.notify(format!("Removed the theme {}", t.name));
                Ok(json!({"removed": t.id, "current": self.look.id}))
            }
            _ => Err(CmdError::new("unknown", format!("unknown command {id}"))),
        }
    }

    /// The theme a set file brought, just installed (D-173): used at once with
    /// `useTheme`, left as it is with `useTheme: false`, otherwise the window asks
    /// ([`Effect::ConfirmTheme`]). Already the theme in use, it is reloaded and nothing is
    /// asked.
    pub(super) fn set_file_theme(&mut self, imported: themes::Imported, use_theme: Option<bool>) -> Value {
        self.theme_state.listed = None;
        let theme = imported.theme.clone();
        let used = use_theme == Some(true) || self.look.id == theme.id;
        let asked = !used && use_theme.is_none() && self.has_window;
        if used {
            self.set_look(imported.look, imported.warnings.clone(), Some(&theme));
        } else if asked {
            self.announce(format!("The set file brings the theme {}. Use it now?", theme.name));
            self.effects.push(Effect::ConfirmTheme { id: theme.id.clone(), name: theme.name.clone() });
        }
        json!({"id": theme.id, "name": theme.name, "base": theme.base, "path": theme.path, "replaced": imported.replaced, "used": used, "asked": asked, "warnings": imported.warnings})
    }

    /// Every theme to choose from, as (id, name): the built-ins, then the installed ones.
    pub fn theme_choices(&mut self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = [Look::console(), Look::panel()].into_iter().map(|l| (l.id, l.name)).collect();
        out.extend(self.installed_themes());
        out
    }

    /// The installed themes as (id, name), by name (the folder is read at most every
    /// couple of seconds, and at once after an import, a removal or a reload).
    pub fn installed_themes(&mut self) -> Vec<(String, String)> {
        let fresh = self.theme_state.listed.as_ref().is_some_and(|(at, _)| at.elapsed() < LIST_FOR);
        if !fresh {
            self.theme_state.listed = Some((Instant::now(), themes::installed(&self.store.data_dir)));
        }
        self.theme_state.listed.iter().flat_map(|(_, l)| l.iter().map(|t| (t.id.clone(), t.name.clone()))).collect()
    }

    /// The theme a name or id means: a built-in ("console", "panel", or its shown name)
    /// or an installed one, with its folder.
    fn find_theme(&self, want: &str) -> Result<(Arc<Look>, Option<PathBuf>), CmdError> {
        self.find_theme_warned(want).map(|(look, dir, _)| (look, dir))
    }

    /// [`Self::find_theme`] with what reading it warned about.
    fn find_theme_warned(&self, want: &str) -> Result<Found, CmdError> {
        if let Some(b) = builtin_named(want) {
            return Ok((if self.look.id == b.id { Arc::clone(&self.look) } else { Arc::new(b) }, None, Vec::new()));
        }
        let t = self.installed_named(want)?;
        let (look, warnings) = themes::load_dir(&t.path, &t.id).map_err(|e| CmdError::new("failed", format!("Couldn't read the theme {}: {e:#}", t.name)))?;
        Ok((Arc::new(look), Some(t.path), warnings))
    }

    /// An installed theme by id, or by name ignoring case.
    fn installed_named(&self, want: &str) -> Result<Installed, CmdError> {
        let w = want.trim();
        let all = themes::installed(&self.store.data_dir);
        let found = all.iter().find(|t| t.id == w).or_else(|| all.iter().find(|t| t.name.eq_ignore_ascii_case(w)));
        found.cloned().ok_or_else(|| {
            let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
            let known = if names.is_empty() { String::new() } else { format!(" (installed: {})", names.join(", ")) };
            CmdError::new("not_found", format!("No theme called {want:?}{known}"))
        })
    }

    fn current_theme_dir(&self) -> Option<PathBuf> {
        (!self.look.is_builtin()).then(|| themes::themes_dir(&self.store.data_dir).join(&self.look.id))
    }

    /// `theme.use`: switch to a built-in or an installed theme.
    fn use_theme(&mut self, want: &str) -> Res {
        let (look, dir, warnings) = self.find_theme_warned(want)?;
        let installed = dir.map(|path| Installed { id: look.id.clone(), name: look.name.clone(), base: themes::base_name(look.base), path });
        let (id, name, base) = (look.id.clone(), look.name.clone(), themes::base_name(look.base));
        self.set_look(Arc::unwrap_or_clone(look), warnings.clone(), installed.as_ref());
        Ok(json!({"theme": id, "name": name, "base": base, "warnings": warnings}))
    }

    /// `view.theme`, and what a removed or unreadable theme falls back to.
    pub(super) fn use_builtin(&mut self, kind: ThemeKind) {
        self.set_look(Look::builtin(kind), Vec::new(), None);
    }

    /// Make `look` the theme in use, save the choice and restyle.
    fn set_look(&mut self, look: Look, warnings: Vec<String>, installed: Option<&Installed>) {
        let lib = &mut self.store.library;
        lib.theme = look.base;
        lib.theme_name = installed.map(|t| t.id.clone());
        self.store.mark_dirty();
        self.theme_state.watch = installed.map(|t| Watch { dir: t.path.clone(), stamp: themes::stamp(&t.path), checked: Instant::now() });
        if !warnings.is_empty() {
            self.notify_error(warning_summary(&look.name, &warnings));
        }
        self.look_warnings = warnings;
        self.look = Arc::new(look);
        self.effects.push(Effect::ApplyLook);
    }

    /// At start: the library's user theme, or its built-in when that's gone.
    pub(super) fn restore_theme(&mut self) {
        let lib = &self.store.library;
        self.look = Arc::new(Look::builtin(lib.theme));
        let Some(id) = lib.theme_name.clone() else { return };
        let dir = themes::themes_dir(&self.store.data_dir).join(&id);
        match themes::load_dir(&dir, &id) {
            Ok((look, warnings)) => {
                self.theme_state.watch = Some(Watch { stamp: themes::stamp(&dir), dir, checked: Instant::now() });
                self.store.library.theme = look.base;
                self.look_warnings = warnings;
                self.look = Arc::new(look);
            }
            Err(e) => {
                self.store.library.theme_name = None;
                self.store.mark_dirty();
                self.notify_error(format!("Couldn't load the theme {id} ({e:#}); using {}", self.look.name));
            }
        }
    }

    /// Hot reload (D-106): re-read the theme in use when its files change. A theme that
    /// no longer reads keeps the look it had, and says why.
    pub(super) fn poll_theme(&mut self) {
        let Some(w) = &mut self.theme_state.watch else { return };
        if w.checked.elapsed() < WATCH_EVERY {
            return;
        }
        w.checked = Instant::now();
        let stamp = themes::stamp(&w.dir);
        if stamp == w.stamp {
            return;
        }
        w.stamp = stamp;
        let dir = w.dir.clone();
        let id = self.look.id.clone();
        match themes::load_dir(&dir, &id) {
            Ok((look, warnings)) => {
                let changed = *self.look != look;
                if look.base != self.store.library.theme {
                    self.store.library.theme = look.base;
                    self.store.mark_dirty();
                }
                if warnings.is_empty() {
                    if changed {
                        self.notify(format!("Reloaded the theme {}", look.name));
                    }
                } else if warnings != self.look_warnings {
                    self.notify_error(warning_summary(&look.name, &warnings));
                }
                self.look_warnings = warnings;
                if changed {
                    // Its name may have changed too: the menus list it by name.
                    self.theme_state.listed = None;
                    self.look = Arc::new(look);
                    self.effects.push(Effect::ApplyLook);
                }
            }
            Err(e) => self.notify_error(format!("Theme {} not reloaded: {e:#}", self.look.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Engine;
    use crate::store::Store;

    /// Import the example theme, use it, survive a restart, hot-reload an edit, keep the
    /// old look on a broken edit, then remove it (back to Console).
    #[test]
    fn theme_commands_and_hot_reload() {
        let root = std::env::temp_dir().join(format!("sb-themes-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        let mut c = Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test"));
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");

        let r = c.execute("theme.import", json!({"path": example})).unwrap();
        assert_eq!(r["theme"]["id"], "harbor");
        assert_eq!(r["used"], true);
        assert_eq!(c.look.id, "harbor");
        assert_eq!(c.store.library.theme_name.as_deref(), Some("harbor"));
        assert!(c.effects.contains(&Effect::ApplyLook));
        let list = c.execute("theme.list", json!({})).unwrap();
        let ids: Vec<&str> = list["themes"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["console", "panel", "harbor"]);
        assert_eq!(list["current"], "harbor");

        // A built-in by name, and back by name.
        c.execute("theme.use", json!({"theme": "panel"})).unwrap();
        assert_eq!((c.look.id.as_str(), c.store.library.theme, c.store.library.theme_name.clone()), ("panel", ThemeKind::Panel, None));
        c.execute("theme.use", json!({"theme": "Harbor"})).unwrap();
        assert_eq!(c.store.library.theme, ThemeKind::Console);
        assert_eq!(c.execute("theme.use", json!({"theme": "nope"})).unwrap_err().code, "not_found");

        // The library brings it back on the next start.
        c.store.save_now().unwrap();
        drop(c);
        let mut c = Controller::new(Store::open(data.clone()).unwrap(), Engine::disabled("test"));
        assert_eq!(c.look.id, "harbor");

        // An edit is picked up (the menus' list too); a broken one keeps the look and says so.
        assert_eq!(c.installed_themes(), [("harbor".to_owned(), "Harbor".to_owned())]);
        let dir = themes::themes_dir(&data).join("harbor");
        std::fs::write(dir.join(themes::THEME_FILE), r#"{"name": "Harbor Night", "base": "console"}"#).unwrap();
        c.theme_state.watch.as_mut().unwrap().checked -= WATCH_EVERY;
        c.effects.clear();
        c.poll_theme();
        assert_eq!(c.look.name, "Harbor Night");
        assert!(c.effects.contains(&Effect::ApplyLook));
        assert_eq!(c.installed_themes(), [("harbor".to_owned(), "Harbor Night".to_owned())]);
        std::fs::write(dir.join(themes::TOKENS_FILE), "{ not json").unwrap();
        c.theme_state.watch.as_mut().unwrap().checked -= WATCH_EVERY;
        c.poll_theme();
        assert_eq!(c.look.name, "Harbor Night", "a broken edit keeps the look");
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("not reloaded")));

        // Export a built-in, remove the theme in use.
        let out = c.execute("theme.export", json!({"theme": "console", "path": root.join("c.lstheme")})).unwrap();
        assert!(Path::new(out["path"].as_str().unwrap()).is_file());
        assert_eq!(c.execute("theme.remove", json!({"theme": "console"})).unwrap_err().code, "bad_params");
        c.execute("theme.remove", json!({"theme": "harbor"})).unwrap();
        assert!(c.installed_themes().is_empty());
        assert_eq!((c.look.id.as_str(), c.store.library.theme_name.clone()), ("console", None));
        std::fs::remove_dir_all(root).ok();
    }

    /// A set file exported while a user theme is in use brings it along (D-173): the
    /// import installs it and the window asks; `useTheme` answers for a script, and
    /// without a window it's only installed.
    #[test]
    fn a_set_file_brings_its_theme_and_the_window_asks() {
        let root = std::env::temp_dir().join(format!("sb-set-theme-{}", uuid::Uuid::new_v4()));
        let mut c = Controller::new(Store::open(root.join("from")).unwrap(), Engine::disabled("test"));
        let clip = root.join("Ding.wav");
        crate::wav::write_tone(&clip, 8000, 0.2, &[660.0], 0.3).unwrap();
        c.execute("tile.add", json!({"paths": [clip]})).unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
        c.execute("theme.import", json!({"path": example})).unwrap();
        let file = root.join("show.lsset");
        assert_eq!(c.execute("set.export", json!({"path": file})).unwrap()["theme"], "harbor");

        let mut d = Controller::new(Store::open(root.join("to")).unwrap(), Engine::disabled("test"));
        d.effects.clear();
        let r = d.execute("set.import", json!({"path": file})).unwrap();
        assert_eq!((&r["theme"]["id"], &r["theme"]["used"], &r["theme"]["asked"]), (&json!("harbor"), &json!(false), &json!(true)));
        assert_eq!(d.look.id, "console", "not used until the answer");
        assert!(d.effects.contains(&Effect::ConfirmTheme { id: "harbor".into(), name: "Harbor".into() }));
        assert_eq!(d.installed_themes(), [("harbor".to_owned(), "Harbor".to_owned())]);
        assert!(d.notice.as_ref().is_some_and(|n| n.text.contains("and the theme Harbor")));
        // "Use It" runs theme.use.
        d.execute("theme.use", json!({"theme": "harbor"})).unwrap();
        assert_eq!((d.look.id.as_str(), d.store.library.theme_name.as_deref()), ("harbor", Some("harbor")));
        // In use already: reloaded, nothing asked.
        d.effects.clear();
        let r = d.execute("set.import", json!({"path": file})).unwrap();
        assert_eq!((&r["theme"]["used"], &r["theme"]["asked"]), (&json!(true), &json!(false)));
        assert!(!d.effects.iter().any(|e| matches!(e, Effect::ConfirmTheme { .. })));

        // A script answers with useTheme; without a window the theme is only installed.
        let mut e = Controller::new(Store::open(root.join("script")).unwrap(), Engine::disabled("test"));
        let r = e.execute("set.import", json!({"path": file, "useTheme": true})).unwrap();
        assert_eq!((&r["theme"]["used"], e.look.id.as_str()), (&json!(true), "harbor"));
        let mut f = Controller::new(Store::open(root.join("cli")).unwrap(), Engine::disabled("test"));
        f.has_window = false;
        let r = f.execute("set.import", json!({"path": file})).unwrap();
        assert_eq!((&r["theme"]["used"], &r["theme"]["asked"], f.look.id.as_str()), (&json!(false), &json!(false), "console"));
        assert!(themes::themes_dir(&root.join("cli")).join("harbor").is_dir());
        std::fs::remove_dir_all(root).ok();
    }
}
