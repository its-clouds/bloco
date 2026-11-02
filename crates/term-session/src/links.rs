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
}
