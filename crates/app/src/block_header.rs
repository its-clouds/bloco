//! The header row of a command block: the command on the left, and the state,
//! exit status, duration and directory as chips on the right. The input dock
//! reuses the chips for the directory and the git branch.

use std::path::Path;
use std::time::Duration;

use term_session::Block;

/// The glyph drawn before the command on a header row.
pub const PROMPT_GLYPH: &str = "❯ ";

/// What a chip on a header row says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    /// The command is still running.
    Running,
    /// The exit status of a failed command.
    Failed,
    Duration,
    /// The git branch of the directory. Only on the input dock.
    Branch,
    Dir,
}

/// A small labelled box on the right of a header row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub text: String,
    pub kind: ChipKind,
}

/// The texts of a header row. The command is cut to fit beside the chips.
pub struct HeaderText {
    pub command: String,
    /// Left to right. A chip may not fit; `layout_chips` says which are drawn.
    pub chips: Vec<Chip>,
    /// The command exited with a non-zero status.
    pub failed: bool,
}

/// The header text of `block` for a row `columns` cells wide.
pub fn header_text(block: &Block, columns: usize) -> HeaderText {
    let failed = block.exit_code.is_some_and(|code| code != 0);
    let mut chips = Vec::new();
    if block.duration().is_none() {
        chips.push(Chip {
            text: "running".into(),
            kind: ChipKind::Running,
        });
    }
    if let Some(code) = block.exit_code.filter(|&code| code != 0) {
        chips.push(Chip {
            text: format!("exit {code}"),
            kind: ChipKind::Failed,
        });
    }
    if let Some(duration) = block.duration() {
        chips.push(Chip {
            text: format_duration(duration),
            kind: ChipKind::Duration,
        });
    }
    if let Some(dir) = &block.cwd {
        chips.push(Chip {
            text: display_dir(dir),
            kind: ChipKind::Dir,
        });
    }

    let command: String = block
        .command
        .as_deref()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // The command stops two cells short of the leftmost chip that is drawn.
    let left = layout_chips(&chips, columns)
        .iter()
        .filter(|(_, cells)| *cells > 0)
        .map(|(start, _)| *start)
        .min()
        .unwrap_or(columns);
    let room = left.saturating_sub(2 + PROMPT_GLYPH.chars().count());
    HeaderText {
        command: shorten(command.trim(), room),
        chips,
        failed,
    }
}

/// Where each chip sits on a header row `columns` cells wide, as `(start
/// column, cells)` in the order of `chips`. The chips are right-aligned, with a
/// cell of gap between them and a cell of margin on the right, and each chip
/// has a cell of padding on both sides. A chip that is not drawn has 0 cells.
///
/// When the chips need more than half the row, the Dir chip is dropped, then
/// the Branch chip, then the Duration chip. Whatever is left is dropped entirely
/// if it still does not fit in the row. The Failed chip is the last to go.
pub fn layout_chips(chips: &[Chip], columns: usize) -> Vec<(usize, usize)> {
    let mut keep = fit(
        chips,
        columns / 2,
        &[ChipKind::Dir, ChipKind::Branch, ChipKind::Duration],
    );
    if chips_width(chips, &keep) > columns {
        keep.iter_mut().for_each(|kept| *kept = false);
    }

    let mut placed = vec![(0, 0); chips.len()];
    let mut end = columns.saturating_sub(1);
    for (index, chip) in chips.iter().enumerate().rev() {
        if !keep[index] {
            continue;
        }
        let cells = chip.text.chars().count() + 2;
        let start = end.saturating_sub(cells);
        placed[index] = (start, cells);
        end = start.saturating_sub(1);
    }
    placed
}

/// Where each chip sits on the chip row of the input dock, `columns` cells
/// wide, as `(start column, cells)` in the order of `chips`. The chips are
/// left-aligned, with a cell of gap between them. When they do not fit, the Dir
/// chip is dropped first, then the Branch chip. A chip that is not drawn has 0
/// cells.
pub fn dock_chips(chips: &[Chip], columns: usize) -> Vec<(usize, usize)> {
    let mut keep = fit(chips, columns, &[ChipKind::Dir, ChipKind::Branch]);
    if chips_width(chips, &keep) > columns {
        keep.iter_mut().for_each(|kept| *kept = false);
    }

    let mut placed = vec![(0, 0); chips.len()];
    let mut start = 0;
    for (index, chip) in chips.iter().enumerate() {
        if !keep[index] {
            continue;
        }
        let cells = chip.text.chars().count() + 2;
        placed[index] = (start, cells);
        start += cells + 1;
    }
    placed
}

/// Which chips to keep: drops the kinds in `order`, one kind at a time, until
/// the kept chips take at most `budget` cells.
fn fit(chips: &[Chip], budget: usize, order: &[ChipKind]) -> Vec<bool> {
    let mut keep = vec![true; chips.len()];
    for dropped in order {
        if chips_width(chips, &keep) <= budget {
            break;
        }
        for (chip, kept) in chips.iter().zip(keep.iter_mut()) {
            if chip.kind == *dropped {
                *kept = false;
            }
        }
    }
    keep
}

/// Cells the kept chips take, with padding, gaps and the right margin.
fn chips_width(chips: &[Chip], keep: &[bool]) -> usize {
    let kept: Vec<usize> = chips
        .iter()
        .zip(keep)
        .filter(|(_, kept)| **kept)
        .map(|(chip, _)| chip.text.chars().count() + 2)
        .collect();
    if kept.is_empty() {
        0
    } else {
        kept.iter().sum::<usize>() + kept.len()
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
pub fn display_dir(dir: &Path) -> String {
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

    fn chip(text: &str, kind: ChipKind) -> Chip {
        Chip {
            text: text.into(),
            kind,
        }
    }

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

    #[test]
    fn chips_are_right_aligned_with_gaps_and_margin() {
        // "2.1 s" is 5 + 2 cells, "~/x" is 3 + 2 cells.
        let chips = [
            chip("2.1 s", ChipKind::Duration),
            chip("~/x", ChipKind::Dir),
        ];
        // The margin leaves column 39 free, so the Dir chip ends at 38.
        assert_eq!(layout_chips(&chips, 40), vec![(26, 7), (34, 5)]);
    }

    #[test]
    fn a_narrow_row_drops_the_dir_chip_first() {
        // Budget is 10 cells: 8 + 7 + 1 does not fit, 8 + 1 does.
        let chips = [
            chip("exit 1", ChipKind::Failed),
            chip("~/abc", ChipKind::Dir),
        ];
        assert_eq!(layout_chips(&chips, 20), vec![(11, 8), (0, 0)]);
    }

    #[test]
    fn a_very_narrow_row_drops_the_duration_chip_after_the_dir_chip() {
        let chips = [
            chip("exit 1", ChipKind::Failed),
            chip("2.1 s", ChipKind::Duration),
            chip("~/abc", ChipKind::Dir),
        ];
        // Budget 10: dropping Dir leaves 8 + 7 + 2 = 17, so Duration goes too.
        assert_eq!(layout_chips(&chips, 20), vec![(11, 8), (0, 0), (0, 0)]);
    }

    #[test]
    fn dock_chips_are_left_aligned_with_gaps() {
        let chips = [chip("~/x", ChipKind::Dir), chip("main", ChipKind::Branch)];
        assert_eq!(dock_chips(&chips, 40), vec![(0, 5), (6, 6)]);
    }

    #[test]
    fn a_narrow_dock_drops_the_dir_chip_then_the_branch_chip() {
        let chips = [
            chip("~/abcdef", ChipKind::Dir),
            chip("main", ChipKind::Branch),
        ];
        // Dir takes 11 cells, Branch 6. Only Branch fits in 8 columns.
        assert_eq!(dock_chips(&chips, 8), vec![(0, 0), (0, 6)]);
        // Nothing fits in 5 columns.
        assert_eq!(dock_chips(&chips, 5), vec![(0, 0), (0, 0)]);
    }

    #[test]
    fn chips_wider_than_the_row_are_all_dropped() {
        let chips = [chip("exit 127", ChipKind::Failed)];
        assert_eq!(layout_chips(&chips, 6), vec![(0, 0)]);
    }

    #[test]
    fn no_chips_lay_out_to_nothing() {
        assert!(layout_chips(&[], 80).is_empty());
    }
}
