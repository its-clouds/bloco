//! Command block records, built from OSC 133 marks.

use std::time::{Duration, Instant, SystemTime};

use crate::osc133::Mark;

/// One command run: from `C` (command start) to `D` (command end).
#[derive(Debug, Clone)]
pub struct Block {
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: SystemTime,
    /// `None` while the command is still running.
    pub finished_at: Option<SystemTime>,
    started: Instant,
    finished: Option<Instant>,
}

impl Block {
    /// Wall time from start to finish. `None` while the command is running.
    pub fn duration(&self) -> Option<Duration> {
        self.finished.map(|end| end.duration_since(self.started))
    }
}

#[derive(Debug, Default)]
pub struct Blocks {
    list: Vec<Block>,
}

impl Blocks {
    /// Updates the block list for one mark. A `D` with no open block (the
    /// first prompt, for example) is ignored.
    pub fn apply(&mut self, mark: Mark) {
        match mark {
            Mark::CommandStart { command } => self.list.push(Block {
                command,
                exit_code: None,
                started_at: SystemTime::now(),
                finished_at: None,
                started: Instant::now(),
                finished: None,
            }),
            Mark::CommandEnd { exit_code } => {
                if let Some(block) = self.list.last_mut().filter(|b| b.finished.is_none()) {
                    block.exit_code = exit_code;
                    block.finished_at = Some(SystemTime::now());
                    block.finished = Some(Instant::now());
                }
            }
            Mark::PromptStart | Mark::PromptEnd | Mark::Cwd(_) => {}
        }
    }

    pub fn all(&self) -> &[Block] {
        &self.list
    }

    /// The block whose command is still running, if any.
    pub fn running(&self) -> Option<&Block> {
        self.list.last().filter(|b| b.finished.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_command_and_exit_code() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::PromptStart);
        blocks.apply(Mark::PromptEnd);
        blocks.apply(Mark::CommandStart {
            command: Some("false".into()),
        });
        assert!(blocks.all()[0].duration().is_none());
        blocks.apply(Mark::CommandEnd { exit_code: Some(1) });

        let block = &blocks.all()[0];
        assert_eq!(block.command.as_deref(), Some("false"));
        assert_eq!(block.exit_code, Some(1));
        assert!(block.finished_at.is_some());
        assert!(block.duration().is_some());
    }

    #[test]
    fn end_without_start_is_ignored() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) });
        assert!(blocks.all().is_empty());
    }

    #[test]
    fn end_closes_only_the_latest_open_block() {
        let mut blocks = Blocks::default();
        blocks.apply(Mark::CommandStart {
            command: Some("a".into()),
        });
        blocks.apply(Mark::CommandEnd { exit_code: Some(0) });
        blocks.apply(Mark::CommandEnd { exit_code: Some(9) });
        assert_eq!(blocks.all()[0].exit_code, Some(0));
    }
}
