//! Settings for Bloco: `config.toml`, `keymap.toml` and the built-in
//! themes. The files live in `~/.config/bloco/`.
//!
//! No UI code here. The app reads the files with [`Files::read`], parses them
//! with [`Settings::parse`], and compares the raw text to decide when to reload.

pub mod edit;
pub mod import;
pub mod keymap;
pub mod theme;

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

pub use keymap::{Binding, UNBOUND};
pub use theme::{Chrome, Rgb, Scheme, Theme};

/// `~/.config/bloco`. `None` when `HOME` is not set.
pub fn default_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".config/bloco"))
}

/// The text of each settings file. `None` when a file does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Files {
    pub config: Option<String>,
    pub keymap: Option<String>,
    /// Theme files in `themes/`, by file name (`nord.itermcolors`). The bytes,
    /// not text: iTerm2 exports binary property lists.
    pub themes: BTreeMap<String, Vec<u8>>,
}

impl Files {
    /// Reads `config.toml`, `keymap.toml` and the theme files from `dir`. A
    /// missing file is not an error. Any other read error is returned as text.
    pub fn read(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            config: read_optional(&dir.join("config.toml"))?,
            keymap: read_optional(&dir.join("keymap.toml"))?,
            themes: read_themes(&dir.join("themes"))?,
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

/// Every theme file in `dir`. A missing directory has no themes.
fn read_themes(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut themes = BTreeMap::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if import::theme_name(file_name).is_none() {
            continue;
        }
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        themes.insert(file_name.to_string(), data);
    }
    Ok(themes)
}

/// The theme names `config.toml` accepts: the built-in themes and the theme
/// files, in that order.
pub fn theme_names(files: &Files) -> Vec<String> {
    let mut names: Vec<String> = theme::NAMES.iter().map(|name| name.to_string()).collect();
    for file in files.themes.keys() {
        if let Some(name) = import::theme_name(file) {
            if !names.iter().any(|known| known == name) {
                names.push(name.to_string());
            }
        }
    }
    names
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

/// The lowest opacity accepted. Lower values make the text hard to read.
pub const MIN_OPACITY: f32 = 0.2;

/// Window settings in `[window]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    /// From `0.2` to `1.0`. Below `1.0` the window shows what is behind it.
    pub background_opacity: f32,
    /// Blurs what is behind a see-through window (macOS). Has no effect at `1.0`.
    pub background_blur: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            background_opacity: 1.0,
            background_blur: false,
        }
    }
}

/// Terminal settings in `[terminal]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Whether programs may set the clipboard with OSC 52.
    pub clipboard_write: ClipboardWrite,
}

/// Whether output may write the system clipboard with OSC 52. Any program can
/// send it, a remote one over ssh too. Reads are never answered.
///
/// In TOML: `"allow"` or `"deny"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardWrite {
    /// OSC 52 replaces the clipboard.
    #[default]
    Allow,
    /// OSC 52 is ignored.
    Deny,
}

/// The top-level keys of `config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    /// A built-in theme (`"dark"` or `"light"`), or the name of a file in
    /// `themes/` without its extension.
    pub theme: String,
    pub option_as_meta: OptionAsMeta,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            theme: "dark".into(),
            option_as_meta: OptionAsMeta::default(),
            window: WindowConfig::default(),
            terminal: TerminalConfig::default(),
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
        let opacity = config.window.background_opacity;
        if !(MIN_OPACITY..=1.0).contains(&opacity) {
            return Err(format!(
                "config.toml: window.background_opacity must be from {MIN_OPACITY} to 1.0, not {opacity}"
            ));
        }
        let theme = resolve_theme(&config.theme, &files.themes)?;
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

/// The theme named in `config.toml`: a theme file in `themes/` wins over a
/// built-in with the same name.
fn resolve_theme(name: &str, themes: &BTreeMap<String, Vec<u8>>) -> Result<Theme, String> {
    for (file, data) in themes {
        if import::theme_name(file) == Some(name) {
            let scheme = import::parse(file, data).map_err(|e| format!("themes/{file}: {e}"))?;
            return Ok(scheme.into_theme(name));
        }
    }
    theme::by_name(name).ok_or_else(|| {
        format!(
            "config.toml: unknown theme \"{name}\" (use one of: {})",
            theme::NAMES.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(config: Option<&str>) -> Result<Settings, String> {
        let files = Files {
            config: config.map(str::to_string),
            ..Files::default()
        };
        Settings::parse(&files, &[])
    }

    fn with_theme_file(file: &str, text: &str) -> Files {
        Files {
            themes: BTreeMap::from([(file.to_string(), text.as_bytes().to_vec())]),
            ..Files::default()
        }
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

    #[test]
    fn window_defaults_are_opaque_and_unblurred() {
        let settings = parse(None).unwrap();
        assert_eq!(settings.config.window, WindowConfig::default());
        assert_eq!(settings.config.window.background_opacity, 1.0);
        assert!(!settings.config.window.background_blur);
    }

    #[test]
    fn window_opacity_must_be_in_range() {
        let error = parse(Some("[window]\nbackground_opacity = 0.05\n")).unwrap_err();
        assert!(error.contains("background_opacity must be from 0.2 to 1.0"), "{error}");
        let error = parse(Some("[window]\nbackground_opacity = 1.5\n")).unwrap_err();
        assert!(error.contains("background_opacity"), "{error}");
        let settings = parse(Some("[window]\nbackground_opacity = 0.85\nbackground_blur = true\n")).unwrap();
        assert_eq!(settings.config.window.background_opacity, 0.85);
        assert!(settings.config.window.background_blur);
    }

    #[test]
    fn clipboard_write_defaults_to_allow() {
        let settings = parse(None).unwrap();
        assert_eq!(settings.config.terminal.clipboard_write, ClipboardWrite::Allow);
        let settings = parse(Some("[terminal]\n")).unwrap();
        assert_eq!(settings.config.terminal.clipboard_write, ClipboardWrite::Allow);
    }

    #[test]
    fn clipboard_write_accepts_allow_and_deny() {
        let value = |text: &str| {
            parse(Some(&format!("[terminal]\nclipboard_write = {text}\n"))).map(|s| s.config.terminal.clipboard_write)
        };
        assert_eq!(value("\"allow\""), Ok(ClipboardWrite::Allow));
        assert_eq!(value("\"deny\""), Ok(ClipboardWrite::Deny));
    }

    #[test]
    fn clipboard_write_rejects_other_values() {
        let error = parse(Some("[terminal]\nclipboard_write = \"ask\"\n")).unwrap_err();
        assert!(error.starts_with("config.toml:"), "{error}");
        assert!(error.contains("ask"), "{error}");
        assert!(parse(Some("[terminal]\nclipboard_write = true\n")).is_err());
        assert!(parse(Some("[terminal]\nclipboard = \"deny\"\n")).is_err());
    }

    #[test]
    fn a_theme_file_is_selected_by_its_name() {
        let itermcolors = "<plist><dict>\
            <key>Background Color</key><dict>\
              <key>Red Component</key><real>0</real>\
              <key>Green Component</key><real>0</real>\
              <key>Blue Component</key><real>1</real></dict>\
            </dict></plist>";
        let files = Files {
            config: Some("theme = \"ocean\"\n".into()),
            ..with_theme_file("ocean.itermcolors", itermcolors)
        };
        // The file is missing the foreground and the ANSI entries, so it fails.
        let error = Settings::parse(&files, &[]).unwrap_err();
        assert!(error.starts_with("themes/ocean.itermcolors:"), "{error}");
    }

    #[test]
    fn a_theme_file_with_the_same_name_as_a_builtin_wins() {
        let base16 = (0..16)
            .map(|index| format!("base{index:02X}: \"{:06x}\"\n", index * 0x010101))
            .collect::<String>();
        let files = Files {
            config: Some("theme = \"light\"\n".into()),
            ..with_theme_file("light.yaml", &base16)
        };
        let settings = Settings::parse(&files, &[]).unwrap();
        assert_eq!(settings.theme.background, Rgb(0));
    }

    #[test]
    fn theme_names_list_builtins_then_files() {
        let files = with_theme_file("nord.yaml", "");
        assert_eq!(theme_names(&files), ["dark", "light", "nord"]);
        assert_eq!(theme_names(&Files::default()), ["dark", "light"]);
    }

    #[test]
    fn unknown_theme_lists_the_builtins() {
        let error = parse(Some("theme = \"solarized\"\n")).unwrap_err();
        assert!(error.contains("(use one of: dark, light)"), "{error}");
    }
}
