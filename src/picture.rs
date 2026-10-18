//! Tile pictures: decoded and scaled down off the UI thread, ready to upload as a texture.

use anyhow::{Context, Result};
use std::path::Path;

/// File-name extensions offered when choosing a tile picture.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Longest side of a picture after loading. Tiles are at most a few hundred
/// pixels across, so this keeps texture memory small.
const MAX_SIDE: u32 = 512;

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Debug)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Straight (not premultiplied) RGBA, row-major.
    pub rgba: Vec<u8>,
}

pub fn load_picture(path: &Path) -> Result<Picture> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    decode_picture(&bytes)
}

pub fn decode_picture(bytes: &[u8]) -> Result<Picture> {
    let img = image::load_from_memory(bytes).context("unsupported or unreadable image")?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Ok(Picture { width: rgba.width() as usize, height: rgba.height() as usize, rgba: rgba.into_raw() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 4×4 opaque red PNG.
    pub(crate) fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_small_pictures_as_is() {
        let p = decode_picture(&tiny_png()).unwrap();
        assert_eq!((p.width, p.height), (4, 4));
        assert_eq!(&p.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn scales_large_pictures_down_keeping_aspect() {
        let img = image::RgbaImage::from_pixel(2000, 1000, image::Rgba([0, 0, 255, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let p = decode_picture(buf.get_ref()).unwrap();
        assert_eq!((p.width, p.height), (512, 256));
        assert_eq!(p.rgba.len(), 512 * 256 * 4);
    }

    #[test]
    fn rejects_non_images_and_matches_extensions() {
        assert!(decode_picture(b"not an image").is_err());
        assert!(is_image_path(Path::new("/x/Cover.JPG")));
        assert!(!is_image_path(Path::new("/x/song.mp3")));
    }
}
