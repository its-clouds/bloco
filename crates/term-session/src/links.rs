//! URL detection on one line of the grid, for Cmd+click.
//!
//! Only whole URLs that fit on one line are found. Wrapped URLs are not joined.

use alacritty_terminal::term::cell::Flags;

const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];

/// A URL found on one line of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    /// Columns covered by the URL, inclusive.
    pub first_column: usize,
    pub last_column: usize,
}

/// The URL that covers `column` on this line, if there is one. `row` holds the
/// character and flags of every cell in the line, so index == column.
pub fn link_at(row: &[(char, Flags)], column: usize) -> Option<Link> {
    // One entry per character: the character and the column where it starts.
    // The spacer after a wide character is skipped.
    let mut chars = Vec::with_capacity(row.len());
    let mut columns = Vec::with_capacity(row.len());
    for (col, &(c, flags)) in row.iter().enumerate() {
        if flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        chars.push(c);
        columns.push(col);
    }

    // The character under the cursor: the last one that starts at or before `column`.
    let hit = columns.partition_point(|&c| c <= column).checked_sub(1)?;

    for start in 0..=hit {
        if !starts_with_scheme(&chars[start..]) {
            continue;
        }
        let end = url_end(&chars, start);
        if (start..end).contains(&hit) {
            return Some(Link {
                url: chars[start..end].iter().collect(),
                first_column: columns[start],
                last_column: columns[end - 1],
            });
        }
    }
    None
}

/// A path-looking token on one line of the grid. Not checked against the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathToken {
    /// The path as printed, without quotes, brackets or a `:LINE[:COL]` suffix.
    pub text: String,
    /// Columns covered by the path, inclusive.
    pub first_column: usize,
    pub last_column: usize,
}

/// Opening characters that wrap a path in output, such as `"a.rs"` or `(b/c)`.
const LEADING: &[char] = &['"', '\'', '`', '(', '[', '<', '{'];
/// Closing characters and sentence punctuation that follow a path.
const TRAILING: &[char] = &['"', '\'', '`', ')', ']', '>', '}', ',', ';', '.', ':', '!', '?'];

/// The path-looking token that covers `column` on this line, if there is one.
/// A token is the run of non-blank characters around the cursor, trimmed of
/// wrapping quotes, brackets and punctuation, and of a `:LINE` or `:LINE:COL`
/// suffix as compilers print them. A token must contain a letter and either a
/// `/`, a leading `~`, or a `.`, so a bare word such as `total` is not one.
pub fn path_at(row: &[(char, Flags)], column: usize) -> Option<PathToken> {
    let mut chars = Vec::with_capacity(row.len());
    let mut columns = Vec::with_capacity(row.len());
    for (col, &(c, flags)) in row.iter().enumerate() {
        if flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        chars.push(c);
        columns.push(col);
    }

    let hit = columns.partition_point(|&c| c <= column).checked_sub(1)?;
    if chars[hit].is_whitespace() {
        return None;
    }

    // The run of non-blank characters around the hit.
    let mut start = hit;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = hit + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }

    // Wrapping characters on both sides. The hit must survive the trimming.
    while start < end && LEADING.contains(&chars[start]) {
        start += 1;
    }
    end = trim_trailing(&chars, start, end);
    for _ in 0..2 {
        end = strip_line_suffix(&chars, start, end);
        end = trim_trailing(&chars, start, end);
    }
    if start >= end || hit < start || hit >= end {
        return None;
    }

    let text: String = chars[start..end].iter().collect();
    if text.contains("://") || !looks_like_path(&text) {
        return None;
    }
    Some(PathToken {
        text,
        first_column: columns[start],
        last_column: columns[end - 1],
    })
}

/// `end` after dropping trailing wrapping characters and punctuation.
fn trim_trailing(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start && TRAILING.contains(&chars[end - 1]) {
        end -= 1;
    }
    end
}

/// `end` before a `:LINE` or `:LINE:COL` suffix made only of digits. Returns
/// `end` unchanged when there is no such suffix.
fn strip_line_suffix(chars: &[char], start: usize, end: usize) -> usize {
    let digits = chars[start..end]
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 || digits == end - start {
        return end;
    }
    let colon = end - digits - 1;
    if chars[colon] == ':' && colon > start {
        colon
    } else {
        end
    }
}

fn looks_like_path(text: &str) -> bool {
    let has_letter = text.chars().any(|c| c.is_alphabetic());
    let shaped = text.contains('/') || text.starts_with('~') || text.contains('.');
    has_letter && shaped
}

fn starts_with_scheme(chars: &[char]) -> bool {
    SCHEMES.iter().any(|scheme| {
        let scheme: Vec<char> = scheme.chars().collect();
        chars.len() > scheme.len() && chars[..scheme.len()] == scheme[..]
    })
}

/// The end (exclusive) of the URL that starts at `start`.
fn url_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && is_url_char(chars[end]) {
        end += 1;
    }
    // Sentence punctuation after a URL is not part of it. A closing bracket is
    // kept when it closes one inside the URL, as in Wikipedia links.
    while end > start {
        let last = chars[end - 1];
        let trim = match last {
            ')' => count(&chars[start..end], ')') > count(&chars[start..end], '('),
            ']' => count(&chars[start..end], ']') > count(&chars[start..end], '['),
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' => true,
            _ => false,
        };
        if !trim {
            break;
        }
        end -= 1;
    }
    end
}

fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | '`' | '{' | '}' | '\\' | '|')
}

fn count(chars: &[char], wanted: char) -> usize {
    chars.iter().filter(|&&c| c == wanted).count()
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::term::cell::Flags;

    use super::*;

    /// One cell per character, except `日` which takes two cells.
    fn row(text: &str) -> Vec<(char, Flags)> {
        let mut cells = Vec::new();
        for c in text.chars() {
            if c == '日' {
                cells.push((c, Flags::WIDE_CHAR));
                cells.push((' ', Flags::WIDE_CHAR_SPACER));
            } else {
                cells.push((c, Flags::empty()));
            }
        }
        cells
    }

    fn url_in(text: &str, column: usize) -> Option<String> {
        link_at(&row(text), column).map(|link| link.url)
    }

    #[test]
    fn finds_url_under_cursor() {
        let text = "see https://example.com/a?b=1 now";
        assert_eq!(url_in(text, 6).as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(url_in(text, 25).as_deref(), Some("https://example.com/a?b=1"));
    }

    #[test]
    fn reports_the_covered_columns() {
        let link = link_at(&row("see https://x.io now"), 5).unwrap();
        assert_eq!((link.first_column, link.last_column), (4, 15));
    }

    #[test]
    fn misses_text_outside_the_url() {
        let text = "see https://example.com now";
        assert_eq!(url_in(text, 0), None);
        assert_eq!(url_in(text, 23), None);
    }

    #[test]
    fn drops_trailing_punctuation() {
        assert_eq!(
            url_in("go to https://x.io/docs.", 10).as_deref(),
            Some("https://x.io/docs")
        );
        assert_eq!(
            url_in("(https://x.io/a)", 3).as_deref(),
            Some("https://x.io/a")
        );
    }

    #[test]
    fn keeps_balanced_parentheses() {
        assert_eq!(
            url_in("https://en.wikipedia.org/wiki/Rust_(programming_language)", 2).as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust_(programming_language)")
        );
    }

    #[test]
    fn handles_wide_characters_before_the_url() {
        // Full-width rows, as the grid always has them. `h` is at column 3
        // because the wide character takes columns 0 and 1.
        let text = "日 https://x.io    ";
        assert_eq!(url_in(text, 3).as_deref(), Some("https://x.io"));
        assert_eq!(url_in(text, 16), None);
    }

    #[test]
    fn plain_words_are_not_urls() {
        assert_eq!(url_in("httpd is running", 2), None);
    }

    fn path_in(text: &str, column: usize) -> Option<String> {
        path_at(&row(text), column).map(|token| token.text)
    }

    #[test]
    fn finds_file_names_from_ls_output() {
        assert_eq!(path_in("README.md  Cargo.toml", 2).as_deref(), Some("README.md"));
        assert_eq!(path_in("README.md  Cargo.toml", 12).as_deref(), Some("Cargo.toml"));
    }

    #[test]
    fn finds_relative_and_home_paths() {
        assert_eq!(path_in("./src/lib.rs", 4).as_deref(), Some("./src/lib.rs"));
        assert_eq!(path_in("open ~/notes/a.md", 9).as_deref(), Some("~/notes/a.md"));
    }

    #[test]
    fn drops_line_and_column_suffix() {
        assert_eq!(
            path_in("error at src/main.rs:42:7 here", 12).as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(path_in("src/main.rs:42", 0).as_deref(), Some("src/main.rs"));
        let token = path_at(&row("src/main.rs:42:7"), 2).unwrap();
        assert_eq!((token.first_column, token.last_column), (0, 10));
    }

    #[test]
    fn trims_quotes_brackets_and_punctuation() {
        assert_eq!(path_in("\"my/file.txt\",", 3).as_deref(), Some("my/file.txt"));
        assert_eq!(path_in("(see docs/a.md).", 6).as_deref(), Some("docs/a.md"));
        assert_eq!(path_in("'a/b.rs'", 2).as_deref(), Some("a/b.rs"));
    }

    #[test]
    fn misses_the_wrapping_characters() {
        assert_eq!(path_in("(docs/a.md)", 0), None);
    }

    #[test]
    fn urls_are_not_paths() {
        assert_eq!(path_in("https://example.com/a.html", 10), None);
        assert_eq!(path_in("file:///tmp/a.txt", 8), None);
    }

    #[test]
    fn bare_words_and_numbers_are_not_paths() {
        assert_eq!(path_in("total 48", 1), None);
        assert_eq!(path_in("ratio 3.14 now", 7), None);
        assert_eq!(path_in("... done", 1), None);
    }

    #[test]
    fn blank_cells_are_not_paths() {
        assert_eq!(path_in("a/b.rs   ", 6), None);
    }

    #[test]
    fn path_columns_skip_wide_characters() {
        // `日` takes columns 0 and 1, so the path starts at column 3.
        let token = path_at(&row("日 src/a.rs"), 5).unwrap();
        assert_eq!(token.text, "src/a.rs");
        assert_eq!((token.first_column, token.last_column), (3, 10));
    }
}
