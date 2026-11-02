//! Settings for Bloco: `config.toml`, `keymap.toml` and the built-in
//! themes. The files live in `~/.config/bloco/`.
//!
//! No UI code here. The app reads the files with [`Files::read`], parses them
//! with [`Settings::parse`], and compares the raw text to decide when to reload.

pub mod keymap;
pub mod theme;

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

pub use keymap::{Binding, UNBOUND};
pub use theme::{Chrome, Rgb, Theme};

/// `~/.config/bloco`. `None` when `HOME` is not set.
pub fn default_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".config/bloco"))
}

/// The text of each settings file. `None` when a file does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Files {
    pub config: Option<String>,
    pub keymap: Option<String>,
}

impl Files {
    /// Reads `config.toml` and `keymap.toml` from `dir`. A missing file is not
    /// an error. Any other read error is returned as text.
    pub fn read(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            config: read_optional(&dir.join("config.toml"))?,
            keymap: read_optional(&dir.join("keymap.toml"))?,
        })
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Font settings in `[font]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Family name as the system knows it, for example "Menlo".
    pub family: String,
    /// Size in points.
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrainsMonoNL Nerd Font".into(),
            size: 14.0,
        }
    }
}

/// The top-level keys of `config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    /// Name of a built-in theme: `"dark"` or `"light"`.
    pub theme: String,
    pub option_as_meta: OptionAsMeta,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            theme: "dark".into(),
            option_as_meta: OptionAsMeta::default(),
        }
    }
}

/// Whether the Option key sends Escape before the key (Meta), as in most
/// Linux terminals, or types a character, as in Terminal.app.
///
/// In TOML: `false`, `true`, `"left"` or `"right"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptionAsMeta {
    /// Option types characters (dead keys and IME still work).
    #[default]
    Off,
    /// Both Option keys send Meta.
    Both,
    /// Only the left Option key sends Meta.
    Left,
    /// Only the right Option key sends Meta.
    Right,
}

impl<'de> Deserialize<'de> for OptionAsMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Side(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bool(false) => Ok(Self::Off),
            Raw::Bool(true) => Ok(Self::Both),
            Raw::Side(side) => match side.as_str() {
                "left" => Ok(Self::Left),
                "right" => Ok(Self::Right),
                other => Err(D::Error::custom(format!(
                    "option_as_meta must be false, true, \"left\" or \"right\", not \"{other}\""
                ))),
            },
        }
    }
}

/// Everything the app reads from the settings files.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub config: Config,
    pub theme: Theme,
    /// The default bindings with the user's bindings applied. See [`keymap::merge`].
    pub keymap: Vec<Binding>,
}

impl Settings {
    /// Parses the files. A missing file uses its defaults. Errors name the file
    /// and what is wrong with it. Action names are not checked here: the app
    /// knows which actions exist.
    pub fn parse(files: &Files, default_keymap: &[Binding]) -> Result<Self, String> {
        let config = match &files.config {
            Some(text) => toml::from_str::<Config>(text).map_err(|e| format!("config.toml: {e}"))?,
            None => Config::default(),
        };
        let theme = theme::by_name(&config.theme).ok_or_else(|| {
            format!(
                "config.toml: unknown theme \"{}\" (use \"dark\" or \"light\")",
                config.theme
            )
        })?;
        let user = match &files.keymap {
            Some(text) => keymap::parse(text).map_err(|e| format!("keymap.toml: {e}"))?,
            None => Vec::new(),
        };
        Ok(Self {
            config,
            theme,
            keymap: keymap::merge(default_keymap, &user),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(config: Option<&str>) -> Result<Settings, String> {
        let files = Files {
            config: config.map(str::to_string),
            keymap: None,
        };
        Settings::parse(&files, &[])
    }

    #[test]
    fn missing_files_give_defaults() {
        let settings = parse(None).unwrap();
        assert_eq!(settings.config, Config::default());
        assert_eq!(settings.theme.name, "dark");
        assert!(settings.keymap.is_empty());
    }

    #[test]
    fn font_fields_override_defaults() {
        let settings = parse(Some("[font]\nfamily = \"Menlo\"\nsize = 12.5\n")).unwrap();
        assert_eq!(settings.config.font.family, "Menlo");
        assert_eq!(settings.config.font.size, 12.5);
    }

    #[test]
    fn partial_font_table_keeps_the_other_default() {
        let settings = parse(Some("[font]\nsize = 16\n")).unwrap();
        assert_eq!(settings.config.font.family, FontConfig::default().family);
        assert_eq!(settings.config.font.size, 16.0);
    }

    #[test]
    fn option_as_meta_accepts_bools_and_sides() {
        let value = |text: &str| parse(Some(&format!("option_as_meta = {text}\n"))).map(|s| s.config.option_as_meta);
        assert_eq!(value("false"), Ok(OptionAsMeta::Off));
        assert_eq!(value("true"), Ok(OptionAsMeta::Both));
        assert_eq!(value("\"left\""), Ok(OptionAsMeta::Left));
        assert_eq!(value("\"right\""), Ok(OptionAsMeta::Right));
    }

    #[test]
    fn option_as_meta_rejects_other_values() {
        let error = parse(Some("option_as_meta = \"both\"\n")).unwrap_err();
        assert!(error.contains("option_as_meta must be"), "{error}");
    }

    #[test]
    fn unknown_keys_are_errors_with_the_file_name() {
        let error = parse(Some("fnt = 3\n")).unwrap_err();
        assert!(error.starts_with("config.toml:"), "{error}");
        assert!(error.contains("fnt"), "{error}");
    }

    #[test]
    fn unknown_theme_is_an_error() {
        let error = parse(Some("theme = \"solarized\"\n")).unwrap_err();
        assert!(error.contains("unknown theme \"solarized\""), "{error}");
    }

    #[test]
    fn light_theme_is_selected_by_name() {
        let settings = parse(Some("theme = \"light\"\n")).unwrap();
        assert_eq!(settings.theme.name, "light");
    }
}
