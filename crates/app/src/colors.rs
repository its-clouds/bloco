//! The colors of the terminal grid and of the UI around it, built from a
//! theme in `crates/config`.

use config::{Rgb, Theme};
use gpui::{Hsla, Rgba, rgb};
use term_session::{Color, NamedColor};

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The window chrome: tab bar, dividers, palette.
pub struct Chrome {
    pub tab_bar: Hsla,
    pub tab_active: Hsla,
    pub tab_inactive: Hsla,
    pub close_hover: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub divider: Hsla,
    pub overlay: Hsla,
    pub selected: Hsla,
    pub accent: Hsla,
    pub error: Hsla,
}

pub struct Palette {
    ansi: [Hsla; 16],
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    pub find_match: Hsla,
    pub find_current: Hsla,
    pub find_text: Hsla,
    pub chrome: Chrome,
}

fn hsla(color: Rgb) -> Hsla {
    rgb(color.0).into()
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let chrome = &theme.chrome;
        Self {
            ansi: theme.ansi.map(hsla),
            foreground: hsla(theme.foreground),
            background: hsla(theme.background),
            cursor: hsla(theme.cursor),
            selection: hsla(theme.selection),
            find_match: hsla(theme.find_match),
            find_current: hsla(theme.find_current),
            find_text: hsla(theme.find_text),
            chrome: Chrome {
                tab_bar: hsla(chrome.tab_bar),
                tab_active: hsla(chrome.tab_active),
                tab_inactive: hsla(chrome.tab_inactive),
                close_hover: hsla(chrome.close_hover),
                text: hsla(chrome.text),
                muted: hsla(chrome.muted),
                divider: hsla(chrome.divider),
                overlay: hsla(chrome.overlay),
                selected: hsla(chrome.selected),
                accent: hsla(chrome.accent),
                error: hsla(chrome.error),
            },
        }
    }

    pub fn resolve(&self, color: Color) -> Hsla {
        match color {
            Color::Spec(rgb) => rgb8(rgb.r, rgb.g, rgb.b),
            Color::Indexed(index) => self.indexed(index),
            Color::Named(named) => self.named(named),
        }
    }

    fn indexed(&self, index: u8) -> Hsla {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                let index = (index - 16) as usize;
                rgb8(
                    CUBE_LEVELS[index / 36],
                    CUBE_LEVELS[(index / 6) % 6],
                    CUBE_LEVELS[index % 6],
                )
            }
            232..=255 => {
                let level = 8 + (index - 232) * 10;
                rgb8(level, level, level)
            }
        }
    }

    fn named(&self, named: NamedColor) -> Hsla {
        let index = named as usize;
        match named {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::DimForeground => dim(self.foreground),
            _ if index < 16 => self.ansi[index],
            // The DimBlack..DimWhite variants follow Cursor (258) in order.
            _ if (259..267).contains(&index) => dim(self.ansi[index - 259]),
            _ => self.foreground,
        }
    }
}

/// SGR 2 (faint): the text is drawn at about two thirds of its lightness.
pub fn dim(color: Hsla) -> Hsla {
    Hsla {
        l: color.l * 0.66,
        ..color
    }
}

fn rgb8(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    }
    .into()
}
