//! The launcher entry and icon for a copy of the app that isn't installed: an AppImage
//! (D-131). Linux desktops find a window's icon through the launcher entry named after
//! its app id (`<slug>.desktop`, brand::SLUG), so taskbars, docks and launchers (Hyprland's
//! waybar or walker, GNOME, KDE) show the brand's icon only once that entry and the icon
//! are in the user's data folder. An AppImage carries both inside it, where nothing else
//! can see them, so it writes them out:
//!
//! - `applications/<slug>.desktop`: the packaged entry (assets/linux), started with the
//!   AppImage;
//! - `icons/hicolor/scalable/apps/<slug>.svg` and `icons/hicolor/256x256/apps/<slug>.png`.
//!
//! `--register-links` does it (writing only what changed, so a moved AppImage or a new
//! icon is picked up).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The user's data folder: `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data_home() -> Result<PathBuf, String> {
    data_home_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

/// [`data_home`] from the two variables. An empty or relative `XDG_DATA_HOME` counts as
/// unset, as the XDG base directory spec says, so nothing lands in the working folder.
fn data_home_from(xdg: Option<OsString>, home: Option<OsString>) -> Result<PathBuf, String> {
    xdg.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_owned())
}

/// `exe` as a launcher entry's Exec path, refusing characters that entries can't hold
/// unescaped.
pub fn exec_path(exe: &Path) -> Result<&str, String> {
    exe.to_str()
        .filter(|p| !p.contains(['"', '`', '$', '\\', '%', '\n']))
        .ok_or_else(|| format!("{} has characters a launcher entry can't hold", exe.display()))
}

/// The program a launcher entry should start, for `exe` (this process's own file). A Nix
/// package runs `bin/.<name>-wrapped` behind a `bin/<name>` wrapper script (libraries),
/// itself behind another package's wrapper that puts ffmpeg on PATH (flake.nix
/// `withFfmpeg`). So the entry names the first `<name>` on `path` (a profile's bin) that
/// is, or is a small script that starts, our wrapper; which also outlives an upgrade.
/// Without one, the wrapper itself.
pub fn program_path(exe: &Path, path: Option<&std::ffi::OsStr>) -> PathBuf {
    let wrapper = exe
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('.')?.strip_suffix("-wrapped"))
        .map(|n| exe.with_file_name(n))
        .filter(|w| w.is_file());
    let Some(wrapper) = wrapper else { return exe.to_owned() };
    let real = std::fs::canonicalize(&wrapper).unwrap_or_else(|_| wrapper.clone());
    let name = wrapper.file_name().unwrap_or_default();
    let starts_ours = |p: &Path| {
        let Ok(c) = std::fs::canonicalize(p) else { return false };
        c == real
            || (std::fs::metadata(&c).is_ok_and(|m| m.len() < 64 * 1024)
                && std::fs::read(&c).is_ok_and(|text| String::from_utf8_lossy(&text).contains(&*real.to_string_lossy())))
    };
    path.into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(name))
        .find(|p| starts_ours(p))
        .unwrap_or(wrapper)
}

/// The packaged launcher entry with its Exec line starting `exe`.
pub fn entry_for(exe: &Path) -> Result<String, String> {
    let exe = exec_path(exe)?;
    Ok(crate::brand::DESKTOP_ENTRY
        .lines()
        .map(|l| if l.starts_with("Exec=") { format!("Exec=\"{exe}\" %u") } else { l.to_owned() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

/// Write the launcher entry for `exe` and the icons under `data`, each only if it differs
/// from what is there. Returns the files written.
pub fn install(data: &Path, exe: &Path) -> Result<Vec<PathBuf>, String> {
    let slug = crate::brand::SLUG;
    let files: [(PathBuf, Vec<u8>); 3] = [
        (data.join("applications").join(format!("{slug}.desktop")), entry_for(exe)?.into_bytes()),
        (data.join("icons/hicolor/scalable/apps").join(format!("{slug}.svg")), crate::brand::ICON_SVG.to_vec()),
        // ICON_PNG is the 256 px launcher icon on Linux, where this runs.
        (data.join("icons/hicolor/256x256/apps").join(format!("{slug}.png")), crate::brand::ICON_PNG.to_vec()),
    ];
    let mut written = Vec::new();
    for (path, bytes) in files {
        if std::fs::read(&path).is_ok_and(|old| old == bytes) {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("can't make {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_nix_wrapped_program_is_started_through_its_wrapper() {
        let root = std::env::temp_dir().join(format!("sb-wrapped-{}", uuid::Uuid::new_v4()));
        let (store, profile) = (root.join("store/bin"), root.join("profile/bin"));
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        let wrapped = store.join(".app-wrapped");
        std::fs::write(&wrapped, "").unwrap();
        // No wrapper beside it: the program itself.
        assert_eq!(program_path(&wrapped, None), wrapped);
        std::fs::write(store.join("app"), "").unwrap();
        assert_eq!(program_path(&wrapped, None), store.join("app"));
        // A profile's bin leading to the same wrapper wins; an unrelated `app` on PATH doesn't.
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("app"), "").unwrap();
        std::os::unix::fs::symlink(store.join("app"), profile.join("app")).unwrap();
        let path = std::env::join_paths([&other, &profile]).unwrap();
        assert_eq!(program_path(&wrapped, Some(&path)), profile.join("app"));
        // An outer wrapper (the ffmpeg one) that starts ours counts too, first on PATH.
        let outer = root.join("outer/bin");
        std::fs::create_dir_all(&outer).unwrap();
        let real = std::fs::canonicalize(store.join("app")).unwrap();
        std::fs::write(outer.join("app"), format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", real.display())).unwrap();
        let path = std::env::join_paths([&other, &outer, &profile]).unwrap();
        assert_eq!(program_path(&wrapped, Some(&path)), outer.join("app"));
        // An ordinary program (or an AppImage) is left as it is.
        assert_eq!(program_path(&other.join("app"), Some(&path)), other.join("app"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_empty_or_relative_xdg_data_home_is_unset() {
        let home = Some(OsString::from("/home/c"));
        let at = |xdg: Option<&str>| data_home_from(xdg.map(OsString::from), home.clone()).unwrap();
        assert_eq!(at(Some("/x/share")), PathBuf::from("/x/share"));
        for xdg in [None, Some(""), Some("share")] {
            assert_eq!(at(xdg), PathBuf::from("/home/c/.local/share"), "{xdg:?}");
        }
        assert!(data_home_from(Some(OsString::new()), None).is_err());
    }

    #[test]
    fn installs_the_entry_and_icons_once() {
        let data = std::env::temp_dir().join(format!("sb-launcher-{}", uuid::Uuid::new_v4()));
        let exe = Path::new("/home/me/Apps/My App.AppImage");
        let written = install(&data, exe).unwrap();
        assert_eq!(written.len(), 3);
        let slug = crate::brand::SLUG;
        let entry = std::fs::read_to_string(data.join(format!("applications/{slug}.desktop"))).unwrap();
        assert!(entry.contains("Exec=\"/home/me/Apps/My App.AppImage\" %u\n"), "{entry}");
        assert!(entry.contains(&format!("Icon={slug}\n")) && entry.contains(&format!("StartupWMClass={slug}\n")), "{entry}");
        assert_eq!(entry.matches("Exec=").count(), 1);
        assert!(std::fs::read(data.join(format!("icons/hicolor/scalable/apps/{slug}.svg"))).unwrap().starts_with(b"<svg"));
        assert!(data.join(format!("icons/hicolor/256x256/apps/{slug}.png")).is_file());
        // Unchanged: nothing written. A moved AppImage: only the entry.
        assert!(install(&data, exe).unwrap().is_empty());
        assert_eq!(install(&data, Path::new("/opt/My.AppImage")).unwrap().len(), 1);
        assert!(install(&data, Path::new("/tmp/$x")).is_err());
        std::fs::remove_dir_all(data).ok();
    }
}
