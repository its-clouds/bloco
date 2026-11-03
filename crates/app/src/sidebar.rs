//! The left sidebar: a list of sessions (the tabs, vertical) or a file tree
//! rooted at the active pane's directory. The workspace draws it; this module
//! holds the state and reads the directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Rows the tree shows at most. A bigger tree is cut off so the sidebar stays quick.
const MAX_ROWS: usize = 2000;

/// The width the sidebar starts at, in pixels.
pub const SIDEBAR_WIDTH: f32 = 260.0;
/// The narrowest and the widest the sidebar can be dragged, in pixels.
pub const SIDEBAR_MIN: f32 = 180.0;
pub const SIDEBAR_MAX: f32 = 480.0;

/// Which list the sidebar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Sessions,
    Files,
}

/// The sidebar as the workspace keeps it. Closed at startup.
#[derive(Debug)]
pub struct Sidebar {
    pub open: bool,
    pub mode: Mode,
    pub tree: FileTree,
    /// The width in pixels. Both panels share it.
    pub width: f32,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::default(),
            tree: FileTree::default(),
            width: SIDEBAR_WIDTH,
        }
    }
}

/// Keeps a sidebar width inside `SIDEBAR_MIN..=SIDEBAR_MAX`.
pub fn clamp_width(width: f32) -> f32 {
    width.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
}

/// One line of the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    /// Only meaningful for a directory: whether its children are listed below.
    pub expanded: bool,
}

/// The directory tree under `root`. Hidden entries (names starting with `.`)
/// are left out. Folders come first, then files, each sorted by name ignoring case.
#[derive(Debug, Default)]
pub struct FileTree {
    root: Option<PathBuf>,
    /// The folders the user opened. Opened folders stay open across a reload.
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
}

impl FileTree {
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Shows `root`. Folders opened under the old root are closed. Nothing is
    /// read when `root` is the root already shown.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.expanded.clear();
        self.reload();
    }

    /// Opens a closed folder or closes an open one. A file does nothing.
    pub fn toggle(&mut self, path: &Path) {
        let is_dir = self.rows.iter().any(|row| row.path == path && row.is_dir);
        if !is_dir {
            return;
        }
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
        self.reload();
    }

    /// Lists the tree again, keeping the open folders. Returns `true` when the
    /// rows changed, so the caller only redraws then.
    pub fn reload(&mut self) -> bool {
        let mut rows = Vec::new();
        if let Some(root) = &self.root {
            push_children(root, 0, &self.expanded, &mut rows);
        }
        let changed = rows != self.rows;
        self.rows = rows;
        changed
    }
}

fn push_children(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, rows: &mut Vec<Row>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<(String, PathBuf, bool)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let is_dir = entry.file_type().ok()?.is_dir();
            Some((name, entry.path(), is_dir))
        })
        .collect();
    children.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    for (name, path, is_dir) in children {
        if rows.len() >= MAX_ROWS {
            return;
        }
        let expanded_here = is_dir && expanded.contains(&path);
        rows.push(Row {
            path: path.clone(),
            name,
            depth,
            is_dir,
            expanded: expanded_here,
        });
        if expanded_here {
            push_children(&path, depth + 1, expanded, rows);
        }
    }
}

/// A path as one word for the shell. Plain paths are left as they are, and
/// anything else goes in single quotes, with each `'` written as `'\''`.
pub fn shell_quote(path: &str) -> String {
    let plain = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c));
    if plain {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory for one test. Removed by the test's guard.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("bloco-sidebar-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(tree: &FileTree) -> Vec<(usize, String)> {
        tree.rows().iter().map(|row| (row.depth, row.name.clone())).collect()
    }

    #[test]
    fn folders_come_first_and_hidden_entries_are_left_out() {
        let scratch = Scratch::new("order");
        for file in ["b.txt", "A.md", ".env"] {
            std::fs::write(scratch.0.join(file), "").unwrap();
        }
        std::fs::create_dir(scratch.0.join("src")).unwrap();
        std::fs::create_dir(scratch.0.join(".git")).unwrap();
        let mut tree = FileTree::default();
        tree.set_root(Some(scratch.0.clone()));
        assert_eq!(
            names(&tree),
            [(0, "src".into()), (0, "A.md".into()), (0, "b.txt".into())]
        );
    }

    #[test]
    fn toggling_a_folder_lists_its_children() {
        let scratch = Scratch::new("toggle");
        std::fs::create_dir_all(scratch.0.join("src/inner")).unwrap();
        std::fs::write(scratch.0.join("src/main.rs"), "").unwrap();
        let mut tree = FileTree::default();
        tree.set_root(Some(scratch.0.clone()));
        let src = scratch.0.join("src");
        tree.toggle(&src);
        assert_eq!(names(&tree), [(0, "src".into()), (1, "inner".into()), (1, "main.rs".into())]);
        assert!(tree.rows()[0].expanded);
        tree.toggle(&src);
        assert_eq!(names(&tree), [(0, "src".into())]);
    }

    #[test]
    fn toggling_a_file_does_nothing() {
        let scratch = Scratch::new("file");
        std::fs::write(scratch.0.join("a.txt"), "").unwrap();
        let mut tree = FileTree::default();
        tree.set_root(Some(scratch.0.clone()));
        tree.toggle(&scratch.0.join("a.txt"));
        assert_eq!(names(&tree), [(0, "a.txt".into())]);
    }

    #[test]
    fn a_new_root_closes_the_folders() {
        let scratch = Scratch::new("root");
        std::fs::create_dir(scratch.0.join("src")).unwrap();
        std::fs::write(scratch.0.join("src/a.rs"), "").unwrap();
        let mut tree = FileTree::default();
        tree.set_root(Some(scratch.0.clone()));
        tree.toggle(&scratch.0.join("src"));
        tree.set_root(Some(scratch.0.join("src")));
        assert_eq!(names(&tree), [(0, "a.rs".into())]);
        tree.set_root(Some(scratch.0.clone()));
        assert_eq!(names(&tree), [(0, "src".into())]);
    }

    #[test]
    fn reload_reports_only_real_changes() {
        let scratch = Scratch::new("reload");
        let mut tree = FileTree::default();
        tree.set_root(Some(scratch.0.clone()));
        assert!(!tree.reload());
        std::fs::write(scratch.0.join("new.txt"), "").unwrap();
        assert!(tree.reload());
        assert_eq!(names(&tree), [(0, "new.txt".into())]);
    }

    #[test]
    fn a_missing_root_shows_nothing() {
        let mut tree = FileTree::default();
        tree.set_root(Some(PathBuf::from("/no/such/bloco/dir")));
        assert!(tree.rows().is_empty());
        tree.set_root(None);
        assert!(tree.root().is_none());
    }

    #[test]
    fn the_width_stays_between_the_limits() {
        assert_eq!(clamp_width(100.0), SIDEBAR_MIN);
        assert_eq!(clamp_width(300.0), 300.0);
        assert_eq!(clamp_width(900.0), SIDEBAR_MAX);
        assert_eq!(Sidebar::default().width, SIDEBAR_WIDTH);
    }

    #[test]
    fn plain_paths_are_not_quoted() {
        assert_eq!(shell_quote("/tmp/a-b_c.txt"), "/tmp/a-b_c.txt");
    }

    #[test]
    fn other_paths_are_quoted_for_the_shell() {
        assert_eq!(shell_quote("my file.txt"), "'my file.txt'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }
}
