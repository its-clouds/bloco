//! The saved window layout: the tabs, their split trees and each pane's
//! directory. The workspace writes it while it runs and on quit, and reads it
//! on launch. Shells and their scrollback are not saved, only where they were.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::split_tree::Axis;
use crate::tab_groups::GroupId;

/// The file in the settings directory that holds the layout.
const FILE: &str = "layout.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
    /// The sidebar's width in pixels. `None` in a layout saved before it was kept.
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    /// The tab groups. Empty in a layout saved before groups existed.
    #[serde(default)]
    pub groups: Vec<SavedGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub root: SavedNode,
    /// The active pane, as its index in the tree's pane order.
    pub active: usize,
    /// The name the user gave the tab. `None` shows the automatic title.
    #[serde(default)]
    pub name: Option<String>,
    /// The group the tab is in. `None` when it is in no group.
    #[serde(default)]
    pub group: Option<GroupId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedGroup {
    pub id: GroupId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SavedNode {
    Pane {
        /// The directory the shell was in. `None` when it was not known.
        cwd: Option<PathBuf>,
        /// The name the user gave the session. `None` shows the automatic title.
        #[serde(default)]
        name: Option<String>,
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
                        name: Some("server".into()),
                    },
                    active: 0,
                    name: Some("build".into()),
                    group: Some(1),
                },
                SavedTab {
                    root: SavedNode::Split {
                        axis: Axis::Vertical,
                        ratio: 0.25,
                        first: Box::new(SavedNode::Pane { cwd: None, name: None }),
                        second: Box::new(SavedNode::Pane {
                            cwd: Some(PathBuf::from("/tmp/b")),
                            name: None,
                        }),
                    },
                    active: 1,
                    name: None,
                    group: None,
                },
            ],
            active_tab: 1,
            sidebar_width: Some(300.0),
            groups: vec![SavedGroup {
                id: 1,
                name: "Servers".into(),
            }],
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
    fn a_layout_saved_before_the_sidebar_width_still_loads() {
        let dir = std::env::temp_dir().join(format!("bloco-old-layout-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let old = r#"{"tabs":[{"root":{"kind":"pane","cwd":"/tmp/a"},"active":0}],"active_tab":0}"#;
        fs::write(dir.join(FILE), old).unwrap();
        let layout = load(&dir).expect("an old layout file loads");
        assert_eq!(layout.sidebar_width, None);
        assert_eq!(layout.tabs.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layout_saved_before_names_and_groups_still_loads() {
        let dir = std::env::temp_dir().join(format!("bloco-old-groups-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let old = r#"{"tabs":[{"root":{"kind":"pane","cwd":null},"active":0}],"active_tab":0,"sidebar_width":260.0}"#;
        fs::write(dir.join(FILE), old).unwrap();
        let layout = load(&dir).expect("a layout without names or groups loads");
        assert_eq!(layout.tabs[0].name, None);
        assert_eq!(layout.tabs[0].group, None);
        assert!(layout.groups.is_empty());
        assert_eq!(
            layout.tabs[0].root,
            SavedNode::Pane { cwd: None, name: None }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_loads_as_none() {
        let dir = std::env::temp_dir().join(format!("bloco-missing-{}", std::process::id()));
        assert_eq!(load(&dir), None);
    }
}
