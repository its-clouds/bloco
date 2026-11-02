//! Command block records, built from OSC 133 marks, and the rows a frame shows.
//!
//! A block's positions are buffer lines (see [`BufferPoint`]), taken from the
//! cursor when the mark arrives. A finished block shows one header row in place
//! of its prompt and command lines. A collapsed block also hides its output.

use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crate::mode::InputMode;
use crate::osc133::Mark;
use crate::reflow::{Anchor, Layout};
use crate::search::BufferPoint;

/// Most blocks kept. The oldest go first, and their headers with them.
const MAX_BLOCKS: usize = 1000;

/// One command run: from `C` (command start) to `D` (command end).
#[derive(Debug, Clone)]
pub struct Block {
    /// Unique within a session. A frame refers to blocks by this id.
    pub id: u64,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: SystemTime,
    /// `None` while the command is still running.
    pub finished_at: Option<SystemTime>,
    /// The directory the shell reported before the command ran.
    pub cwd: Option<PathBuf>,
    /// Buffer line where the prompt was drawn (`A`). `None` when no prompt
    /// mark came first, or the mark arrived on the alternate screen.
    pub prompt_line: Option<usize>,
    /// Buffer line where the output starts (`C`).
    pub output_line: Option<usize>,
    /// Buffer line after the output (`D`). The output is `output_line..end_line`.
    pub end_line: Option<usize>,
    /// The output is hidden, and the header stands for the whole block.
    pub collapsed: bool,
    started: Instant,
    finished: Option<Instant>,
}

impl Block {
    /// Wall time from start to finish. `None` while the command is running.
    pub fn duration(&self) -> Option<Duration> {
        self.finished.map(|end| end.duration_since(self.started))
    }

    /// The lines of a block that is drawn as a block: its header lines, and the
    /// output lines that a collapsed block hides. `None` for a running block,
    /// or one whose lines are unknown, which is drawn as plain rows.
    fn fold(&self) -> Option<Fold> {
        let prompt = self.prompt_line?;
        let output = self.output_line?;
        let end = self.end_line?;
        self.finished_at?;
        if prompt >= output || output > end {
            return None;
        }
        let hidden = if self.collapsed {
            output..end
        } else {
            output..output
        };
        Some(Fold {
            header: prompt..output,
            hidden,
        })
    }
}

/// Buffer lines of one drawn block. See [`Block::fold`].
struct Fold {
    header: Range<usize>,
    hidden: Range<usize>,
}

#[derive(Debug, Default)]
pub struct Blocks {
    list: Vec<Block>,
    next_id: u64,
    /// The buffer line where the current prompt was drawn.
    prompt_line: Option<usize>,
    /// The directory the shell reported last (OSC 7).
    cwd: Option<PathBuf>,
    /// Who owns the keyboard, from the marks seen so far.
    input: InputMode,
}

impl Blocks {
    /// Updates the block list for one mark. `at` is the cursor's buffer point
    /// when the mark arrived, or `None` on the alternate screen.
    ///
    /// A `D` with no open block (the first prompt, for example) is ignored.
    pub fn apply(&mut self, mark: Mark, at: Option<BufferPoint>) {
        self.input = self.input.after(&mark);
        match mark {
            Mark::PromptStart => self.prompt_line = at.map(|p| p.line),
            Mark::PromptEnd => {}
            Mark::CommandStart { command } => {
                let id = self.next_id;
                self.next_id += 1;
                self.list.push(Block {
                    id,
                    command,
                    exit_code: None,
                    started_at: SystemTime::now(),
                    finished_at: None,
                    cwd: self.cwd.clone(),
                    prompt_line: self.prompt_line.take(),
                    output_line: at.map(|p| p.line),
                    end_line: None,
                    collapsed: false,
                    started: Instant::now(),
                    finished: None,
                });
                if self.list.len() > MAX_BLOCKS {
                    self.list.remove(0);
                }
            }
            Mark::CommandEnd { exit_code } => {
                if let Some(block) = self.list.last_mut().filter(|b| b.finished.is_none()) {
                    block.exit_code = exit_code;
                    block.finished_at = Some(SystemTime::now());
                    block.finished = Some(Instant::now());
                    // A cursor after the last output character has moved on to
                    // the next line, which is not output.
                    block.end_line = at.map(|p| p.line + usize::from(p.column > 0));
                }
            }
            Mark::Cwd(path) => self.cwd = Some(PathBuf::from(path)),
        }
    }

    /// Forgets where the blocks are. Used when the buffer no longer matches the
    /// recorded lines and they cannot be followed, which is when the scrollback
    /// counts were lost. The blocks stay, and their output is drawn as plain rows.
    /// Who owns the keyboard, from the marks alone. The alternate screen is
    /// not part of this; see [`InputMode::effective`].
    pub fn input_mode(&self) -> InputMode {
        self.input
    }

    pub fn forget_positions(&mut self) {
        for line in self.lines_mut() {
            *line = None;
        }
    }

    /// Takes every recorded line as an anchor in `layout`, before the grid is
    /// resized. [`Blocks::restore`] puts them back after the resize.
    pub(crate) fn anchors(&mut self, layout: &Layout) -> Vec<Option<Anchor>> {
        self.lines_mut()
            .into_iter()
            .map(|line| (*line).and_then(|line| layout.anchor(line)))
            .collect()
    }

    /// Puts each line back where its anchor is in `layout`, the grid after the
    /// resize. A line whose text left the buffer loses its position.
    pub(crate) fn restore(&mut self, anchors: &[Option<Anchor>], layout: &Layout) {
        for (line, anchor) in self.lines_mut().into_iter().zip(anchors.iter().copied()) {
            *line = anchor.and_then(|anchor| layout.line(anchor));
        }
    }

    /// Every recorded line, in one fixed order: the pending prompt line, then the
    /// prompt, output and end lines of each block.
    fn lines_mut(&mut self) -> Vec<&mut Option<usize>> {
        let mut lines = vec![&mut self.prompt_line];
        for block in &mut self.list {
            lines.push(&mut block.prompt_line);
            lines.push(&mut block.output_line);
            lines.push(&mut block.end_line);
        }
        lines
    }

    /// Moves every recorded line up by `lines`, for lines dropped from the top
    /// of a full scrollback. A line that was dropped loses its position.
    pub fn shift_up(&mut self, lines: usize) {
        let shift = |line: &mut Option<usize>| *line = line.and_then(|l| l.checked_sub(lines));
        shift(&mut self.prompt_line);
        for block in &mut self.list {
            shift(&mut block.prompt_line);
            shift(&mut block.output_line);
            shift(&mut block.end_line);
        }
    }

    pub fn all(&self) -> &[Block] {
        &self.list
    }

    /// The block whose command is still running, if any.
    pub fn running(&self) -> Option<&Block> {
        self.list.last().filter(|b| b.finished.is_none())
    }

    /// The directory the shell last reported (OSC 7).
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Shows or hides the output of block `id`. Returns `false` for an unknown id.
    pub fn toggle_collapsed(&mut self, id: u64) -> bool {
        match self.list.iter_mut().find(|b| b.id == id) {
            Some(block) => {
                block.collapsed = !block.collapsed;
                true
            }
            None => false,
        }
    }
}

/// One visual row of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A buffer line, drawn as it is.
    Line(usize),
    /// The header of the finished block with this id.
    Header(u64),
}

/// The visual rows for buffer lines `first..=last`, top first.
///
/// A finished block takes one header row in place of its prompt and command
/// lines. A collapsed block's output lines take no row. A block that has not
/// finished takes no header, so its command runs as plain rows.
pub fn rows(blocks: &[Block], first: usize, last: usize) -> Vec<Row> {
    let folds: Vec<(&Block, Fold)> = blocks
        .iter()
        .filter_map(|block| block.fold().map(|fold| (block, fold)))
        .filter(|(_, fold)| {
            (fold.header.end > first && fold.header.start <= last)
                || (fold.hidden.end > first && fold.hidden.start <= last)
        })
        .collect();

    let mut out = Vec::new();
    let mut line = first;
    while line <= last {
        let fold = folds
            .iter()
            .find(|(_, fold)| fold.header.contains(&line) || fold.hidden.contains(&line));
        match fold {
            None => {
                out.push(Row::Line(line));
                line += 1;
            }
            Some((block, fold)) if fold.header.contains(&line) => {
                out.push(Row::Header(block.id));
                line = fold.header.end;
            }
            Some((_, fold)) => line = fold.hidden.end,
        }
    }
    out
}

/// The rows of a frame of `height` rows, starting at buffer line `top`.
///
/// Headers and hidden output take fewer rows than lines, so the frame takes
/// more lines to fill up. It takes them from below when the buffer has lines
/// past the frame (`total` is the number of buffer lines). At the bottom of the
/// buffer it takes them from above instead, so the newest line stays at the
/// bottom.
pub fn frame_rows(blocks: &[Block], top: usize, total: usize, height: usize) -> Vec<Row> {
    let mut first = top;
    let mut last = (top + height)
        .saturating_sub(1)
        .min(total.saturating_sub(1));
    loop {
        let out = rows(blocks, first, last);
        let missing = height.saturating_sub(out.len());
        if missing == 0 {
            return out;
        }
        if last + 1 < total {
            last = (last + missing).min(total - 1);
        } else if first > 0 {
            first = first.saturating_sub(missing);
        } else {
            return out;
        }
    }
}

/// The finished block whose output shows at buffer line `line`, when that
/// output is not hidden. Used for the sticky header over a scrolled block.
pub fn block_containing_output(blocks: &[Block], line: usize) -> Option<&Block> {
    blocks.iter().find(|block| {
        block.fold().is_some_and(|fold| {
            let output = block.output_line.unwrap_or(0);
            let end = block.end_line.unwrap_or(0);
            (output..end).contains(&line) && !fold.hidden.contains(&line)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, column: usize) -> Option<BufferPoint> {
        Some(BufferPoint { line, column })
    }

    /// A finished block whose prompt is at line `prompt`, output from `output`,
    /// ending before `end`.
    fn finished(blocks: &mut Blocks, prompt: usize, output: usize, end: usize) {
        blocks.apply(Mark::PromptStart, at(prompt, 0));
        blocks.apply(Mark::PromptEnd, at(prompt, 2));
        blocks.apply(
            Mark::CommandStart {
                command: Some("cmd".into()),
            },
            at(output, 0),
        );
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) }, at(end, 0));
    }

    #[test]
    fn records_command_and_exit_code() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::PromptStart, at(0, 0));
        blocks.apply(Mark::PromptEnd, at(0, 2));
        blocks.apply(
            Mark::CommandStart {
                command: Some("false".into()),
            },
            at(1, 0),
        );
        assert!(blocks.all()[0].duration().is_none());
        blocks.apply(Mark::CommandEnd { exit_code: Some(1) }, at(2, 0));

        let block = &blocks.all()[0];
        assert_eq!(block.command.as_deref(), Some("false"));
        assert_eq!(block.exit_code, Some(1));
        assert_eq!(block.prompt_line, Some(0));
        assert_eq!(block.output_line, Some(1));
        assert_eq!(block.end_line, Some(2));
        assert!(block.finished_at.is_some());
        assert!(block.duration().is_some());
    }

    #[test]
    fn end_without_start_is_ignored() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) }, at(0, 0));
        assert!(blocks.all().is_empty());
    }

    #[test]
    fn end_closes_only_the_latest_open_block() {
        let mut blocks = Blocks::default();
        blocks.apply(
            Mark::CommandStart {
                command: Some("a".into()),
            },
            at(0, 0),
        );
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) }, at(1, 0));
        blocks.apply(Mark::CommandEnd { exit_code: Some(9) }, at(2, 0));
        assert_eq!(blocks.all()[0].exit_code, Some(0));
    }

    #[test]
    fn end_after_output_text_includes_the_partial_line() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::CommandStart { command: None }, at(4, 0));
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) }, at(5, 3));
        assert_eq!(blocks.all()[0].end_line, Some(6));
    }

    #[test]
    fn shifted_positions_follow_their_lines_and_drop_when_gone() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 5, 6, 9);
        blocks.shift_up(4);
        assert_eq!(
            rows(blocks.all(), 0, 5),
            vec![
                Row::Line(0),
                Row::Header(0),
                Row::Line(2),
                Row::Line(3),
                Row::Line(4),
                Row::Line(5)
            ]
        );
        blocks.shift_up(3);
        assert!(blocks.all()[0].prompt_line.is_none());
        assert!(blocks.all()[0].fold().is_none());
    }

    #[test]
    fn forgotten_positions_draw_plain_rows() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 1, 3);
        blocks.forget_positions();
        assert_eq!(
            rows(blocks.all(), 0, 2),
            vec![Row::Line(0), Row::Line(1), Row::Line(2)]
        );
        assert_eq!(blocks.all()[0].command.as_deref(), Some("cmd"));
    }

    #[test]
    fn marks_on_the_alternate_screen_leave_positions_unset() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::CommandStart { command: None }, None);
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) }, None);
        let block = &blocks.all()[0];
        assert_eq!(block.output_line, None);
        assert_eq!(block.end_line, None);
        assert!(block.fold().is_none());
    }

    #[test]
    fn tracks_the_reported_directory() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::Cwd("/tmp/a".into()), None);
        blocks.apply(Mark::CommandStart { command: None }, at(0, 0));
        assert_eq!(blocks.cwd(), Some(&PathBuf::from("/tmp/a")));
        assert_eq!(blocks.all()[0].cwd, Some(PathBuf::from("/tmp/a")));
    }

    #[test]
    fn finished_block_shows_a_header_row_for_its_prompt_and_command() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 1, 3);
        let id = blocks.all()[0].id;
        // Lines 0 is the prompt and command, shown as one header row. Output
        // starts at line 1 and is drawn as it is.
        assert_eq!(
            rows(blocks.all(), 0, 4),
            vec![
                Row::Header(id),
                Row::Line(1),
                Row::Line(2),
                Row::Line(3),
                Row::Line(4)
            ]
        );
    }

    #[test]
    fn running_block_is_plain_rows() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::PromptStart, at(0, 0));
        blocks.apply(Mark::CommandStart { command: None }, at(1, 0));
        assert_eq!(
            rows(blocks.all(), 0, 2),
            vec![Row::Line(0), Row::Line(1), Row::Line(2)]
        );
    }

    #[test]
    fn collapsed_block_hides_its_output() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 1, 5);
        let id = blocks.all()[0].id;
        assert!(blocks.toggle_collapsed(id));
        assert_eq!(
            rows(blocks.all(), 0, 6),
            vec![Row::Header(id), Row::Line(5), Row::Line(6)]
        );
        assert!(blocks.toggle_collapsed(id));
        assert_eq!(
            rows(blocks.all(), 0, 2),
            vec![Row::Header(id), Row::Line(1), Row::Line(2)]
        );
        assert!(!blocks.toggle_collapsed(99));
    }

    #[test]
    fn window_starting_inside_a_header_or_hidden_output() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 2, 6);
        let id = blocks.all()[0].id;
        // Starting at the command line still gives the header, at the top.
        assert_eq!(
            rows(blocks.all(), 1, 3),
            vec![Row::Header(id), Row::Line(2), Row::Line(3)]
        );
        blocks.toggle_collapsed(id);
        // Starting inside the hidden output, nothing of the block is drawn.
        assert_eq!(rows(blocks.all(), 3, 7), vec![Row::Line(6), Row::Line(7)]);
    }

    #[test]
    fn frame_takes_more_lines_when_rows_are_folded() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 1, 10);
        let id = blocks.all()[0].id;
        blocks.toggle_collapsed(id);
        // Lines 1..10 are hidden, so a frame of 3 rows from line 0 takes lines
        // 10 and 11 as well: the header, then the next lines.
        assert_eq!(
            frame_rows(blocks.all(), 0, 40, 3),
            vec![Row::Header(id), Row::Line(10), Row::Line(11)]
        );
        // At the bottom of the buffer there are no lines below, so the frame
        // reaches up to the header instead.
        assert_eq!(
            frame_rows(blocks.all(), 9, 12, 3),
            vec![Row::Header(id), Row::Line(10), Row::Line(11)]
        );
    }

    #[test]
    fn sticky_block_is_the_one_whose_output_is_at_the_top() {
        let mut blocks = Blocks::default();
        finished(&mut blocks, 0, 1, 8);
        assert_eq!(
            block_containing_output(blocks.all(), 4).map(|b| b.id),
            Some(0)
        );
        assert!(block_containing_output(blocks.all(), 0).is_none());
        let id = blocks.all()[0].id;
        blocks.toggle_collapsed(id);
        assert!(block_containing_output(blocks.all(), 4).is_none());
    }
}
