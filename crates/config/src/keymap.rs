//! `keymap.toml`: overrides for the default keybindings.
//!
//! ```toml
//! [[bind]]
//! keys = "cmd-k cmd-t"   # a chord: keystrokes separated by a space
//! action = "tab.new"
//!
//! [[bind]]
//! keys = "cmd-w"
//! action = "none"        # removes the default binding for these keys
//! ```
//!
//! A keystroke is modifiers and a key joined by `-`. Modifiers: `ctrl`, `alt`,
//! `shift`, `cmd`, `fn`. Keys are GPUI key names, for example `t`, `up`, `f1`,
//! `enter`. Write a shifted symbol as the symbol you type: Cmd+Shift+[ is
//! `cmd-{` or `cmd-shift-[`. Both mean the same keystroke.

use serde::Deserialize;

/// The action name that removes a default binding.
pub const UNBOUND: &str = "none";

/// Accepted modifier names, in the order keystrokes are written.
const MODIFIERS: &[&str] = &["ctrl", "alt", "shift", "cmd", "fn"];

/// What each symbol key gives with Shift on a US layout. GPUI reports these
/// shifted symbols as the symbol itself, with the shift flag cleared.
const US_SHIFTED: &[(char, char)] = &[
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    ('\\', '|'),
    (';', ':'),
    ('\'', '"'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
    ('`', '~'),
];

/// One binding: a key sequence and the action id it runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub keys: String,
    pub action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    bind: Vec<Binding>,
}

/// Parses `keymap.toml`. Checks the syntax of each key sequence. Whether a key
/// name exists is checked by the app, which knows GPUI's names.
pub fn parse(text: &str) -> Result<Vec<Binding>, String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    for binding in &file.bind {
        validate_keys(&binding.keys).map_err(|e| format!("\"{}\": {e}", binding.keys))?;
    }
    Ok(file.bind)
}

/// Checks that every keystroke in `keys` has known modifiers and a key name.
pub fn validate_keys(keys: &str) -> Result<(), String> {
    if keys.split_whitespace().next().is_none() {
        return Err("no keys given".into());
    }
    for stroke in keys.split_whitespace() {
        let mut pieces: Vec<&str> = stroke.split('-').collect();
        let key = pieces.pop().unwrap_or_default();
        if key.is_empty() {
            return Err(format!("\"{stroke}\" has no key after the modifiers"));
        }
        if let Some(bad) = pieces.iter().find(|m| !MODIFIERS.contains(m)) {
            return Err(format!("unknown modifier \"{bad}\" (use ctrl, alt, shift, cmd or fn)"));
        }
    }
    Ok(())
}

/// The default bindings, with `user` applied on top.
///
/// A user binding replaces every default with the same keys. `action = "none"`
/// removes those defaults and adds nothing. Other user bindings are appended,
/// so the later one wins when keys repeat, as in GPUI.
pub fn merge(defaults: &[Binding], user: &[Binding]) -> Vec<Binding> {
    let mut merged: Vec<Binding> = defaults
        .iter()
        .filter(|default| !user.iter().any(|u| same_keys(&u.keys, &default.keys)))
        .cloned()
        .collect();
    merged.extend(user.iter().filter(|u| u.action != UNBOUND).cloned());
    for binding in &mut merged {
        binding.keys = normalize(&binding.keys);
    }
    merged
}

/// Two key sequences are the same when their keystrokes match. See [`normalize`].
pub fn same_keys(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

/// The key sequence in the form GPUI reports it. Shift is folded into symbols
/// (`cmd-shift-[` becomes `cmd-{`), an uppercase letter becomes the lowercase
/// letter with shift (`cmd-D` becomes `cmd-shift-d`), and modifiers are in the
/// order of [`MODIFIERS`].
pub fn normalize(keys: &str) -> String {
    keys.split_whitespace()
        .map(normalize_keystroke)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_keystroke(stroke: &str) -> String {
    let (head, key) = stroke.rsplit_once('-').unwrap_or(("", stroke));
    let mut shift = head.split('-').any(|m| m.eq_ignore_ascii_case("shift"));
    let mut key = key.to_string();
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() {
            shift = true;
            key = c.to_ascii_lowercase().to_string();
        } else if shift {
            if let Some(&(_, symbol)) = US_SHIFTED.iter().find(|(base, _)| *base == c) {
                key = symbol.to_string();
                shift = false;
            } else if US_SHIFTED.iter().any(|(_, symbol)| *symbol == c) {
                shift = false;
            }
        }
    }
    let mut out: Vec<&str> = MODIFIERS
        .iter()
        .copied()
        .filter(|m| match *m {
            "shift" => shift,
            _ => head.split('-').any(|p| p.eq_ignore_ascii_case(m)),
        })
        .collect();
    out.push(&key);
    out.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(keys: &str, action: &str) -> Binding {
        Binding {
            keys: keys.into(),
            action: action.into(),
        }
    }

    #[test]
    fn parses_bind_tables() {
        let text = "[[bind]]\nkeys = \"cmd-k cmd-t\"\naction = \"tab.new\"\n";
        assert_eq!(parse(text).unwrap(), vec![binding("cmd-k cmd-t", "tab.new")]);
    }

    #[test]
    fn empty_file_has_no_bindings() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn rejects_unknown_modifiers_and_missing_keys() {
        let error = parse("[[bind]]\nkeys = \"super-t\"\naction = \"tab.new\"\n").unwrap_err();
        assert!(error.contains("unknown modifier \"super\""), "{error}");
        assert!(validate_keys("cmd-").is_err());
        assert!(validate_keys("   ").is_err());
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(parse("[[bind]]\nkeys = \"cmd-t\"\nact = \"x\"\n").is_err());
    }

    #[test]
    fn same_keys_ignores_modifier_order_and_case() {
        assert!(same_keys("shift-cmd-t", "cmd-shift-T"));
        assert!(same_keys("cmd-k cmd-t", "cmd-k cmd-t"));
        assert!(!same_keys("cmd-k cmd-t", "cmd-t cmd-k"));
        assert!(!same_keys("cmd-t", "cmd-shift-t"));
    }

    #[test]
    fn user_binding_replaces_default_with_same_keys() {
        let defaults = [binding("cmd-t", "tab.new"), binding("cmd-w", "pane.close")];
        let user = [binding("cmd-t", "palette.toggle")];
        let merged = merge(&defaults, &user);
        assert_eq!(
            merged,
            vec![binding("cmd-w", "pane.close"), binding("cmd-t", "palette.toggle")]
        );
    }

    #[test]
    fn none_removes_a_default_and_adds_nothing() {
        let defaults = [binding("cmd-w", "pane.close")];
        let user = [binding("cmd-w", UNBOUND)];
        assert!(merge(&defaults, &user).is_empty());
    }

    #[test]
    fn shifted_symbols_match_what_gpui_reports() {
        assert_eq!(normalize("cmd-shift-["), "cmd-{");
        assert_eq!(normalize("shift-cmd-]"), "cmd-}");
        assert_eq!(normalize("cmd-shift-}"), "cmd-}");
        assert_eq!(normalize("cmd-shift-1"), "cmd-!");
        assert_eq!(normalize("cmd-shift-d"), "shift-cmd-d");
        assert_eq!(normalize("cmd-D"), "shift-cmd-d");
        assert_eq!(normalize("ctrl-alt-left"), "ctrl-alt-left");
        assert_eq!(normalize("cmd-k cmd-t"), "cmd-k cmd-t");
    }

    #[test]
    fn merged_keys_are_normalized_for_gpui() {
        let defaults = [binding("cmd-{", "tab.previous")];
        let user = [binding("shift-cmd-[", "palette.toggle")];
        let merged = merge(&defaults, &user);
        assert_eq!(merged, vec![binding("cmd-{", "palette.toggle")]);
    }

    #[test]
    fn new_chords_are_appended_after_defaults() {
        let defaults = [binding("cmd-t", "tab.new")];
        let user = [binding("cmd-k cmd-n", "tab.new")];
        let merged = merge(&defaults, &user);
        assert_eq!(merged.last(), Some(&binding("cmd-k cmd-n", "tab.new")));
        assert_eq!(merged.len(), 2);
    }
}
