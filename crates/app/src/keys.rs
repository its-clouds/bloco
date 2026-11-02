// Adapted from zed-industries/zed, crates/terminal/src/mappings/keys.rs
// (commit c4273271092b0287f7d181f616bd5171e0a2f70c, GPL-3.0-or-later).
// Changes: uses term_session::TermMode. The caller passes option_as_meta = false
// unless the config asks for Meta, so on macOS Option types characters (dead keys,
// IME) by default.

use std::borrow::Cow;

use gpui::Keystroke;
use term_session::TermMode;

#[derive(Debug, PartialEq, Eq)]
enum TerminalModifiers {
    None,
    Alt,
    Ctrl,
    Shift,
    CtrlShift,
    Other,
}

impl TerminalModifiers {
    fn new(ks: &Keystroke) -> Self {
        match (
            ks.modifiers.alt,
            ks.modifiers.control,
            ks.modifiers.shift,
            ks.modifiers.platform,
        ) {
            (false, false, false, false) => TerminalModifiers::None,
            (true, false, false, false) => TerminalModifiers::Alt,
            (false, true, false, false) => TerminalModifiers::Ctrl,
            (false, false, true, false) => TerminalModifiers::Shift,
            (false, true, true, false) => TerminalModifiers::CtrlShift,
            _ => TerminalModifiers::Other,
        }
    }

    fn any(&self) -> bool {
        match &self {
            TerminalModifiers::None => false,
            TerminalModifiers::Alt => true,
            TerminalModifiers::Ctrl => true,
            TerminalModifiers::Shift => true,
            TerminalModifiers::CtrlShift => true,
            TerminalModifiers::Other => true,
        }
    }
}

/// A command-line edit that a default keybinding runs (see `commands.rs`). Option+arrows
/// and Option+Backspace work by word, Cmd+arrows and Cmd+Backspace by line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    WordBack,
    WordForward,
    LineStart,
    LineEnd,
    DeleteWordBack,
    DeleteToLineStart,
}

/// The edit a keystroke makes by default, or `None` if it makes none.
pub fn text_edit_for(keystroke: &Keystroke) -> Option<TextEdit> {
    let modifiers = &keystroke.modifiers;
    let alt_only = modifiers.alt && !modifiers.control && !modifiers.shift && !modifiers.platform;
    let cmd_only = modifiers.platform && !modifiers.alt && !modifiers.control && !modifiers.shift;
    match (keystroke.key.as_ref(), alt_only, cmd_only) {
        ("left", true, _) => Some(TextEdit::WordBack),
        ("right", true, _) => Some(TextEdit::WordForward),
        ("backspace", true, _) => Some(TextEdit::DeleteWordBack),
        ("left", _, true) => Some(TextEdit::LineStart),
        ("right", _, true) => Some(TextEdit::LineEnd),
        ("backspace", _, true) => Some(TextEdit::DeleteToLineStart),
        _ => None,
    }
}

/// The bytes an edit sends. On the main screen they are the emacs-keymap bytes that the
/// shell's line editor reads (zsh, bash and fish by default). In a full-screen app they are
/// the xterm key for the same modifiers, and Cmd+Backspace sends nothing. The encoder has
/// no Kitty keyboard protocol mode, so there is nothing to respect here yet.
pub fn text_edit_bytes(edit: TextEdit, mode: TermMode) -> Option<&'static str> {
    if !mode.contains(TermMode::ALT_SCREEN) {
        return Some(match edit {
            TextEdit::WordBack => "\x1bb",
            TextEdit::WordForward => "\x1bf",
            TextEdit::LineStart => "\x01",
            TextEdit::LineEnd => "\x05",
            TextEdit::DeleteWordBack => "\x1b\x7f",
            TextEdit::DeleteToLineStart => "\x15",
        });
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    match edit {
        TextEdit::WordBack => Some("\x1b[1;3D"),
        TextEdit::WordForward => Some("\x1b[1;3C"),
        TextEdit::LineStart if app_cursor => Some("\x1bOH"),
        TextEdit::LineStart => Some("\x1b[H"),
        TextEdit::LineEnd if app_cursor => Some("\x1bOF"),
        TextEdit::LineEnd => Some("\x1b[F"),
        TextEdit::DeleteWordBack => Some("\x1b\x7f"),
        TextEdit::DeleteToLineStart => None,
    }
}

pub fn to_esc_str(
    keystroke: &Keystroke,
    mode: TermMode,
    option_as_meta: bool,
) -> Option<Cow<'static, str>> {
    let modifiers = TerminalModifiers::new(keystroke);

    // Text edits are sent by the keymap, through `text_edit_bytes`, whatever
    // `option_as_meta` says. Returning `None` lets the keystroke reach the keymap.
    if text_edit_for(keystroke).is_some() {
        return None;
    }

    // Manual Bindings including modifiers
    let manual_esc_str: Option<&'static str> = match (keystroke.key.as_ref(), &modifiers) {
        //Basic special keys
        ("tab", TerminalModifiers::None) => Some("\x09"),
        ("escape", TerminalModifiers::None) => Some("\x1b"),
        ("enter", TerminalModifiers::None) => Some("\x0d"),
        ("enter", TerminalModifiers::Shift) => Some("\x0a"),
        ("enter", TerminalModifiers::Alt) => Some("\x1b\x0d"),
        ("backspace", TerminalModifiers::None) => Some("\x7f"),
        //Interesting escape codes
        ("tab", TerminalModifiers::Shift) => Some("\x1b[Z"),
        ("backspace", TerminalModifiers::Ctrl) => Some("\x08"),
        ("backspace", TerminalModifiers::Alt) => Some("\x1b\x7f"),
        ("backspace", TerminalModifiers::Shift) => Some("\x7f"),
        ("space", TerminalModifiers::Ctrl) => Some("\x00"),
        ("home", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOH"),
        ("home", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[H"),
        ("end", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOF"),
        ("end", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[F"),
        ("up", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOA"),
        ("up", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[A"),
        ("down", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOB"),
        ("down", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[B"),
        ("right", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOC"),
        ("right", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[C"),
        ("left", TerminalModifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOD"),
        ("left", TerminalModifiers::None) if !mode.contains(TermMode::APP_CURSOR) => Some("\x1b[D"),
        ("back", TerminalModifiers::None) => Some("\x7f"),
        ("insert", TerminalModifiers::None) => Some("\x1b[2~"),
        ("delete", TerminalModifiers::None) => Some("\x1b[3~"),
        ("pageup", TerminalModifiers::None) => Some("\x1b[5~"),
        ("pagedown", TerminalModifiers::None) => Some("\x1b[6~"),
        ("f1", TerminalModifiers::None) => Some("\x1bOP"),
        ("f2", TerminalModifiers::None) => Some("\x1bOQ"),
        ("f3", TerminalModifiers::None) => Some("\x1bOR"),
        ("f4", TerminalModifiers::None) => Some("\x1bOS"),
        ("f5", TerminalModifiers::None) => Some("\x1b[15~"),
        ("f6", TerminalModifiers::None) => Some("\x1b[17~"),
        ("f7", TerminalModifiers::None) => Some("\x1b[18~"),
        ("f8", TerminalModifiers::None) => Some("\x1b[19~"),
        ("f9", TerminalModifiers::None) => Some("\x1b[20~"),
        ("f10", TerminalModifiers::None) => Some("\x1b[21~"),
        ("f11", TerminalModifiers::None) => Some("\x1b[23~"),
        ("f12", TerminalModifiers::None) => Some("\x1b[24~"),
        ("f13", TerminalModifiers::None) => Some("\x1b[25~"),
        ("f14", TerminalModifiers::None) => Some("\x1b[26~"),
        ("f15", TerminalModifiers::None) => Some("\x1b[28~"),
        ("f16", TerminalModifiers::None) => Some("\x1b[29~"),
        ("f17", TerminalModifiers::None) => Some("\x1b[31~"),
        ("f18", TerminalModifiers::None) => Some("\x1b[32~"),
        ("f19", TerminalModifiers::None) => Some("\x1b[33~"),
        ("f20", TerminalModifiers::None) => Some("\x1b[34~"),
        // NumpadEnter, Action::Esc("\n".into());
        //Mappings for caret notation keys
        ("a", TerminalModifiers::Ctrl) => Some("\x01"), //1
        ("A", TerminalModifiers::CtrlShift) => Some("\x01"), //1
        ("b", TerminalModifiers::Ctrl) => Some("\x02"), //2
        ("B", TerminalModifiers::CtrlShift) => Some("\x02"), //2
        ("c", TerminalModifiers::Ctrl) => Some("\x03"), //3
        ("C", TerminalModifiers::CtrlShift) => Some("\x03"), //3
        ("d", TerminalModifiers::Ctrl) => Some("\x04"), //4
        ("D", TerminalModifiers::CtrlShift) => Some("\x04"), //4
        ("e", TerminalModifiers::Ctrl) => Some("\x05"), //5
        ("E", TerminalModifiers::CtrlShift) => Some("\x05"), //5
        ("f", TerminalModifiers::Ctrl) => Some("\x06"), //6
        ("F", TerminalModifiers::CtrlShift) => Some("\x06"), //6
        ("g", TerminalModifiers::Ctrl) => Some("\x07"), //7
        ("G", TerminalModifiers::CtrlShift) => Some("\x07"), //7
        ("h", TerminalModifiers::Ctrl) => Some("\x08"), //8
        ("H", TerminalModifiers::CtrlShift) => Some("\x08"), //8
        ("i", TerminalModifiers::Ctrl) => Some("\x09"), //9
        ("I", TerminalModifiers::CtrlShift) => Some("\x09"), //9
        ("j", TerminalModifiers::Ctrl) => Some("\x0a"), //10
        ("J", TerminalModifiers::CtrlShift) => Some("\x0a"), //10
        ("k", TerminalModifiers::Ctrl) => Some("\x0b"), //11
        ("K", TerminalModifiers::CtrlShift) => Some("\x0b"), //11
        ("l", TerminalModifiers::Ctrl) => Some("\x0c"), //12
        ("L", TerminalModifiers::CtrlShift) => Some("\x0c"), //12
        ("m", TerminalModifiers::Ctrl) => Some("\x0d"), //13
        ("M", TerminalModifiers::CtrlShift) => Some("\x0d"), //13
        ("n", TerminalModifiers::Ctrl) => Some("\x0e"), //14
        ("N", TerminalModifiers::CtrlShift) => Some("\x0e"), //14
        ("o", TerminalModifiers::Ctrl) => Some("\x0f"), //15
        ("O", TerminalModifiers::CtrlShift) => Some("\x0f"), //15
        ("p", TerminalModifiers::Ctrl) => Some("\x10"), //16
        ("P", TerminalModifiers::CtrlShift) => Some("\x10"), //16
        ("q", TerminalModifiers::Ctrl) => Some("\x11"), //17
        ("Q", TerminalModifiers::CtrlShift) => Some("\x11"), //17
        ("r", TerminalModifiers::Ctrl) => Some("\x12"), //18
        ("R", TerminalModifiers::CtrlShift) => Some("\x12"), //18
        ("s", TerminalModifiers::Ctrl) => Some("\x13"), //19
        ("S", TerminalModifiers::CtrlShift) => Some("\x13"), //19
        ("t", TerminalModifiers::Ctrl) => Some("\x14"), //20
        ("T", TerminalModifiers::CtrlShift) => Some("\x14"), //20
        ("u", TerminalModifiers::Ctrl) => Some("\x15"), //21
        ("U", TerminalModifiers::CtrlShift) => Some("\x15"), //21
        ("v", TerminalModifiers::Ctrl) => Some("\x16"), //22
        ("V", TerminalModifiers::CtrlShift) => Some("\x16"), //22
        ("w", TerminalModifiers::Ctrl) => Some("\x17"), //23
        ("W", TerminalModifiers::CtrlShift) => Some("\x17"), //23
        ("x", TerminalModifiers::Ctrl) => Some("\x18"), //24
        ("X", TerminalModifiers::CtrlShift) => Some("\x18"), //24
        ("y", TerminalModifiers::Ctrl) => Some("\x19"), //25
        ("Y", TerminalModifiers::CtrlShift) => Some("\x19"), //25
        ("z", TerminalModifiers::Ctrl) => Some("\x1a"), //26
        ("Z", TerminalModifiers::CtrlShift) => Some("\x1a"), //26
        ("@", TerminalModifiers::Ctrl) => Some("\x00"), //0
        ("[", TerminalModifiers::Ctrl) => Some("\x1b"), //27
        ("\\", TerminalModifiers::Ctrl) => Some("\x1c"), //28
        ("]", TerminalModifiers::Ctrl) => Some("\x1d"), //29
        ("^", TerminalModifiers::Ctrl) => Some("\x1e"), //30
        ("_", TerminalModifiers::Ctrl) => Some("\x1f"), //31
        ("?", TerminalModifiers::Ctrl) => Some("\x7f"), //127
        _ => None,
    };
    if let Some(esc_str) = manual_esc_str {
        return Some(Cow::Borrowed(esc_str));
    }

    // Automated bindings applying modifiers
    if modifiers.any() {
        let modifier_code = modifier_code(keystroke);
        let modified_esc_str = match keystroke.key.as_ref() {
            "up" => Some(format!("\x1b[1;{}A", modifier_code)),
            "down" => Some(format!("\x1b[1;{}B", modifier_code)),
            "right" => Some(format!("\x1b[1;{}C", modifier_code)),
            "left" => Some(format!("\x1b[1;{}D", modifier_code)),
            "f1" => Some(format!("\x1b[1;{}P", modifier_code)),
            "f2" => Some(format!("\x1b[1;{}Q", modifier_code)),
            "f3" => Some(format!("\x1b[1;{}R", modifier_code)),
            "f4" => Some(format!("\x1b[1;{}S", modifier_code)),
            "f5" => Some(format!("\x1b[15;{}~", modifier_code)),
            "f6" => Some(format!("\x1b[17;{}~", modifier_code)),
            "f7" => Some(format!("\x1b[18;{}~", modifier_code)),
            "f8" => Some(format!("\x1b[19;{}~", modifier_code)),
            "f9" => Some(format!("\x1b[20;{}~", modifier_code)),
            "f10" => Some(format!("\x1b[21;{}~", modifier_code)),
            "f11" => Some(format!("\x1b[23;{}~", modifier_code)),
            "f12" => Some(format!("\x1b[24;{}~", modifier_code)),
            "f13" => Some(format!("\x1b[25;{}~", modifier_code)),
            "f14" => Some(format!("\x1b[26;{}~", modifier_code)),
            "f15" => Some(format!("\x1b[28;{}~", modifier_code)),
            "f16" => Some(format!("\x1b[29;{}~", modifier_code)),
            "f17" => Some(format!("\x1b[31;{}~", modifier_code)),
            "f18" => Some(format!("\x1b[32;{}~", modifier_code)),
            "f19" => Some(format!("\x1b[33;{}~", modifier_code)),
            "f20" => Some(format!("\x1b[34;{}~", modifier_code)),
            "insert" => Some(format!("\x1b[2;{}~", modifier_code)),
            "pageup" => Some(format!("\x1b[5;{}~", modifier_code)),
            "pagedown" => Some(format!("\x1b[6;{}~", modifier_code)),
            "end" => Some(format!("\x1b[1;{}F", modifier_code)),
            "home" => Some(format!("\x1b[1;{}H", modifier_code)),
            _ => None,
        };
        if let Some(esc_str) = modified_esc_str {
            return Some(Cow::Owned(esc_str));
        }
    }

    // Handling alt keys. These work for single ASCII characters.
    if (!cfg!(target_os = "macos") || option_as_meta)
        && keystroke.modifiers.alt
        && keystroke.key.is_ascii()
        && keystroke.key.len() == 1
    {
        let key = keystroke.key.chars().next().unwrap();

        if modifiers == TerminalModifiers::Alt {
            return Some(Cow::Owned(format!("\x1b{key}")));
        } else if keystroke.modifiers.shift {
            return Some(Cow::Owned(format!("\x1b{}", key.to_ascii_uppercase())));
        } else if keystroke.modifiers.control && key.is_ascii_lowercase() {
            return Some(Cow::Owned(format!(
                "\x1b{}",
                (key as u8 - b'a' + 1) as char
            )));
        }
    }

    None
}

///   Code     Modifiers
/// ---------+---------------------------
///    2     | Shift
///    3     | Alt
///    4     | Shift + Alt
///    5     | Control
///    6     | Shift + Control
///    7     | Alt + Control
///    8     | Shift + Alt + Control
/// ---------+---------------------------
/// from: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-PC-Style-Function-Keys
fn modifier_code(keystroke: &Keystroke) -> u32 {
    let mut modifier_code = 0;
    if keystroke.modifiers.shift {
        modifier_code |= 1;
    }
    if keystroke.modifiers.alt {
        modifier_code |= 1 << 1;
    }
    if keystroke.modifiers.control {
        modifier_code |= 1 << 2;
    }
    modifier_code + 1
}

#[cfg(test)]
mod test {
    use gpui::Modifiers;

    use super::*;

    #[test]
    fn test_plain_inputs() {
        let ks = Keystroke {
            modifiers: Modifiers {
                control: false,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: "🖖🏻".to_string(), //2 char string
            key_char: None,
        };
        assert_eq!(to_esc_str(&ks, TermMode::NONE, false), None);
    }

    #[test]
    fn test_application_mode() {
        let app_cursor = TermMode::APP_CURSOR;
        let none = TermMode::NONE;

        let up = Keystroke::parse("up").unwrap();
        let down = Keystroke::parse("down").unwrap();
        let left = Keystroke::parse("left").unwrap();
        let right = Keystroke::parse("right").unwrap();

        assert_eq!(to_esc_str(&up, none, false), Some("\x1b[A".into()));
        assert_eq!(to_esc_str(&down, none, false), Some("\x1b[B".into()));
        assert_eq!(to_esc_str(&right, none, false), Some("\x1b[C".into()));
        assert_eq!(to_esc_str(&left, none, false), Some("\x1b[D".into()));

        assert_eq!(to_esc_str(&up, app_cursor, false), Some("\x1bOA".into()));
        assert_eq!(to_esc_str(&down, app_cursor, false), Some("\x1bOB".into()));
        assert_eq!(to_esc_str(&right, app_cursor, false), Some("\x1bOC".into()));
        assert_eq!(to_esc_str(&left, app_cursor, false), Some("\x1bOD".into()));

        let home = Keystroke::parse("home").unwrap();
        let end = Keystroke::parse("end").unwrap();
        assert_eq!(to_esc_str(&home, none, false), Some("\x1b[H".into()));
        assert_eq!(to_esc_str(&end, none, false), Some("\x1b[F".into()));
        assert_eq!(to_esc_str(&home, app_cursor, false), Some("\x1bOH".into()));
        assert_eq!(to_esc_str(&end, app_cursor, false), Some("\x1bOF".into()));

        let shift_up = Keystroke::parse("shift-up").unwrap();
        let shift_down = Keystroke::parse("shift-down").unwrap();
        let shift_home = Keystroke::parse("shift-home").unwrap();
        let shift_end = Keystroke::parse("shift-end").unwrap();
        assert_eq!(to_esc_str(&shift_up, none, false), Some("\x1b[1;2A".into()));
        assert_eq!(
            to_esc_str(&shift_down, none, false),
            Some("\x1b[1;2B".into())
        );
        assert_eq!(
            to_esc_str(&shift_home, none, false),
            Some("\x1b[1;2H".into())
        );
        assert_eq!(
            to_esc_str(&shift_end, none, false),
            Some("\x1b[1;2F".into())
        );
    }

    #[test]
    fn test_ctrl_codes() {
        let letters_lower = 'a'..='z';
        let letters_upper = 'A'..='Z';
        let mode = TermMode::APP_CURSOR;

        for (lower, upper) in letters_lower.zip(letters_upper) {
            assert_eq!(
                to_esc_str(
                    &Keystroke::parse(&format!("ctrl-shift-{}", lower)).unwrap(),
                    mode,
                    false
                ),
                to_esc_str(
                    &Keystroke::parse(&format!("ctrl-{}", upper)).unwrap(),
                    mode,
                    false
                ),
                "On letter: {}/{}",
                lower,
                upper
            )
        }
    }

    #[test]
    fn alt_is_meta() {
        let ascii_printable = ' '..='~';
        for character in ascii_printable {
            assert_eq!(
                to_esc_str(
                    &Keystroke::parse(&format!("alt-{}", character)).unwrap(),
                    TermMode::NONE,
                    true
                )
                .unwrap(),
                format!("\x1b{}", character)
            );
        }

        // Alt+left and alt+right are text edits now (see `text_edit_for`), so they are not here.
        let gpui_keys = [
            "up", "down", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11",
            "f12", "f13", "f14", "f15", "f16", "f17", "f18", "f19", "f20", "insert", "pageup",
            "pagedown", "end", "home",
        ];

        for key in gpui_keys {
            assert_ne!(
                to_esc_str(
                    &Keystroke::parse(&format!("alt-{}", key)).unwrap(),
                    TermMode::NONE,
                    true
                )
                .unwrap(),
                format!("\x1b{}", key)
            );
        }
    }

    #[test]
    fn default_text_keys_map_to_edits() {
        let edit_of = |keys: &str| text_edit_for(&Keystroke::parse(keys).unwrap());
        let cases = [
            ("alt-left", TextEdit::WordBack),
            ("alt-right", TextEdit::WordForward),
            ("alt-backspace", TextEdit::DeleteWordBack),
            ("cmd-left", TextEdit::LineStart),
            ("cmd-right", TextEdit::LineEnd),
            ("cmd-backspace", TextEdit::DeleteToLineStart),
        ];
        for (keys, edit) in cases {
            assert_eq!(edit_of(keys), Some(edit), "{keys}");
        }

        // Other keys and modifier combinations keep their own meaning.
        let others = [
            "left",
            "shift-alt-left",
            "ctrl-alt-left",
            "alt-cmd-left",
            "cmd-alt-left",
            "cmd-shift-left",
            "alt-b",
        ];
        for keys in others {
            assert_eq!(edit_of(keys), None, "{keys}");
        }
    }

    #[test]
    fn text_edit_keys_are_left_to_the_keymap_for_both_option_as_meta_values() {
        let strokes = [
            "alt-left",
            "alt-right",
            "alt-backspace",
            "cmd-left",
            "cmd-right",
            "cmd-backspace",
        ];
        for keys in strokes {
            let keystroke = Keystroke::parse(keys).unwrap();
            for mode in [TermMode::NONE, TermMode::ALT_SCREEN] {
                for meta in [false, true] {
                    assert_eq!(to_esc_str(&keystroke, mode, meta), None, "{keys}");
                }
            }
        }
    }

    #[test]
    fn main_screen_sends_readline_bytes() {
        let expected = [
            (TextEdit::WordBack, "\x1bb"),
            (TextEdit::WordForward, "\x1bf"),
            (TextEdit::LineStart, "\x01"),
            (TextEdit::LineEnd, "\x05"),
            (TextEdit::DeleteWordBack, "\x1b\x7f"),
            (TextEdit::DeleteToLineStart, "\x15"),
        ];
        let none = TermMode::NONE;
        // zsh sets application cursor keys while it edits a line. The bytes stay the same.
        let app_cursor = TermMode::APP_CURSOR;
        for (edit, bytes) in expected {
            assert_eq!(text_edit_bytes(edit, none), Some(bytes), "{edit:?}");
            assert_eq!(text_edit_bytes(edit, app_cursor), Some(bytes), "{edit:?}");
        }
    }

    #[test]
    fn alternate_screen_sends_xterm_keys() {
        let alt = TermMode::ALT_SCREEN;
        let cases = [
            (TextEdit::WordBack, "\x1b[1;3D"),
            (TextEdit::WordForward, "\x1b[1;3C"),
            (TextEdit::LineStart, "\x1b[H"),
            (TextEdit::LineEnd, "\x1b[F"),
            (TextEdit::DeleteWordBack, "\x1b\x7f"),
        ];
        for (edit, bytes) in cases {
            assert_eq!(text_edit_bytes(edit, alt), Some(bytes), "{edit:?}");
        }
        assert_eq!(text_edit_bytes(TextEdit::DeleteToLineStart, alt), None);

        // Application cursor keys (set by vim) change Home and End.
        let app_cursor = alt | TermMode::APP_CURSOR;
        let cases = [
            (TextEdit::WordBack, "\x1b[1;3D"),
            (TextEdit::LineStart, "\x1bOH"),
            (TextEdit::LineEnd, "\x1bOF"),
        ];
        for (edit, bytes) in cases {
            assert_eq!(text_edit_bytes(edit, app_cursor), Some(bytes), "{edit:?}");
        }
    }

    #[test]
    fn test_ctrl_alt_codes() {
        let letters_lower = 'a'..='z';
        let esc_codes = '\x01'..='\x1a';
        for (character, esc_code) in letters_lower.zip(esc_codes) {
            assert_eq!(
                to_esc_str(
                    &Keystroke::parse(&format!("ctrl-alt-{}", character)).unwrap(),
                    TermMode::NONE,
                    true
                )
                .unwrap(),
                format!("\x1b{}", esc_code)
            );
        }
    }

    #[test]
    fn test_shift_enter_newline() {
        let shift_enter = Keystroke::parse("shift-enter").unwrap();
        let regular_enter = Keystroke::parse("enter").unwrap();
        let mode = TermMode::NONE;

        // Shift-enter should send line feed (newline)
        assert_eq!(to_esc_str(&shift_enter, mode, false), Some("\x0a".into()));

        // Regular enter should still send carriage return
        assert_eq!(to_esc_str(&regular_enter, mode, false), Some("\x0d".into()));
    }

    #[test]
    fn test_modifier_code_calc() {
        //   Code     Modifiers
        // ---------+---------------------------
        //    2     | Shift
        //    3     | Alt
        //    4     | Shift + Alt
        //    5     | Control
        //    6     | Shift + Control
        //    7     | Alt + Control
        //    8     | Shift + Alt + Control
        // ---------+---------------------------
        // from: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-PC-Style-Function-Keys
        assert_eq!(2, modifier_code(&Keystroke::parse("shift-a").unwrap()));
        assert_eq!(3, modifier_code(&Keystroke::parse("alt-a").unwrap()));
        assert_eq!(4, modifier_code(&Keystroke::parse("shift-alt-a").unwrap()));
        assert_eq!(5, modifier_code(&Keystroke::parse("ctrl-a").unwrap()));
        assert_eq!(6, modifier_code(&Keystroke::parse("shift-ctrl-a").unwrap()));
        assert_eq!(7, modifier_code(&Keystroke::parse("alt-ctrl-a").unwrap()));
        assert_eq!(
            8,
            modifier_code(&Keystroke::parse("shift-ctrl-alt-a").unwrap())
        );
    }
}
