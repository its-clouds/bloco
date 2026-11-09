//! What Cmd+V types. Text pastes as it is. Copied files paste as their quoted
//! paths, and an image (a screenshot, say) is saved to a temporary file whose
//! quoted path is pasted, so programs that take image paths, like Claude Code,
//! can attach it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{ClipboardEntry, ClipboardItem, Image};

use crate::sidebar::shell_quote;

/// The text a paste of `item` sends, or `None` when there is nothing to paste
/// or the image could not be saved.
pub fn paste_text(item: &ClipboardItem) -> Option<String> {
    let text: String = item
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::String(s) => Some(s.text().as_str()),
            _ => None,
        })
        .collect();
    if !text.is_empty() {
        return Some(text);
    }
    let paths: Vec<&Path> = item
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths()),
            _ => None,
        })
        .flatten()
        .map(PathBuf::as_path)
        .collect();
    if !paths.is_empty() {
        return Some(quote_paths(&paths));
    }
    let image = item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image),
        _ => None,
    })?;
    let path = save_image(image, &std::env::temp_dir().join("bloco-paste")).ok()?;
    Some(quote_paths(&[&path]))
}

/// Paths as shell words, separated by spaces.
fn quote_paths(paths: &[&Path]) -> String {
    paths
        .iter()
        .map(|path| shell_quote(&path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Writes the image into `dir` under a fresh name and returns its path.
fn save_image(image: &Image, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let path = dir.join(format!("paste-{stamp}.{}", image.format().extension()));
    std::fs::write(&path, image.bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{ExternalPaths, ImageFormat};

    #[test]
    fn text_pastes_as_it_is() {
        let item = ClipboardItem::new_string("echo hi".into());
        assert_eq!(paste_text(&item).as_deref(), Some("echo hi"));
    }

    #[test]
    fn copied_files_paste_as_quoted_paths() {
        let paths = ExternalPaths(vec![PathBuf::from("/a/b.txt"), PathBuf::from("/my docs/c.png")].into());
        let item = ClipboardItem { entries: vec![ClipboardEntry::ExternalPaths(paths)] };
        assert_eq!(paste_text(&item).as_deref(), Some("/a/b.txt '/my docs/c.png'"));
    }

    #[test]
    fn an_image_is_saved_and_its_path_pasted() {
        let bytes = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let item = ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, bytes.clone()));
        let text = paste_text(&item).unwrap();
        let path = PathBuf::from(text.trim_matches('\''));
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(path).unwrap();
    }
}
