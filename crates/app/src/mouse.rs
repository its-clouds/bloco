// Adapted from zed-industries/zed, crates/terminal/src/mappings/mouse.rs
// (commit c4273271092b0287f7d181f616bd5171e0a2f70c, GPL-3.0-or-later).
// Changes: the grid-point helpers are replaced by `cell_at` in terminal_view.rs,
// because this app keeps all coordinates in viewport cells.

use gpui::{Modifiers, MouseButton};
use term_session::TermMode;

/// Mouse report encoding, as in xterm: SGR when the app asked for it, otherwise
/// the classic (optionally UTF-8) format.
enum MouseFormat {
    Sgr,
    Normal(bool),
}

impl MouseFormat {
    fn from_mode(mode: TermMode) -> Self {
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseFormat::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseFormat::Normal(true)
        } else {
            MouseFormat::Normal(false)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MouseButtonCode {
    LeftButton = 0,
    MiddleButton = 1,
    RightButton = 2,
    LeftMove = 32,
    MiddleMove = 33,
    RightMove = 34,
    NoneMove = 35,
    ScrollUp = 64,
    ScrollDown = 65,
    Other = 99,
}

impl MouseButtonCode {
    fn from_move_button(e: Option<MouseButton>) -> Self {
        match e {
            Some(MouseButton::Left) => MouseButtonCode::LeftMove,
            Some(MouseButton::Middle) => MouseButtonCode::MiddleMove,
            Some(MouseButton::Right) => MouseButtonCode::RightMove,
            Some(MouseButton::Navigate(_)) => MouseButtonCode::Other,
            None => MouseButtonCode::NoneMove,
        }
    }

    fn from_button(e: MouseButton) -> Self {
        match e {
            MouseButton::Left => MouseButtonCode::LeftButton,
            MouseButton::Middle => MouseButtonCode::MiddleButton,
            MouseButton::Right => MouseButtonCode::RightButton,
            MouseButton::Navigate(_) => MouseButtonCode::Other,
        }
    }

    fn from_scroll(lines: i32) -> Self {
        if lines > 0 {
            MouseButtonCode::ScrollUp
        } else {
            MouseButtonCode::ScrollDown
        }
    }

    fn is_other(&self) -> bool {
        matches!(self, MouseButtonCode::Other)
    }
}

/// Reports for `lines` wheel notches at a viewport cell. Empty when the app has
/// not enabled mouse reporting.
pub fn scroll_report(
    point: (usize, usize),
    lines: i32,
    modifiers: Modifiers,
    mode: TermMode,
) -> Vec<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return Vec::new();
    }
    let report = mouse_report(
        point,
        MouseButtonCode::from_scroll(lines),
        true,
        modifiers,
        MouseFormat::from_mode(mode),
    );
    report
        .map(|report| vec![report; lines.unsigned_abs() as usize])
        .unwrap_or_default()
}

/// Arrow keys for wheel scrolling on the alternate screen, which has no
/// scrollback. `lines` > 0 scrolls up.
pub fn alt_scroll(lines: i32) -> Vec<u8> {
    let cmd = if lines > 0 { b'A' } else { b'B' };

    let mut content = Vec::with_capacity(lines.unsigned_abs() as usize * 3);
    for _ in 0..lines.abs() {
        content.push(0x1b);
        content.push(b'O');
        content.push(cmd);
    }
    content
}

pub fn mouse_button_report(
    point: (usize, usize),
    button: MouseButton,
    modifiers: Modifiers,
    pressed: bool,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = MouseButtonCode::from_button(button);
    if !button.is_other() && mode.intersects(TermMode::MOUSE_MODE) {
        mouse_report(
            point,
            button,
            pressed,
            modifiers,
            MouseFormat::from_mode(mode),
        )
    } else {
        None
    }
}

pub fn mouse_moved_report(
    point: (usize, usize),
    button: Option<MouseButton>,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = MouseButtonCode::from_move_button(button);

    if !button.is_other() && mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG) {
        // Only drags are reported in drag mode, so block NoneMove.
        if mode.contains(TermMode::MOUSE_DRAG) && matches!(button, MouseButtonCode::NoneMove) {
            None
        } else {
            mouse_report(point, button, true, modifiers, MouseFormat::from_mode(mode))
        }
    } else {
        None
    }
}

/// Generates the bytes for one mouse event at a viewport cell.
fn mouse_report(
    point: (usize, usize),
    button: MouseButtonCode,
    pressed: bool,
    modifiers: Modifiers,
    format: MouseFormat,
) -> Option<Vec<u8>> {
    let mut mods = 0;
    if modifiers.shift {
        mods += 4;
    }
    if modifiers.alt {
        mods += 8;
    }
    if modifiers.control {
        mods += 16;
    }

    match format {
        MouseFormat::Sgr => Some(sgr_mouse_report(point, button as u8 + mods, pressed).into_bytes()),
        MouseFormat::Normal(utf8) => {
            if pressed {
                normal_mouse_report(point, button as u8 + mods, utf8)
            } else {
                normal_mouse_report(point, 3 + mods, utf8)
            }
        }
    }
}

fn normal_mouse_report(point: (usize, usize), button: u8, utf8: bool) -> Option<Vec<u8>> {
    let (line, column) = point;
    let max_point = if utf8 { 2015 } else { 223 };

    if line >= max_point || column >= max_point {
        return None;
    }

    let mut msg = vec![b'\x1b', b'[', b'M', 32 + button];

    let mouse_pos_encode = |pos: usize| -> Vec<u8> {
        let pos = 32 + 1 + pos;
        let first = 0xC0 + pos / 64;
        let second = 0x80 + (pos & 63);
        vec![first as u8, second as u8]
    };

    if utf8 && column >= 95 {
        msg.append(&mut mouse_pos_encode(column));
    } else {
        msg.push(32 + 1 + column as u8);
    }

    if utf8 && line >= 95 {
        msg.append(&mut mouse_pos_encode(line));
    } else {
        msg.push(32 + 1 + line as u8);
    }

    Some(msg)
}

fn sgr_mouse_report(point: (usize, usize), button: u8, pressed: bool) -> String {
    let c = if pressed { 'M' } else { 'm' };
    let (line, column) = point;
    format!("\x1b[<{};{};{}{}", button, column + 1, line + 1, c)
}
