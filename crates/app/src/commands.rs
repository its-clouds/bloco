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
    Command { id: "tab.new", title: "New tab", keys: &["cmd-t"] },
    Command { id: "tab.close", title: "Close tab", keys: &[] },
    Command { id: "tab.next", title: "Next tab", keys: &["cmd-}"] },
    Command { id: "tab.previous", title: "Previous tab", keys: &["cmd-{"] },
    Command { id: "tab.select.1", title: "Go to tab 1", keys: &["cmd-1"] },
    Command { id: "tab.select.2", title: "Go to tab 2", keys: &["cmd-2"] },
    Command { id: "tab.select.3", title: "Go to tab 3", keys: &["cmd-3"] },
    Command { id: "tab.select.4", title: "Go to tab 4", keys: &["cmd-4"] },
    Command { id: "tab.select.5", title: "Go to tab 5", keys: &["cmd-5"] },
    Command { id: "tab.select.6", title: "Go to tab 6", keys: &["cmd-6"] },
    Command { id: "tab.select.7", title: "Go to tab 7", keys: &["cmd-7"] },
    Command { id: "tab.select.8", title: "Go to tab 8", keys: &["cmd-8"] },
    Command { id: "tab.select.9", title: "Go to tab 9", keys: &["cmd-9"] },
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
    Command { id: "app.quit", title: "Quit Bloco", keys: &["cmd-q"] },
];

/// The action a command id runs. `None` for an unknown id.
pub fn action_for(id: &str) -> Option<Box<dyn Action>> {
    let action: Box<dyn Action> = match id {
        "palette.toggle" => Box::new(TogglePalette),
        "tab.new" => Box::new(NewTab),
        "tab.close" => Box::new(CloseTab),
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
    fn default_keymap_has_one_binding_per_key() {
        let keymap = default_keymap();
        let keys: usize = COMMANDS.iter().map(|c| c.keys.len()).sum();
        assert_eq!(keymap.len(), keys);
        assert!(keymap.iter().any(|b| b.keys == "cmd-{" && b.action == "tab.previous"));
    }
}
