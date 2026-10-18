//! Paste to import: files copied in a file manager, read from the system clipboard.
//!
//! winit 0.30 delivers no file drops under Wayland, so on Hyprland and other Wayland
//! desktops copy-and-paste is how files get in without a file picker. egui's clipboard
//! only carries text, so the file list is read here per platform:
//! - macOS: the general pasteboard's file URLs (Finder's Copy).
//! - Linux on Wayland: `wl-paste`, then on X11 `xclip`, asking for `text/uri-list`
//!   (what Nautilus, Dolphin, Thunar, Nemo and PCManFM offer) and GNOME's
//!   `x-special/gnome-copied-files`. The Nix package puts both tools on PATH.
//!
//! Plain text that names existing files (absolute paths or `file://` URIs, one per
//! line) works everywhere as a fallback.

use std::path::{Path, PathBuf};

/// Files on the clipboard. `x11` is true when the window runs on X11 (or XWayland).
pub fn clipboard_files(x11: bool, pasted_text: Option<&str>) -> Vec<PathBuf> {
    let mut files = platform_files(x11);
    if files.is_empty() {
        if let Some(text) = pasted_text {
            files = paths_in_text(text);
        }
    }
    files
}

#[cfg(target_os = "macos")]
fn platform_files(_x11: bool) -> Vec<PathBuf> {
    use objc2::{msg_send, rc::Retained, ClassType};
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
    // objc2-app-kit's `generalPasteboard()` panics on NULL, which is what AppKit returns
    // when there's no pasteboard service (the Nix build sandbox, a bare ssh session).
    // Asking for an Option makes that an empty clipboard instead.
    // SAFETY: `+generalPasteboard` takes no arguments and returns an NSPasteboard or nil.
    let board: Option<Retained<NSPasteboard>> = unsafe { msg_send![NSPasteboard::class(), generalPasteboard] };
    let Some(items) = board.and_then(|b| b.pasteboardItems()) else { return Vec::new() };
    // SAFETY: a constant NSString exported by AppKit.
    let file_url = unsafe { NSPasteboardTypeFileURL };
    items
        .iter()
        .filter_map(|item| item.stringForType(file_url))
        .filter_map(|s| uri_to_path(&s.to_string()))
        .collect()
}

/// Reads one clipboard type with one tool (wl-paste or xclip).
#[cfg(not(target_os = "macos"))]
type ClipboardReader<'a> = &'a dyn Fn(&str) -> Option<String>;

#[cfg(not(target_os = "macos"))]
fn platform_files(x11: bool) -> Vec<PathBuf> {
    // Ask the window's own clipboard first, then the other one: on a Wayland desktop an
    // XWayland window can still read the Wayland clipboard with wl-paste, and XWayland
    // only mirrors it to X11 once an X window has had focus.
    let wl = |mime: &str| run("wl-paste", &["--no-newline", "--type", mime]);
    let xc = |mime: &str| run("xclip", &["-selection", "clipboard", "-target", mime, "-out"]);
    let wayland_session = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let readers: Vec<ClipboardReader> = match (x11, wayland_session) {
        (false, true) => vec![&wl, &xc],
        (true, true) => vec![&xc, &wl],
        (_, false) => vec![&xc],
    };
    for read in readers {
        for mime in ["text/uri-list", "x-special/gnome-copied-files"] {
            if let Some(files) = read(mime).map(|t| parse_uri_list(&t)).filter(|f| !f.is_empty()) {
                return files;
            }
        }
    }
    Vec::new()
}

/// Run a clipboard tool and return its output, or None if it's missing or fails (for
/// example when the clipboard doesn't offer that type).
#[cfg(not(target_os = "macos"))]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Paths from a `text/uri-list` (RFC 2483) or GNOME's copied-files list, whose first
/// line is `copy` or `cut`. Only local `file:` URIs count.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && *l != "copy" && *l != "cut")
        .filter_map(uri_to_path)
        .collect()
}

/// Pasted plain text: every line must be an existing absolute path or a `file:` URI,
/// otherwise it's ordinary text and nothing is imported.
pub fn paths_in_text(text: &str) -> Vec<PathBuf> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let paths: Vec<PathBuf> = lines
        .iter()
        .filter_map(|l| if l.starts_with("file:") { uri_to_path(l) } else { Some(PathBuf::from(l)) })
        .filter(|p| p.is_absolute() && p.exists())
        .collect();
    if paths.len() == lines.len() {
        paths
    } else {
        Vec::new()
    }
}

/// `file:///a/b%20c` or `file://localhost/a/b` → `/a/b c`. Other hosts and schemes are
/// not local files.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = if rest.starts_with('/') {
        rest
    } else {
        rest.strip_prefix("localhost")?
    };
    let decoded = percent_decode(path)?;
    Some(Path::new(&decoded).to_path_buf())
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uri_lists() {
        let list = "# from Dolphin\r\nfile:///home/someone/Clips/Rimshot.mp3\r\nfile://localhost/tmp/a.wav\r\nhttps://example.com/x.mp3\r\n";
        assert_eq!(
            parse_uri_list(list),
            vec![PathBuf::from("/home/someone/Clips/Rimshot.mp3"), PathBuf::from("/tmp/a.wav")]
        );
        let gnome = "copy\nfile:///home/someone/air%20horn.flac";
        assert_eq!(parse_uri_list(gnome), vec![PathBuf::from("/home/someone/air horn.flac")]);
        assert_eq!(parse_uri_list("file://otherhost/x.wav"), Vec::<PathBuf>::new());
    }

    #[test]
    fn decodes_utf8_and_rejects_bad_escapes() {
        assert_eq!(uri_to_path("file:///tmp/caf%C3%A9.wav"), Some(PathBuf::from("/tmp/café.wav")));
        assert_eq!(uri_to_path("file:///tmp/100%"), None);
        assert_eq!(uri_to_path("file:///tmp/%zz"), None);
    }

    #[test]
    fn plain_text_must_be_all_existing_paths() {
        let dir = std::env::temp_dir();
        let here = dir.to_string_lossy().into_owned();
        assert_eq!(paths_in_text(&here), vec![dir.clone()]);
        assert!(paths_in_text("Rimshot please").is_empty());
        assert!(paths_in_text(&format!("{here}\nnot a path")).is_empty());
    }
}
