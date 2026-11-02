//! Built-in themes. Each one colors the terminal palette and the UI chrome
//! (tab bar, dividers, command palette).

/// A color as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u32);

/// Colors for the UI around the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub tab_bar: Rgb,
    pub tab_active: Rgb,
    pub tab_inactive: Rgb,
    /// Background of a tab's close button under the pointer.
    pub close_hover: Rgb,
    /// Text on the tab bar and in the palette.
    pub text: Rgb,
    /// Secondary text, such as keybindings in the palette.
    pub muted: Rgb,
    pub divider: Rgb,
    pub overlay: Rgb,
    /// Background of the selected palette row.
    pub selected: Rgb,
    pub accent: Rgb,
    pub error: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
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

impl Theme {
    /// Tomorrow Night, close to what most dark terminals ship with.
    pub fn dark() -> Self {
        Self {
            name: "dark",
            background: Rgb(0x1d1f21),
            foreground: Rgb(0xc5c8c6),
            cursor: Rgb(0xc5c8c6),
            selection: Rgb(0x373b41),
            find_match: Rgb(0xf0c674),
            find_current: Rgb(0xde935f),
            find_text: Rgb(0x1d1f21),
            ansi: [
                Rgb(0x1d1f21),
                Rgb(0xcc6666),
                Rgb(0xb5bd68),
                Rgb(0xf0c674),
                Rgb(0x81a2be),
                Rgb(0xb294bb),
                Rgb(0x8abeb7),
                Rgb(0xc5c8c6),
                Rgb(0x666666),
                Rgb(0xd54e53),
                Rgb(0xb9ca4a),
                Rgb(0xe7c547),
                Rgb(0x7aa6da),
                Rgb(0xc397d8),
                Rgb(0x70c0b1),
                Rgb(0xeaeaea),
            ],
            chrome: Chrome {
                tab_bar: Rgb(0x141517),
                tab_active: Rgb(0x1d1f21),
                tab_inactive: Rgb(0x25282c),
                close_hover: Rgb(0x373b41),
                text: Rgb(0xc5c8c6),
                muted: Rgb(0x7c8086),
                divider: Rgb(0x373b41),
                overlay: Rgb(0x25282c),
                selected: Rgb(0x373b41),
                accent: Rgb(0x81a2be),
                error: Rgb(0xcc6666),
            },
        }
    }

    /// One Light, with the same ANSI order as the dark theme.
    pub fn light() -> Self {
        Self {
            name: "light",
            background: Rgb(0xfafafa),
            foreground: Rgb(0x383a42),
            cursor: Rgb(0x526fff),
            selection: Rgb(0xd7dae0),
            find_match: Rgb(0xffe08a),
            find_current: Rgb(0xff9f43),
            find_text: Rgb(0x383a42),
            ansi: [
                Rgb(0x383a42),
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
                tab_bar: Rgb(0xe6e6e6),
                tab_active: Rgb(0xfafafa),
                tab_inactive: Rgb(0xd8d8d8),
                close_hover: Rgb(0xc8c8c8),
                text: Rgb(0x383a42),
                muted: Rgb(0x8a8f99),
                divider: Rgb(0xc8c8c8),
                overlay: Rgb(0xffffff),
                selected: Rgb(0xe0e4ea),
                accent: Rgb(0x4078f2),
                error: Rgb(0xb3261e),
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
