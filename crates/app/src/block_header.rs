//! The header row of a finished command block: the command on the left, and
//! the exit status, duration and directory on the right.

use std::path::Path;
use std::time::Duration;

use term_session::Block;

/// The two texts of a header row. The command is cut to fit beside the details.
pub struct HeaderText {
    pub command: String,
    /// Exit status, duration and directory, separated by two spaces.
    pub details: String,
    /// The command exited with a non-zero status.
    pub failed: bool,
}

/// The header text of `block` for a row `columns` cells wide.
pub fn header_text(block: &Block, columns: usize) -> HeaderText {
    let mut parts = Vec::new();
    match block.exit_code {
        Some(0) => parts.push("✓".to_string()),
        Some(code) => parts.push(format!("✗ {code}")),
        None => {}
    }
    if let Some(duration) = block.duration() {
        parts.push(format_duration(duration));
    }
    if let Some(dir) = &block.cwd {
        parts.push(display_dir(dir));
    }
    let details = shorten(&parts.join("  "), columns / 2);

    let command: String = block
        .command
        .as_deref()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let room = columns.saturating_sub(details.chars().count() + 2);
    HeaderText {
        command: shorten(command.trim(), room),
        details,
        failed: block.exit_code.is_some_and(|code| code != 0),
    }
}

/// `912 ms`, `4.2 s`, or `2m 05s`.
pub fn format_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", duration.as_secs_f64())
    } else {
        let secs = duration.as_secs();
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

/// `~` for the home directory and paths under it, otherwise the path as it is.
fn display_dir(dir: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return dir.display().to_string();
    };
    match dir.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => dir.display().to_string(),
    }
}

/// Cuts `text` to `max` characters, ending with an ellipsis when it is longer.
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_short_and_readable() {
        assert_eq!(format_duration(Duration::from_millis(912)), "912 ms");
        assert_eq!(format_duration(Duration::from_millis(4200)), "4.2 s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
    }

    #[test]
    fn shorten_keeps_short_text_and_ends_long_text_with_an_ellipsis() {
        assert_eq!(shorten("abc", 3), "abc");
        assert_eq!(shorten("abcd", 3), "ab…");
        assert_eq!(shorten("abcd", 0), "");
    }
}
