//! Which part of the session owns the keyboard. The app reads this to decide
//! whether typing goes to the input editor or straight to the PTY. See
//! `docs/architecture.md`, "Session mode state machine".

use crate::osc133::Mark;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// The shell owns the keys, and no prompt mark (`B`) has come since the last
    /// command or ever. This covers a shell without integration, and the time
    /// between a command ending (`D`) and the next prompt. Keys go to the PTY.
    #[default]
    Unknown,
    /// The shell is at its prompt (after `OSC 133;B`). The input editor owns the keys.
    Prompt,
    /// A command runs (after `OSC 133;C`). Keys go to the PTY, so `read`,
    /// password prompts and REPLs work.
    Running,
    /// The alternate screen is on. Keys go to the PTY and the pane shows the grid.
    FullScreen,
}

impl InputMode {
    /// The mode after an OSC 133 mark. Marks that do not change the keyboard
    /// owner (`A`, and the directory report) leave it as it is.
    pub fn after(self, mark: &Mark) -> InputMode {
        match mark {
            Mark::PromptEnd => InputMode::Prompt,
            // Zsh and bash print `B` after the prompt that follows `D`. Until then the
            // shell has the keys, so the editor is not drawn over a prompt not yet printed.
            Mark::CommandEnd { .. } => InputMode::Unknown,
            Mark::CommandStart { .. } => InputMode::Running,
            Mark::PromptStart | Mark::Cwd(_) => self,
        }
    }

    /// The mode the user sees. The alternate screen wins over the mode the marks
    /// set, because full-screen apps run between `C` and `D` and draw their own screen.
    pub fn effective(marked: InputMode, alternate_screen: bool) -> InputMode {
        if alternate_screen {
            InputMode::FullScreen
        } else {
            marked
        }
    }

    /// Whether the input editor takes the keys.
    pub fn owns_input(self) -> bool {
        self == InputMode::Prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::Blocks;
    use crate::search::BufferPoint;

    fn start() -> Mark {
        Mark::CommandStart {
            command: Some("ls".into()),
        }
    }

    fn end() -> Mark {
        Mark::CommandEnd { exit_code: Some(0) }
    }

    #[test]
    fn prompt_end_hands_the_keys_to_the_editor() {
        assert_eq!(
            InputMode::Unknown.after(&Mark::PromptEnd),
            InputMode::Prompt
        );
        assert_eq!(
            InputMode::Running.after(&Mark::PromptEnd),
            InputMode::Prompt
        );
    }

    #[test]
    fn command_start_takes_the_keys_back_to_the_pty() {
        assert_eq!(InputMode::Prompt.after(&start()), InputMode::Running);
        assert_eq!(InputMode::Unknown.after(&start()), InputMode::Running);
    }

    #[test]
    fn command_end_gives_the_keys_back_to_the_shell_until_the_next_prompt() {
        assert_eq!(InputMode::Running.after(&end()), InputMode::Unknown);
    }

    #[test]
    fn prompt_start_and_cwd_do_not_change_the_owner() {
        assert_eq!(
            InputMode::Running.after(&Mark::PromptStart),
            InputMode::Running
        );
        assert_eq!(
            InputMode::Prompt.after(&Mark::Cwd("/tmp".into())),
            InputMode::Prompt
        );
    }

    #[test]
    fn alternate_screen_overrides_the_marked_mode() {
        assert_eq!(
            InputMode::effective(InputMode::Running, true),
            InputMode::FullScreen
        );
        assert_eq!(
            InputMode::effective(InputMode::Prompt, true),
            InputMode::FullScreen
        );
    }

    #[test]
    fn leaving_the_alternate_screen_restores_the_marked_mode() {
        assert_eq!(
            InputMode::effective(InputMode::Running, false),
            InputMode::Running
        );
        assert_eq!(
            InputMode::effective(InputMode::Prompt, false),
            InputMode::Prompt
        );
    }

    #[test]
    fn only_the_prompt_owns_input() {
        assert!(InputMode::Prompt.owns_input());
        assert!(!InputMode::Unknown.owns_input());
        assert!(!InputMode::Running.owns_input());
        assert!(!InputMode::FullScreen.owns_input());
    }

    #[test]
    fn a_session_of_marks_walks_prompt_command_prompt() {
        let at = Some(BufferPoint { line: 0, column: 0 });
        let mut blocks = Blocks::default();
        assert_eq!(blocks.input_mode(), InputMode::Unknown);
        blocks.apply(Mark::PromptStart, at);
        blocks.apply(Mark::PromptEnd, at);
        assert_eq!(blocks.input_mode(), InputMode::Prompt);
        blocks.apply(start(), at);
        assert_eq!(blocks.input_mode(), InputMode::Running);
        blocks.apply(end(), at);
        assert_eq!(blocks.input_mode(), InputMode::Unknown);
        blocks.apply(Mark::PromptStart, at);
        blocks.apply(Mark::PromptEnd, at);
        assert_eq!(blocks.input_mode(), InputMode::Prompt);
    }
}
