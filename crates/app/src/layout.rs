//! The saved window layout: the tabs, their split trees and each pane's
//! directory. The workspace writes it while it runs and on quit, and reads it
//! on launch. Shells and their scrollback are not saved, only where they were.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::split_tree::Axis;

/// The file in the settings directory that holds the layout.
const FILE: &str = "layout.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub root: SavedNode,
    /// The active pane, as its index in the tree's pane order.
    pub active: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SavedNode {
    Pane {
        /// The directory the shell was in. `None` when it was not known.
        cwd: Option<PathBuf>,
    },
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<SavedNode>,
        second: Box<SavedNode>,
    },
}

/// Reads the layout from `dir`. A missing or unreadable file gives `None`, and
/// the workspace starts with one empty tab.
pub fn load(dir: &Path) -> Option<SavedLayout> {
    let text = fs::read_to_string(dir.join(FILE)).ok()?;
    match serde_json::from_str(&text) {
        Ok(layout) => Some(layout),
        Err(e) => {
            eprintln!("bloco: cannot read {FILE}: {e}");
            None
        }
    }
}

/// Writes the layout to `dir`. The file is replaced in one step, so a crash
/// during the write leaves the previous layout in place.
pub fn save(dir: &Path, layout: &SavedLayout) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(layout).map_err(io::Error::other)?;
    let path = dir.join(FILE);
    let temp = dir.join(format!("{FILE}.tmp"));
    fs::write(&temp, text)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SavedLayout {
        SavedLayout {
            tabs: vec![
                SavedTab {
                    root: SavedNode::Pane {
                        cwd: Some(PathBuf::from("/tmp/a")),
                    },
                    active: 0,
                },
                SavedTab {
                    root: SavedNode::Split {
                        axis: Axis::Vertical,
                        ratio: 0.25,
                        first: Box::new(SavedNode::Pane { cwd: None }),
                        second: Box::new(SavedNode::Pane {
                            cwd: Some(PathBuf::from("/tmp/b")),
                        }),
                    },
                    active: 1,
                },
            ],
            active_tab: 1,
        }
    }

    #[test]
    fn a_saved_layout_loads_back_unchanged() {
        let dir = std::env::temp_dir().join(format!("bloco-layout-{}", std::process::id()));
        save(&dir, &sample()).unwrap();
        assert_eq!(load(&dir), Some(sample()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_loads_as_none() {
        let dir = std::env::temp_dir().join(format!("bloco-missing-{}", std::process::id()));
        assert_eq!(load(&dir), None);
    }
}
