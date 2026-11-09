//! The action registry. Each command has an id, a title and default keys.
//! Keybindings (`keymap.toml` over these defaults) and the command palette both
//! come from this list.

use config::Binding;
use gpui::{Action, App, KeyBinding, actions};

actions!(
    bloco,
    [
        TogglePalette,
        NewTab,
        CloseTab,
        ClosePane,
        RenameTab,
        RenamePane,
        MoveTabLeft,
        MoveTabRight,
        MoveTabToGroup,
        MovePaneToNewTab,
        MovePaneToTab,
        NewGroup,
        RenameGroup,
        DeleteGroup,
        NextTab,
        PreviousTab,
        SplitRight,
        SplitDown,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        CopySelection,
        PasteClipboard,
        TextWordBack,
        TextWordForward,
        TextLineStart,
        TextLineEnd,
        TextDeleteWordBack,
        TextDeleteToLineStart,
        FindOpen,
        FindNext,
        FindPrevious,
        FindClose,
        BlockPrevious,
        BlockNext,
        BlockCopyCommand,
        BlockCopyOutput,
        BlockToggleCollapse,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleSidebar,
        ShowSessions,
        ShowFiles,
        OpenSettings,
        ImportTheme,
        ClosePreview,
        QuitApp,
    ]
);

/// Switches to tab `0`-based index. The keymap uses 1-based ids (`tab.select.1`).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = bloco, no_json)]
pub struct SelectTab(pub usize);

pub struct Command {
    pub id: &'static str,
    pub title: &'static str,
    pub keys: &'static [&'static str],
}

pub const COMMANDS: &[Command] = &[
    Command { id: "palette.toggle", title: "Show command palette", keys: &["cmd-shift-p", "cmd-p"] },
    Command { id: "tab.new", title: "New session", keys: &["cmd-t"] },
    Command { id: "tab.close", title: "Close session", keys: &[] },
    Command { id: "tab.rename", title: "Rename session", keys: &[] },
    Command { id: "pane.rename", title: "Rename pane", keys: &[] },
    Command { id: "tab.move_left", title: "Move session up", keys: &["cmd-ctrl-left"] },
    Command { id: "tab.move_right", title: "Move session down", keys: &["cmd-ctrl-right"] },
    Command { id: "tab.move_to_group", title: "Move session to group...", keys: &[] },
    Command { id: "pane.move_to_new_tab", title: "Move pane to new session", keys: &[] },
    Command { id: "pane.move_to_tab", title: "Move pane to session...", keys: &[] },
    Command { id: "group.new", title: "New group", keys: &[] },
    Command { id: "group.rename", title: "Rename group", keys: &[] },
    Command { id: "group.delete", title: "Delete group", keys: &[] },
    Command { id: "tab.next", title: "Next session", keys: &["cmd-}"] },
    Command { id: "tab.previous", title: "Previous session", keys: &["cmd-{"] },
    Command { id: "tab.select.1", title: "Go to session 1", keys: &["cmd-1"] },
    Command { id: "tab.select.2", title: "Go to session 2", keys: &["cmd-2"] },
    Command { id: "tab.select.3", title: "Go to session 3", keys: &["cmd-3"] },
    Command { id: "tab.select.4", title: "Go to session 4", keys: &["cmd-4"] },
    Command { id: "tab.select.5", title: "Go to session 5", keys: &["cmd-5"] },
    Command { id: "tab.select.6", title: "Go to session 6", keys: &["cmd-6"] },
    Command { id: "tab.select.7", title: "Go to session 7", keys: &["cmd-7"] },
    Command { id: "tab.select.8", title: "Go to session 8", keys: &["cmd-8"] },
    Command { id: "tab.select.9", title: "Go to session 9", keys: &["cmd-9"] },
    Command { id: "pane.close", title: "Close pane", keys: &["cmd-w"] },
    Command { id: "pane.split_right", title: "Split right", keys: &["cmd-d"] },
    Command { id: "pane.split_down", title: "Split down", keys: &["cmd-shift-d"] },
    Command { id: "pane.focus_left", title: "Focus pane on the left", keys: &["cmd-alt-left"] },
    Command { id: "pane.focus_right", title: "Focus pane on the right", keys: &["cmd-alt-right"] },
    Command { id: "pane.focus_up", title: "Focus pane above", keys: &["cmd-alt-up"] },
    Command { id: "pane.focus_down", title: "Focus pane below", keys: &["cmd-alt-down"] },
    Command { id: "edit.copy", title: "Copy", keys: &["cmd-c"] },
    Command { id: "edit.paste", title: "Paste", keys: &["cmd-v"] },
    Command { id: "terminal.word_back", title: "Move back one word", keys: &["alt-left"] },
    Command { id: "terminal.word_forward", title: "Move forward one word", keys: &["alt-right"] },
    Command { id: "terminal.line_start", title: "Move to start of line", keys: &["cmd-left"] },
    Command { id: "terminal.line_end", title: "Move to end of line", keys: &["cmd-right"] },
    Command { id: "terminal.delete_word_back", title: "Delete word backward", keys: &["alt-backspace"] },
    Command { id: "terminal.delete_to_line_start", title: "Delete to start of line", keys: &["cmd-backspace"] },
    Command { id: "find.open", title: "Find", keys: &["cmd-f"] },
    Command { id: "find.next", title: "Find next", keys: &["cmd-g"] },
    Command { id: "find.previous", title: "Find previous", keys: &["cmd-shift-g"] },
    Command { id: "find.close", title: "Close find bar", keys: &[] },
    Command { id: "block.previous", title: "Previous block", keys: &["cmd-up"] },
    Command { id: "block.next", title: "Next block", keys: &["cmd-down"] },
    Command { id: "block.copy_command", title: "Copy block command", keys: &["cmd-shift-c"] },
    Command { id: "block.copy_output", title: "Copy block output", keys: &["cmd-alt-c"] },
    Command { id: "block.toggle_collapse", title: "Collapse or expand block", keys: &[] },
    Command { id: "view.zoom_in", title: "Zoom in", keys: &["cmd-=", "cmd-+"] },
    Command { id: "view.zoom_out", title: "Zoom out", keys: &["cmd--"] },
    Command { id: "view.zoom_reset", title: "Reset zoom", keys: &["cmd-0"] },
    Command { id: "sidebar.toggle", title: "Toggle sidebar", keys: &["cmd-shift-b"] },
    Command { id: "sidebar.sessions", title: "Show sessions in sidebar", keys: &[] },
    Command { id: "sidebar.files", title: "Show file tree in sidebar", keys: &["cmd-shift-e"] },
    Command { id: "settings.open", title: "Open settings", keys: &["cmd-,"] },
    Command { id: "theme.import", title: "Import theme from file", keys: &[] },
    Command { id: "preview.close", title: "Close File Preview", keys: &[] },
    Command { id: "app.quit", title: "Quit Bloco", keys: &["cmd-q"] },
];

/// The action a command id runs. `None` for an unknown id.
pub fn action_for(id: &str) -> Option<Box<dyn Action>> {
    let action: Box<dyn Action> = match id {
        "palette.toggle" => Box::new(TogglePalette),
        "tab.new" => Box::new(NewTab),
        "tab.close" => Box::new(CloseTab),
        "tab.rename" => Box::new(RenameTab),
        "pane.rename" => Box::new(RenamePane),
        "tab.move_left" => Box::new(MoveTabLeft),
        "tab.move_right" => Box::new(MoveTabRight),
        "tab.move_to_group" => Box::new(MoveTabToGroup),
        "pane.move_to_new_tab" => Box::new(MovePaneToNewTab),
        "pane.move_to_tab" => Box::new(MovePaneToTab),
        "group.new" => Box::new(NewGroup),
        "group.rename" => Box::new(RenameGroup),
        "group.delete" => Box::new(DeleteGroup),
        "tab.next" => Box::new(NextTab),
        "tab.previous" => Box::new(PreviousTab),
        "pane.close" => Box::new(ClosePane),
        "pane.split_right" => Box::new(SplitRight),
        "pane.split_down" => Box::new(SplitDown),
        "pane.focus_left" => Box::new(FocusLeft),
        "pane.focus_right" => Box::new(FocusRight),
        "pane.focus_up" => Box::new(FocusUp),
        "pane.focus_down" => Box::new(FocusDown),
        "edit.copy" => Box::new(CopySelection),
        "edit.paste" => Box::new(PasteClipboard),
        "terminal.word_back" => Box::new(TextWordBack),
        "terminal.word_forward" => Box::new(TextWordForward),
        "terminal.line_start" => Box::new(TextLineStart),
        "terminal.line_end" => Box::new(TextLineEnd),
        "terminal.delete_word_back" => Box::new(TextDeleteWordBack),
        "terminal.delete_to_line_start" => Box::new(TextDeleteToLineStart),
        "find.open" => Box::new(FindOpen),
        "find.next" => Box::new(FindNext),
        "find.previous" => Box::new(FindPrevious),
        "find.close" => Box::new(FindClose),
        "block.previous" => Box::new(BlockPrevious),
        "block.next" => Box::new(BlockNext),
        "block.copy_command" => Box::new(BlockCopyCommand),
        "block.copy_output" => Box::new(BlockCopyOutput),
        "block.toggle_collapse" => Box::new(BlockToggleCollapse),
        "view.zoom_in" => Box::new(ZoomIn),
        "view.zoom_out" => Box::new(ZoomOut),
        "view.zoom_reset" => Box::new(ZoomReset),
        "sidebar.toggle" => Box::new(ToggleSidebar),
        "sidebar.sessions" => Box::new(ShowSessions),
        "sidebar.files" => Box::new(ShowFiles),
        "settings.open" => Box::new(OpenSettings),
        "theme.import" => Box::new(ImportTheme),
        "preview.close" => Box::new(ClosePreview),
        "app.quit" => Box::new(QuitApp),
        _ => {
            let number: usize = id.strip_prefix("tab.select.")?.parse().ok()?;
            if !(1..=9).contains(&number) {
                return None;
            }
            Box::new(SelectTab(number - 1))
        }
    };
    Some(action)
}

/// The keybindings of the registry's default keys.
pub fn default_keymap() -> Vec<Binding> {
    COMMANDS
        .iter()
        .flat_map(|command| {
            command.keys.iter().map(|keys| Binding {
                keys: (*keys).into(),
                action: command.id.into(),
            })
        })
        .collect()
}

/// GPUI bindings for a keymap. Fails on the first unknown action or invalid key sequence.
pub fn key_bindings(keymap: &[Binding], cx: &App) -> Result<Vec<KeyBinding>, String> {
    keymap
        .iter()
        .map(|binding| {
            let action = action_for(&binding.action)
                .ok_or_else(|| format!("keymap.toml: unknown action \"{}\"", binding.action))?;
            KeyBinding::load(
                &binding.keys,
                action,
                None,
                false,
                None,
                cx.keyboard_mapper().as_ref(),
            )
            .map_err(|e| format!("keymap.toml: keys \"{}\": {e}", binding.keys))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_id_has_an_action() {
        for command in COMMANDS {
            assert!(action_for(command.id).is_some(), "{}", command.id);
        }
        assert!(action_for("tab.select.0").is_none());
        assert!(action_for("tab.select.10").is_none());
        assert!(action_for("nope").is_none());
    }

    #[test]
    fn no_two_default_bindings_share_keys() {
        let mut seen = std::collections::HashSet::new();
        for binding in default_keymap() {
            let keys = config::keymap::normalize(&binding.keys);
            assert!(seen.insert(keys.clone()), "{keys} is bound twice");
        }
    }

    #[test]
    fn default_keymap_has_one_binding_per_key() {
        let keymap = default_keymap();
        let keys: usize = COMMANDS.iter().map(|c| c.keys.len()).sum();
        assert_eq!(keymap.len(), keys);
        assert!(keymap.iter().any(|b| b.keys == "cmd-{" && b.action == "tab.previous"));
    }
}
