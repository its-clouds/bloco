//! Turns a path printed in the terminal into a file on disk, and shows it in
//! Finder. Used by Cmd+click on path links.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The file or directory that `token` names. `~` is the home directory, other
/// relative paths are based on `base` (the directory the output was printed
/// in). `None` when nothing exists at that place.
pub fn resolve(token: &str, base: Option<&Path>) -> Option<PathBuf> {
    let expanded = expand_home(token)?;
    let path = if expanded.is_absolute() {
        expanded
    } else {
        base?.join(expanded)
    };
    path.exists().then_some(path)
}

/// `~` and `~/rest` become the home directory. Other paths are returned as is.
fn expand_home(token: &str) -> Option<PathBuf> {
    match token.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = PathBuf::from(std::env::var_os("HOME")?);
            Some(home.join(rest.trim_start_matches('/')))
        }
        _ => Some(PathBuf::from(token)),
    }
}

/// The path a `file://` URL names, percent-decoded. The host part is skipped:
/// `file:///p` and `file://host/p` both give `/p`. A query or fragment is
/// dropped. `None` when `url` is not a file URL or has no path.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    const SCHEME: &str = "file://";
    let start = url.get(..SCHEME.len())?;
    if !start.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let rest = &url[SCHEME.len()..];
    let path = &rest[rest.find('/')?..];
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let bytes = percent_decode(path);
    if bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// Whether `url` starts with `http://` or `https://`, in any case. Only
/// these are handed to the system to open.
pub fn is_web_url(url: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

/// `%XX` escapes become the byte they name. A `%` without two hex digits
/// after it stays as it is.
fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Shows `path` in Finder. A directory is opened, a file is selected inside
/// its directory. Errors are ignored: a failed `open` leaves nothing to show.
pub fn reveal_in_finder(path: &Path) {
    let mut command = Command::new("open");
    if path.is_dir() {
        command.arg(path);
    } else {
        command.arg("-R").arg(path);
    }
    let _ = command.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("bloco-file-links-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join("src/main.rs"), "").unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn relative_paths_resolve_against_the_base() {
        let scratch = Scratch::new("relative");
        assert_eq!(
            resolve("src/main.rs", Some(&scratch.0)),
            Some(scratch.0.join("src/main.rs"))
        );
        assert_eq!(
            resolve("./src", Some(&scratch.0)),
            Some(scratch.0.join("./src"))
        );
    }

    #[test]
    fn missing_paths_do_not_resolve() {
        let scratch = Scratch::new("missing");
        assert_eq!(resolve("nope.md", Some(&scratch.0)), None);
        assert_eq!(resolve("src/main.rs", None), None);
    }

    #[test]
    fn absolute_paths_ignore_the_base() {
        let scratch = Scratch::new("absolute");
        let absolute = scratch.0.join("src/main.rs");
        assert_eq!(
            resolve(absolute.to_str().unwrap(), None),
            Some(absolute.clone())
        );
    }

    #[test]
    fn file_urls_become_decoded_paths() {
        let path = |p: &str| Some(PathBuf::from(p));
        assert_eq!(file_url_path("file:///tmp/a.txt"), path("/tmp/a.txt"));
        assert_eq!(file_url_path("file://host/tmp/a"), path("/tmp/a"));
        assert_eq!(file_url_path("FILE:///tmp/a"), path("/tmp/a"));
        assert_eq!(
            file_url_path("file:///Users/me/My%20File%E2%9C%93.md"),
            path("/Users/me/My File\u{2713}.md")
        );
        assert_eq!(file_url_path("file:///a/b?x=1#top"), path("/a/b"));
        assert_eq!(file_url_path("file:///a%2"), path("/a%2"));
        assert_eq!(file_url_path("file:///a%zz"), path("/a%zz"));
        assert_eq!(file_url_path("file:///"), path("/"));
    }

    #[test]
    fn non_file_urls_have_no_path() {
        assert_eq!(file_url_path("https://x.io/a"), None);
        assert_eq!(file_url_path("file://host"), None);
        assert_eq!(file_url_path("file:"), None);
        assert_eq!(file_url_path(""), None);
        assert_eq!(file_url_path("file:///a%00b"), None);
    }

    #[test]
    fn only_http_and_https_are_web_urls() {
        assert!(is_web_url("https://x.io"));
        assert!(is_web_url("HTTP://x.io"));
        assert!(!is_web_url("file:///x"));
        assert!(!is_web_url("smb://host"));
        assert!(!is_web_url(" https://x.io"));
        assert!(!is_web_url(""));
    }

    #[test]
    fn tilde_is_the_home_directory() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        assert_eq!(expand_home("~/x/y.md"), Some(home.join("x/y.md")));
        assert_eq!(expand_home("~"), Some(home.clone()));
        // `~user` is not expanded; it stays a relative name.
        assert_eq!(expand_home("~bob/x"), Some(PathBuf::from("~bob/x")));
    }
}
