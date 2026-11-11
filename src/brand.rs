//! The app's names, in one place. LeadStinger is a pun on "lead singer" and "stinger" (a
//! short sound cue). The binaries are `leadstinger` and `leadstinger-cli`.

use std::ffi::OsString;
use std::path::PathBuf;
#[cfg(not(target_os = "ios"))]
use std::path::Path;

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
/// app's earlier name so existing scripts keep working.
pub const ENV_PREFIXES: &[&str] = &["LEADSTINGER", "SOUNDBOARD"];
/// Data folders of the app's earlier names, *moved* on first launch.
pub const MOVE_FROM: &[&str] = &["Soundboard"];
/// The second theme's name in menus and Settings.
pub const PANEL_THEME: &str = "Panel";
/// The Panel theme's own title (D-038). Its middle part is drawn in the accent colour.
pub const PANEL_LOGO: (&str, &str, &str) = ("LeadST", ":", "TNGer");
/// The line under the title in the Panel theme.
pub const PANEL_SUBTITLE: &str = "PADD  ·  Sound cue control";
/// The small readout above "Sets" in the Panel theme.
pub const PANEL_TAG: &str = concat!("V", env!("CARGO_PKG_VERSION"));
/// The app icon: a compact cassette as a dark stroke glyph on a flat amber plate
/// (docs/design/logo.py, D-257 made flat by D-259), drawn by scripts/icons.sh. The window icon is this PNG. On macOS it
/// is also the Dock's and the app switcher's while the app runs, so there it is the
/// macOS artwork, on Apple's smaller plate, the same size as other apps' (D-129).
#[cfg(target_os = "macos")]
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icons/leadstinger-macos.png");
#[cfg(not(target_os = "macos"))]
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icons/leadstinger-256.png");
/// The scalable launcher icon, installed beside the launcher entry (src/launcher.rs).
pub const ICON_SVG: &[u8] = include_bytes!("../assets/linux/leadstinger.svg");
/// The launcher entry, as packaged (assets/linux).
pub const DESKTOP_ENTRY: &str = include_str!("../assets/linux/leadstinger.desktop");

/// The command-line companion.
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
/// moved here (see [`MOVE_FROM`]).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = env("DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "ios")]
    return crate::ios::library_dir();
    #[cfg(not(target_os = "ios"))]
    desktop_data_dir()
}

/// [`data_dir`] on macOS and Linux.
#[cfg(not(target_os = "ios"))]
fn desktop_data_dir() -> PathBuf {
    let dir_for = |name: &str| directories::ProjectDirs::from("", "", name).map(|d| d.data_dir().to_path_buf());
    let Some(dir) = dir_for(FILE_NAME) else {
        return std::env::temp_dir().join(SLUG);
    };
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
#[cfg(not(target_os = "ios"))]
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
    fn logo_spells_the_name() {
        assert_eq!(format!("{}{}{}", LOGO.0, LOGO.1, LOGO.2), APP_NAME);
        assert_eq!(format!("{}{}{}", PANEL_LOGO.0, PANEL_LOGO.1, PANEL_LOGO.2), "LeadST:TNGer");
        assert!(!FILE_NAME.contains(':') && !SLUG.contains(':'));
        assert!(env_name("SILENT").ends_with("_SILENT"));
    }
}
