//! Built-in themes. Each one colors the terminal palette and the UI chrome
//! (tab bar, dividers, command palette).

/// A color as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u32);

/// Colors for the UI around the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    /// Title bar and tab strip background.
    pub tab_bar: Rgb,
    /// Background of the active tab pill.
    pub tab_active: Rgb,
    /// Background of an inactive tab pill under the pointer.
    pub tab_hover: Rgb,
    /// Background of a tab's close button under the pointer.
    pub close_hover: Rgb,
    /// Text on the tab bar and in the palette.
    pub text: Rgb,
    /// Secondary text, such as keybindings in the palette.
    pub muted: Rgb,
    pub divider: Rgb,
    /// Background of the palette and find bar.
    pub overlay: Rgb,
    /// Raised surfaces inside the terminal, such as the input area and chips.
    pub surface: Rgb,
    /// Background of the selected palette row.
    pub selected: Rgb,
    pub accent: Rgb,
    pub error: Rgb,
    pub success: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// Background of every find match.
    pub find_match: Rgb,
    /// Background of the current find match.
    pub find_current: Rgb,
    /// Text drawn over a find match.
    pub find_text: Rgb,
    /// The 16 ANSI colors, black through bright white.
    pub ansi: [Rgb; 16],
    pub chrome: Chrome,
}

/// The names `config.toml` accepts for `theme`.
pub const NAMES: &[&str] = &["dark", "light"];

pub fn by_name(name: &str) -> Option<Theme> {
    match name {
        "dark" => Some(Theme::dark()),
        "light" => Some(Theme::light()),
        _ => None,
    }
}

/// The colors an imported theme file provides. The rest of the theme is derived
/// from them in [`Scheme::into_theme`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// Black through bright white, in ANSI order.
    pub ansi: [Rgb; 16],
}

impl Scheme {
    /// Builds a full theme. The find colors come from the yellow and orange
    /// ANSI colors, and the chrome is a step lighter or darker than the background.
    pub fn into_theme(self, name: &str) -> Theme {
        let (bg, fg, ansi) = (self.background, self.foreground, self.ansi);
        Theme {
            name: name.to_string(),
            background: bg,
            foreground: fg,
            cursor: self.cursor,
            selection: self.selection,
            find_match: ansi[11],
            find_current: ansi[9],
            find_text: bg,
            ansi,
            chrome: Chrome {
                tab_bar: mix(bg, Rgb(0), 0.25),
                tab_active: mix(bg, fg, 0.1),
                tab_hover: mix(bg, fg, 0.05),
                close_hover: mix(bg, fg, 0.2),
                text: fg,
                muted: mix(fg, bg, 0.45),
                divider: mix(bg, fg, 0.2),
                overlay: mix(bg, fg, 0.06),
                surface: mix(bg, fg, 0.04),
                selected: mix(bg, fg, 0.2),
                accent: ansi[4],
                error: ansi[1],
                success: ansi[2],
            },
        }
    }
}

/// `a` moved `t` of the way toward `b` (0.0 is `a`, 1.0 is `b`), per channel.
pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let channel = |shift: u32| {
        let x = ((a.0 >> shift) & 0xff) as f32;
        let y = ((b.0 >> shift) & 0xff) as f32;
        (x + (y - x) * t).round() as u32
    };
    Rgb((channel(16) << 16) | (channel(8) << 8) | channel(0))
}

impl Theme {
    /// Bloco Night: a cool near-black with a soft periwinkle accent.
    pub fn dark() -> Self {
        Self {
            name: "dark".into(),
            background: Rgb(0x15171c),
            foreground: Rgb(0xd5d9e3),
            cursor: Rgb(0x8da2ff),
            selection: Rgb(0x2c3350),
            find_match: Rgb(0xe2b86b),
            find_current: Rgb(0xf08c5a),
            find_text: Rgb(0x15171c),
            ansi: [
                Rgb(0x23262e),
                Rgb(0xf0717d),
                Rgb(0x9fd17a),
                Rgb(0xe5c07b),
                Rgb(0x7aa2f7),
                Rgb(0xc39bf2),
                Rgb(0x6fd0e0),
                Rgb(0xc8ccd6),
                Rgb(0x5c6372),
                Rgb(0xff8a95),
                Rgb(0xb4e391),
                Rgb(0xf2d398),
                Rgb(0x9bb8ff),
                Rgb(0xd5b4ff),
                Rgb(0x8fe0ee),
                Rgb(0xf2f4f8),
            ],
            chrome: Chrome {
                tab_bar: Rgb(0x0f1014),
                tab_active: Rgb(0x22252e),
                tab_hover: Rgb(0x1a1c23),
                close_hover: Rgb(0x2e323d),
                text: Rgb(0xe3e6ee),
                muted: Rgb(0x7d8494),
                divider: Rgb(0x262932),
                overlay: Rgb(0x1c1e25),
                surface: Rgb(0x1b1d24),
                selected: Rgb(0x2a2f42),
                accent: Rgb(0x8da2ff),
                error: Rgb(0xf0717d),
                success: Rgb(0x9fd17a),
            },
        }
    }

    /// Bloco Day: the light twin of Bloco Night.
    pub fn light() -> Self {
        Self {
            name: "light".into(),
            background: Rgb(0xfbfbfc),
            foreground: Rgb(0x2f3340),
            cursor: Rgb(0x4f6bed),
            selection: Rgb(0xd9e0fb),
            find_match: Rgb(0xffe08a),
            find_current: Rgb(0xffb070),
            find_text: Rgb(0x2f3340),
            ansi: [
                Rgb(0x2f3340),
                Rgb(0xe45649),
                Rgb(0x50a14f),
                Rgb(0xc18401),
                Rgb(0x0184bc),
                Rgb(0xa626a4),
                Rgb(0x0997b3),
                Rgb(0xfafafa),
                Rgb(0x4f525e),
                Rgb(0xe06c75),
                Rgb(0x98c379),
                Rgb(0xe5c07b),
                Rgb(0x61afef),
                Rgb(0xc678dd),
                Rgb(0x56b6c2),
                Rgb(0xffffff),
            ],
            chrome: Chrome {
                tab_bar: Rgb(0xeceef2),
                tab_active: Rgb(0xffffff),
                tab_hover: Rgb(0xe2e5eb),
                close_hover: Rgb(0xd6dae2),
                text: Rgb(0x2f3340),
                muted: Rgb(0x7a8091),
                divider: Rgb(0xe0e3e9),
                overlay: Rgb(0xffffff),
                surface: Rgb(0xf2f3f6),
                selected: Rgb(0xe3e8fc),
                accent: Rgb(0x4f6bed),
                error: Rgb(0xd03b4a),
                success: Rgb(0x3f9a4c),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_resolves() {
        for name in NAMES {
            assert_eq!(by_name(name).unwrap().name, *name);
        }
        assert!(by_name("solarized").is_none());
    }
}
