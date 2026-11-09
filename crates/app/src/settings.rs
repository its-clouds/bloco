//! The rows of the settings panel: their labels, what they show, and what one
//! step does to the text of `config.toml`. The panel draws the rows and saves
//! the file; this module does the parts that can be tested without a window.

use config::{ClipboardWrite, Config, OptionAsMeta, edit};

use crate::zoom::{MAX_SIZE, MIN_SIZE};

/// The settings the panel edits. Each one is a single key in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    FontSize,
    Theme,
    Opacity,
    Blur,
    OptionAsMeta,
    ClipboardWrite,
}

/// The rows, top to bottom.
pub const ROWS: [Setting; 6] = [
    Setting::FontSize,
    Setting::Theme,
    Setting::Opacity,
    Setting::Blur,
    Setting::OptionAsMeta,
    Setting::ClipboardWrite,
];

/// How much one step changes the opacity.
const OPACITY_STEP: f32 = 0.05;

/// The order `option_as_meta` cycles through.
const META_ORDER: [OptionAsMeta; 4] = [
    OptionAsMeta::Off,
    OptionAsMeta::Both,
    OptionAsMeta::Left,
    OptionAsMeta::Right,
];

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Setting::FontSize => "Font size",
            Setting::Theme => "Theme",
            Setting::Opacity => "Background opacity",
            Setting::Blur => "Background blur",
            Setting::OptionAsMeta => "Option as Meta",
            Setting::ClipboardWrite => "Programs can set clipboard",
        }
    }

    /// The value as the row shows it.
    pub fn value_text(self, config: &Config) -> String {
        match self {
            Setting::FontSize => format!("{} pt", config.font.size),
            Setting::Theme => config.theme.clone(),
            Setting::Opacity => format!("{:.0}%", config.window.background_opacity * 100.0),
            Setting::Blur => on_off(config.window.background_blur).into(),
            Setting::OptionAsMeta => match config.option_as_meta {
                OptionAsMeta::Off => "Off",
                OptionAsMeta::Both => "Both",
                OptionAsMeta::Left => "Left",
                OptionAsMeta::Right => "Right",
            }
            .into(),
            Setting::ClipboardWrite => match config.terminal.clipboard_write {
                ClipboardWrite::Allow => "Allow",
                ClipboardWrite::Deny => "Deny",
            }
            .into(),
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// The text of `config.toml` after one step on `setting`. `step` is `-1` for
/// the left arrow and `1` for the right one. `themes` is the list the theme
/// row cycles through, in order. `config` is the config as it is now.
pub fn step(
    text: &str,
    setting: Setting,
    config: &Config,
    themes: &[String],
    step: i32,
) -> Result<String, String> {
    match setting {
        Setting::FontSize => {
            let size = (config.font.size + step as f32).clamp(MIN_SIZE, MAX_SIZE);
            edit::set_font_size(text, size)
        }
        Setting::Theme => {
            if themes.is_empty() {
                return Err("no themes to choose from".into());
            }
            let current = themes.iter().position(|name| *name == config.theme).unwrap_or(0);
            let next = (current as i32 + step).rem_euclid(themes.len() as i32) as usize;
            edit::set_theme(text, &themes[next])
        }
        Setting::Opacity => edit::set_background_opacity(
            text,
            config.window.background_opacity + OPACITY_STEP * step as f32,
        ),
        Setting::Blur => edit::set_background_blur(text, !config.window.background_blur),
        Setting::OptionAsMeta => {
            let current = META_ORDER
                .iter()
                .position(|value| *value == config.option_as_meta)
                .unwrap_or(0);
            let next = (current as i32 + step).rem_euclid(META_ORDER.len() as i32) as usize;
            edit::set_option_as_meta(text, META_ORDER[next])
        }
        Setting::ClipboardWrite => {
            let next = match config.terminal.clipboard_write {
                ClipboardWrite::Allow => ClipboardWrite::Deny,
                ClipboardWrite::Deny => ClipboardWrite::Allow,
            };
            edit::set_clipboard_write(text, next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The config a file gives. The file must be valid.
    fn parsed(text: &str) -> Config {
        config::Settings::parse(
            &config::Files {
                config: Some(text.into()),
                ..config::Files::default()
            },
            &[],
        )
        .unwrap()
        .config
    }

    #[test]
    fn values_read_as_the_panel_shows_them() {
        let config = parsed("[font]\nsize = 12.5\n[window]\nbackground_opacity = 0.85\nbackground_blur = true\n");
        assert_eq!(Setting::FontSize.value_text(&config), "12.5 pt");
        assert_eq!(Setting::Opacity.value_text(&config), "85%");
        assert_eq!(Setting::Blur.value_text(&config), "On");
        assert_eq!(Setting::OptionAsMeta.value_text(&config), "Off");
        assert_eq!(Setting::ClipboardWrite.value_text(&config), "Allow");
    }

    #[test]
    fn font_size_steps_and_stops_at_the_limits() {
        let config = parsed("[font]\nsize = 14\n");
        let up = step("", Setting::FontSize, &config, &[], 1).unwrap();
        assert!(up.contains("size = 15.0"), "{up}");
        let at_min = parsed("[font]\nsize = 6\n");
        let down = step("[font]\nsize = 6\n", Setting::FontSize, &at_min, &[], -1).unwrap();
        assert!(down.contains("size = 6.0"), "{down}");
    }

    #[test]
    fn theme_cycles_through_the_names_and_wraps() {
        let names: Vec<String> = ["dark", "light", "nord"].map(String::from).to_vec();
        let config = Config {
            theme: "nord".into(),
            ..Config::default()
        };
        let next = step("", Setting::Theme, &config, &names, 1).unwrap();
        assert!(next.contains("theme = \"dark\""), "{next}");
        let previous = step("", Setting::Theme, &config, &names, -1).unwrap();
        assert!(previous.contains("theme = \"light\""), "{previous}");
    }

    #[test]
    fn theme_needs_a_name_to_choose() {
        assert!(step("", Setting::Theme, &Config::default(), &[], 1).is_err());
    }

    #[test]
    fn opacity_steps_by_five_percent_and_stops_at_the_minimum() {
        let config = parsed("[window]\nbackground_opacity = 0.9\n");
        let down = step("", Setting::Opacity, &config, &[], -1).unwrap();
        assert!(down.contains("background_opacity = 0.85"), "{down}");
        let floor = parsed("[window]\nbackground_opacity = 0.2\n");
        let still = step("", Setting::Opacity, &floor, &[], -1).unwrap();
        assert!(still.contains("background_opacity = 0.2"), "{still}");
    }

    #[test]
    fn blur_toggles() {
        let text = step("", Setting::Blur, &Config::default(), &[], 1).unwrap();
        assert!(text.contains("background_blur = true"), "{text}");
    }

    #[test]
    fn option_as_meta_cycles_through_all_four() {
        let mut current = Config::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(current.option_as_meta);
            let text = step("", Setting::OptionAsMeta, &current, &[], 1).unwrap();
            current = parsed(&text);
        }
        assert_eq!(seen, META_ORDER);
    }

    #[test]
    fn clipboard_write_toggles() {
        let text = step("", Setting::ClipboardWrite, &Config::default(), &[], 1).unwrap();
        assert!(text.contains("clipboard_write = \"deny\""), "{text}");
        let denied = parsed(&text);
        assert_eq!(Setting::ClipboardWrite.value_text(&denied), "Deny");
        let back = step(&text, Setting::ClipboardWrite, &denied, &[], -1).unwrap();
        assert_eq!(parsed(&back).terminal.clipboard_write, ClipboardWrite::Allow);
    }
}
