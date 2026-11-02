//! Changes single values in `config.toml`. The rest of the file is kept as it
//! is: comments, blank lines, key order and quoting. The settings UI writes
//! through here, so a hand-written file keeps its notes.

use std::io::Write;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::OptionAsMeta;

/// Sets `key` in `[table]` to `value` and returns the new text. An empty
/// `table` means the top level. A missing table is added at the end.
///
/// An existing value keeps its comment on the same line. A key's comment on
/// the line above it is kept too.
pub fn set(text: &str, table: &str, key: &str, value: impl Into<Value>) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("config.toml: {e}"))?;
    let target: &mut Table = if table.is_empty() {
        doc.as_table_mut()
    } else {
        doc.entry(table)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| format!("config.toml: \"{table}\" is not a table"))?
    };
    match target.get_mut(key) {
        Some(item) if item.is_value() => {
            let decor = item.as_value().map(|old| old.decor().clone()).unwrap_or_default();
            let mut new: Value = value.into();
            *new.decor_mut() = decor;
            *item = Item::Value(new);
        }
        _ => {
            target.insert(key, Item::Value(value.into()));
        }
    }
    Ok(doc.to_string())
}

/// Sets `[font] size`, in points.
pub fn set_font_size(text: &str, size: f32) -> Result<String, String> {
    set(text, "font", "size", f64::from(size))
}

/// Sets the top-level `theme`. The name is not checked here: [`crate::Settings::parse`] does that.
pub fn set_theme(text: &str, name: &str) -> Result<String, String> {
    set(text, "", "theme", name)
}

/// Sets the top-level `option_as_meta`, in the form [`OptionAsMeta`] reads.
pub fn set_option_as_meta(text: &str, value: OptionAsMeta) -> Result<String, String> {
    match value {
        OptionAsMeta::Off => set(text, "", "option_as_meta", false),
        OptionAsMeta::Both => set(text, "", "option_as_meta", true),
        OptionAsMeta::Left => set(text, "", "option_as_meta", "left"),
        OptionAsMeta::Right => set(text, "", "option_as_meta", "right"),
    }
}

/// Sets `[window] background_opacity`, clamped to the range the config accepts.
pub fn set_background_opacity(text: &str, opacity: f32) -> Result<String, String> {
    // Rounded to hundredths, so the file says `0.2` and not `0.20000000298`.
    let opacity = f64::from(opacity.clamp(crate::MIN_OPACITY, 1.0));
    let opacity = (opacity * 100.0).round() / 100.0;
    set(text, "window", "background_opacity", opacity)
}

/// Sets `[window] background_blur`.
pub fn set_background_blur(text: &str, blur: bool) -> Result<String, String> {
    set(text, "window", "background_blur", blur)
}

/// Writes `text` to `path` through a temporary file, so a crash never leaves a
/// half-written config.
pub fn save(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = path.with_extension("toml.tmp");
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# Bloco settings. Edit by hand or from the settings panel.

# Colors.
theme = \"dark\"

[font]
# The family name as the system knows it.
family = \"Menlo\" # keep this one
size = 13.0
";

    #[test]
    fn an_unchanged_file_round_trips() {
        assert_eq!(set(FILE, "font", "size", 13.0).unwrap(), FILE);
    }

    #[test]
    fn a_value_changes_and_its_comments_stay() {
        let text = set(FILE, "font", "size", 15.0).unwrap();
        assert!(text.contains("size = 15.0\n"), "{text}");
        assert!(text.contains("# The family name as the system knows it."), "{text}");
        assert!(text.contains("family = \"Menlo\" # keep this one"), "{text}");
        assert!(text.starts_with("# Bloco settings."), "{text}");
    }

    #[test]
    fn a_top_level_value_changes() {
        let text = set(FILE, "", "theme", "light").unwrap();
        assert!(text.contains("theme = \"light\"\n"), "{text}");
        assert!(text.contains("# Colors."), "{text}");
    }

    #[test]
    fn a_missing_table_is_added_at_the_end() {
        let text = set(FILE, "window", "background_opacity", 0.9).unwrap();
        assert!(text.ends_with("[window]\nbackground_opacity = 0.9\n"), "{text}");
        assert!(text.contains("# Colors."), "{text}");
    }

    #[test]
    fn a_missing_key_is_added_to_its_table() {
        let text = set(FILE, "font", "bold", true).unwrap();
        assert!(text.contains("size = 13.0\nbold = true"), "{text}");
    }

    #[test]
    fn a_table_that_is_not_a_table_is_an_error() {
        let error = set("font = 3\n", "font", "size", 12.0).unwrap_err();
        assert!(error.contains("\"font\" is not a table"), "{error}");
    }

    #[test]
    fn broken_toml_is_an_error_and_nothing_is_written() {
        assert!(set("[font\n", "font", "size", 12.0).is_err());
    }

    #[test]
    fn typed_setters_write_values_that_parse_back() {
        let text = set_option_as_meta(FILE, OptionAsMeta::Left).unwrap();
        let text = set_background_opacity(&text, 0.5).unwrap();
        let text = set_background_blur(&text, true).unwrap();
        let text = set_theme(&text, "nord").unwrap();
        let text = set_font_size(&text, 16.0).unwrap();
        assert!(text.contains("option_as_meta = \"left\""), "{text}");
        assert!(text.contains("background_blur = true"), "{text}");
        assert!(text.contains("theme = \"nord\""), "{text}");
        assert!(text.contains("size = 16.0"), "{text}");
        assert!(text.contains("# keep this one"), "{text}");
        assert!(text.contains("background_opacity = 0.5"), "{text}");
    }

    #[test]
    fn opacity_is_clamped_to_the_accepted_range() {
        let text = set_background_opacity("", 0.01).unwrap();
        assert!(text.contains("background_opacity = 0.2"), "{text}");
    }

    #[test]
    fn option_as_meta_values_round_trip() {
        for value in [OptionAsMeta::Off, OptionAsMeta::Both, OptionAsMeta::Left, OptionAsMeta::Right] {
            let text = set_option_as_meta("", value).unwrap();
            let settings = crate::Settings::parse(
                &crate::Files {
                    config: Some(text),
                    ..crate::Files::default()
                },
                &[],
            )
            .unwrap();
            assert_eq!(settings.config.option_as_meta, value);
        }
    }

    #[test]
    fn written_text_parses_back_with_the_new_value() {
        let text = set(FILE, "font", "size", 18.5).unwrap();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["font"]["size"].as_float(), Some(18.5));
        assert_eq!(parsed["font"]["family"].as_str(), Some("Menlo"));
    }
}
