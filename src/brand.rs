//! The app's names, in one place, for the two brands it builds as:
//!
//! - **LeadStinger** (the default): the public release. A pun on "lead singer" and
//!   "stinger" (a short sound cue).
//! - **ST:TNGer** (`--features sttnger`): a private gift edition, a pun on "stinger"
//!   and a TV series' initials. Its colon belongs in what people read; file names, the
//!   bundle and the data folder leave it out, because macOS shows a ":" in a file name
//!   as "/" and a colon is awkward in shell commands (D-023).
//!
//! The binaries are `leadstinger` and `leadstinger-cli` in both builds (D-036).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(not(feature = "sttnger"))]
mod names {
    /// Shown in the window title, the logo, the macOS menu bar and About.
    pub const APP_NAME: &str = "LeadStinger";
    /// The parts of the logo; the middle part is drawn in the accent colour.
    pub const LOGO: (&str, &str, &str) = ("Lead", "Stinger", "");
    /// macOS bundle and data-folder name.
    pub const FILE_NAME: &str = "LeadStinger";
    /// The Wayland app id / X11 class, log prefix and short name.
    pub const SLUG: &str = "leadstinger";
    /// macOS bundle identifier.
    pub const BUNDLE_ID: &str = "io.github.weckere.leadstinger";
    /// Environment variable prefixes, newest first: `LEADSTINGER_DATA_DIR`, then the
    /// older names so existing scripts keep working.
    pub const ENV_PREFIXES: &[&str] = &["LEADSTINGER", "STTNGER", "SOUNDBOARD"];
    /// Data folders of other names whose library is *copied* on first launch (the gift
    /// build may still be using it).
    pub const COPY_FROM: &[&str] = &["STTNGer"];
    /// Data folders of the app's earlier names, *moved* on first launch.
    pub const MOVE_FROM: &[&str] = &["Soundboard"];
    /// The second theme's name in menus and Settings.
    pub const PANEL_THEME: &str = "Panel";
    /// The Panel theme's own title: the brand with a nod to the PADD look it comes from
    /// (D-038). Its middle part is drawn in the accent colour.
    pub const PANEL_LOGO: (&str, &str, &str) = ("LeadST", ":", "TNGer");
    /// The line under the title in the Panel theme.
    pub const PANEL_SUBTITLE: &str = "PADD  ·  Sound cue control";
    /// The small readout above "Sets" in the Panel theme.
    pub const PANEL_TAG: &str = concat!("V", env!("CARGO_PKG_VERSION"));
}

#[cfg(feature = "sttnger")]
mod names {
    pub const APP_NAME: &str = "ST:TNGer";
    pub const LOGO: (&str, &str, &str) = ("ST", ":", "TNGer");
    pub const FILE_NAME: &str = "STTNGer";
    pub const SLUG: &str = "sttnger";
    pub const BUNDLE_ID: &str = "local.sttnger";
    /// Its own prefix first; LEADSTINGER_ too, so a setting meant for either build (a
    /// test's data folder, say) never silently falls back to the real library.
    pub const ENV_PREFIXES: &[&str] = &["STTNGER", "LEADSTINGER", "SOUNDBOARD"];
    pub const COPY_FROM: &[&str] = &[];
    pub const MOVE_FROM: &[&str] = &["Soundboard"];
    pub const PANEL_THEME: &str = "PADD (LCARS)";
    pub const PANEL_LOGO: (&str, &str, &str) = LOGO;
    pub const PANEL_SUBTITLE: &str = "PADD  ·  Sound cue control";
    pub const PANEL_TAG: &str = "02-1987";
}

pub use names::*;

/// The command-line companion (the same in both brands).
pub const CLI_NAME: &str = "leadstinger-cli";

/// An environment setting, e.g. `env("DATA_DIR")` reads `LEADSTINGER_DATA_DIR`, falling
/// back to the older prefixes in [`ENV_PREFIXES`].
pub fn env(name: &str) -> Option<OsString> {
    ENV_PREFIXES.iter().find_map(|p| std::env::var_os(format!("{p}_{name}")))
}

pub fn env_string(name: &str) -> Option<String> {
    env(name).and_then(|v| v.into_string().ok())
}

/// The name to show for a setting, e.g. `env_name("SILENT")` = `LEADSTINGER_SILENT`.
pub fn env_name(name: &str) -> String {
    format!("{}_{name}", ENV_PREFIXES[0])
}

/// The per-user data folder: `~/Library/Application Support/<FILE_NAME>` on macOS,
/// `~/.local/share/<slug>` on Linux. On first launch a library from an earlier name is
/// copied or moved here (see [`COPY_FROM`], [`MOVE_FROM`]).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = env("DATA_DIR") {
        return PathBuf::from(dir);
    }
    let dir_for = |name: &str| directories::ProjectDirs::from("", "", name).map(|d| d.data_dir().to_path_buf());
    let Some(dir) = dir_for(FILE_NAME) else {
        return std::env::temp_dir().join(SLUG);
    };
    for name in COPY_FROM {
        let Some(other) = dir_for(name) else { continue };
        match copy_library(&other, &dir) {
            Ok(true) => eprintln!("{SLUG}: copied your library from {} to {}", other.display(), dir.display()),
            Ok(false) => {}
            Err(e) => eprintln!("{SLUG}: couldn't copy the library from {}: {e}", other.display()),
        }
    }
    for name in MOVE_FROM {
        let Some(legacy) = dir_for(name) else { continue };
        match migrate(&legacy, &dir) {
            Ok(true) => eprintln!("{SLUG}: moved your library from {} to {}", legacy.display(), dir.display()),
            Ok(false) => {}
            Err(e) => eprintln!("{SLUG}: couldn't move the library from {}: {e}", legacy.display()),
        }
    }
    dir
}

/// Move `legacy` to `new` if only the legacy folder exists. Returns whether it moved.
fn migrate(legacy: &Path, new: &Path) -> std::io::Result<bool> {
    if new.exists() || !legacy.join("Library.json").exists() {
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(legacy, new)?;
    Ok(true)
}

/// Copy the library in `from` (Library.json and Media/) to `to` if `to` has none yet.
/// Returns whether it copied. Lock files and sockets stay behind.
fn copy_library(from: &Path, to: &Path) -> std::io::Result<bool> {
    if to.join("Library.json").exists() || !from.join("Library.json").exists() {
        return Ok(false);
    }
    // Copy into a temporary folder and rename it into place, so a half-finished copy
    // never looks like a library.
    let tmp = to.with_extension("copying");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(tmp.join("Media"))?;
    std::fs::copy(from.join("Library.json"), tmp.join("Library.json"))?;
    if let Ok(entries) = std::fs::read_dir(from.join("Media")) {
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::copy(entry.path(), tmp.join("Media").join(entry.file_name()))?;
            }
        }
    }
    if to.exists() {
        // An empty folder made by an earlier launch: keep anything in it.
        for entry in std::fs::read_dir(to)? {
            let entry = entry?;
            let dest = tmp.join(entry.file_name());
            if !dest.exists() {
                std::fs::rename(entry.path(), dest)?;
            }
        }
        std::fs::remove_dir_all(to)?;
    }
    std::fs::rename(&tmp, to)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_only_a_real_legacy_library_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!("sb-brand-{}", uuid::Uuid::new_v4()));
        let (old, new) = (root.join("Soundboard"), root.join("New"));
        std::fs::create_dir_all(&old).unwrap();
        assert!(!migrate(&old, &new).unwrap(), "no Library.json: nothing to move");
        std::fs::write(old.join("Library.json"), b"{}").unwrap();
        assert!(migrate(&old, &new).unwrap());
        assert!(new.join("Library.json").exists() && !old.exists());
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("Library.json"), b"{}").unwrap();
        assert!(!migrate(&old, &new).unwrap(), "an existing new library is never replaced");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn copies_a_library_once_and_leaves_the_original() {
        let root = std::env::temp_dir().join(format!("sb-brand-{}", uuid::Uuid::new_v4()));
        let (from, to) = (root.join("STTNGer"), root.join("LeadStinger"));
        std::fs::create_dir_all(from.join("Media")).unwrap();
        std::fs::write(from.join("Library.json"), b"{\"sets\":[]}").unwrap();
        std::fs::write(from.join("Media").join("a.wav"), b"RIFF").unwrap();
        std::fs::write(from.join("library.lock"), b"").unwrap();
        // An earlier launch may have left an empty folder with a lock in it.
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("library.lock"), b"").unwrap();
        assert!(copy_library(&from, &to).unwrap());
        assert!(to.join("Library.json").exists() && to.join("Media/a.wav").exists() && to.join("library.lock").exists());
        assert!(from.join("Library.json").exists(), "the original stays");
        std::fs::write(from.join("Library.json"), b"{\"changed\":1}").unwrap();
        assert!(!copy_library(&from, &to).unwrap(), "only once");
        assert_eq!(std::fs::read(to.join("Library.json")).unwrap(), b"{\"sets\":[]}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn logo_spells_the_name() {
        assert_eq!(format!("{}{}{}", LOGO.0, LOGO.1, LOGO.2), APP_NAME);
        assert!(format!("{}{}{}", PANEL_LOGO.0, PANEL_LOGO.1, PANEL_LOGO.2).to_uppercase().ends_with("ST:TNGER"));
        assert!(!FILE_NAME.contains(':') && !SLUG.contains(':'));
        assert!(env_name("SILENT").ends_with("_SILENT"));
    }
}
