//! The git branch of a directory, read from the repository's `HEAD` file. No
//! git process runs, so reading it is cheap enough to do at each prompt.

use std::fs;
use std::path::{Path, PathBuf};

/// The branch checked out in the repository that holds `dir`. A detached
/// `HEAD` gives the first seven characters of its commit. `None` outside a
/// repository, or when `HEAD` cannot be read.
pub fn branch(dir: &Path) -> Option<String> {
    let git_dir = find_git_dir(dir)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(name) = head.strip_prefix("ref: refs/heads/") {
        return Some(name.to_string());
    }
    if head.starts_with("ref:") {
        return None;
    }
    let short = head.get(..7)?;
    short
        .chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| short.to_string())
}

/// The git directory of the repository that holds `dir`: a `.git` directory in
/// `dir` or one of its parents. A `.git` file (a worktree or a submodule) says
/// where the directory is, relative to the folder that holds the file.
fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    for ancestor in dir.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if dot_git.is_file() {
            let text = fs::read_to_string(&dot_git).ok()?;
            let target = Path::new(text.trim().strip_prefix("gitdir:")?.trim());
            return Some(if target.is_absolute() {
                target.to_path_buf()
            } else {
                ancestor.join(target)
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let unique = format!(
                "bloco-git-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn branch_is_read_from_head() {
        let root = TempDir::new("branch");
        fs::create_dir_all(root.0.join(".git")).unwrap();
        fs::write(root.0.join(".git/HEAD"), "ref: refs/heads/feature/tabs\n").unwrap();
        assert_eq!(branch(&root.0).as_deref(), Some("feature/tabs"));
    }

    #[test]
    fn a_subdirectory_finds_the_repository_above_it() {
        let root = TempDir::new("subdir");
        fs::create_dir_all(root.0.join(".git")).unwrap();
        fs::write(root.0.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let deep = root.0.join("src/app");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(branch(&deep).as_deref(), Some("main"));
    }

    #[test]
    fn a_detached_head_shows_the_short_commit() {
        let root = TempDir::new("detached");
        fs::create_dir_all(root.0.join(".git")).unwrap();
        fs::write(
            root.0.join(".git/HEAD"),
            "9f8e7d6c5b4a3928170f1e2d3c4b5a6978695a4b\n",
        )
        .unwrap();
        assert_eq!(branch(&root.0).as_deref(), Some("9f8e7d6"));
    }

    #[test]
    fn a_git_file_points_to_the_worktree_directory() {
        let root = TempDir::new("worktree");
        let worktree = root.0.join("checkout");
        let git_dir = root.0.join("elsewhere/wt");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/topic\n").unwrap();
        fs::write(worktree.join(".git"), "gitdir: ../elsewhere/wt\n").unwrap();
        assert_eq!(branch(&worktree).as_deref(), Some("topic"));
    }

    #[test]
    fn a_directory_outside_a_repository_has_no_branch() {
        let root = TempDir::new("plain");
        assert_eq!(branch(&root.0), None);
    }

    #[test]
    fn an_unreadable_head_has_no_branch() {
        let root = TempDir::new("missing-head");
        fs::create_dir_all(root.0.join(".git")).unwrap();
        assert_eq!(branch(&root.0), None);
    }
}
