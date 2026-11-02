//! The terminal view: keyboard, mouse, clipboard and IME handling. The grid
//! itself is drawn by `terminal_element.rs`.
//!
//! Key handling follows `zed-industries/zed`, crates/terminal_view/src/terminal_view.rs
//! (GPL-3.0-or-later). See NOTICE.

use std::cell::Cell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use async_channel::Receiver;
use config::OptionAsMeta;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InputHandler, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollEvent, SharedString, UTF16Selection, Window, div,
    prelude::*,
};
use term_session::{Link, SelectionKind, Session, SessionEvent, TermMode};

use crate::colors::Palette;
use crate::commands::{
    CopySelection, PasteClipboard, TextDeleteToLineStart, TextDeleteWordBack, TextLineEnd,
    TextLineStart, TextWordBack, TextWordForward,
};
use crate::keys::{TextEdit, text_edit_bytes, to_esc_str};
use crate::mouse::{alt_scroll, mouse_button_report, mouse_moved_report, scroll_report};
use crate::option_key::option_keys_down;
use crate::terminal_element::{Metrics, TerminalElement};

/// What a pane tells its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The mouse was pressed in this pane.
    Activated,
    /// The shell asked to change the window title (OSC 0 or 2).
    Title(String),
    /// The tab label (the running command or the directory) changed.
    TabTitleChanged,
    /// The shell closed the PTY, usually because it exited.
    Exited,
}

/// The settings that change how a terminal looks and types.
#[derive(Clone)]
pub struct TerminalStyle {
    pub palette: Rc<Palette>,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub option_as_meta: OptionAsMeta,
}

/// A URL under the mouse while Cmd is held.
struct HoveredLink {
    line: usize,
    link: Link,
}

pub struct TerminalView {
    session: Arc<Session>,
    focus_handle: FocusHandle,
    palette: Rc<Palette>,
    font_family: SharedString,
    font_size: Pixels,
    option_as_meta: OptionAsMeta,
    metrics: Rc<Cell<Metrics>>,
    /// Preedit text from the input method, not yet committed to the shell.
    marked_text: Option<String>,
    /// The left button is down and the drag extends a selection.
    selecting: bool,
    hovered_link: Option<HoveredLink>,
    /// Wheel movement in pixels that has not added up to a whole line yet.
    scroll_pixels: f32,
    /// The tab label as of the last output, to report only real changes.
    tab_label: String,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl TerminalView {
    /// `events` receives what the session's listener sends. A task forwards
    /// them to this view until the session ends.
    pub fn new(
        session: Arc<Session>,
        events: Receiver<SessionEvent>,
        style: TerminalStyle,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |view, cx| view.handle_session_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let tab_label = tab_label(&session);
        Self {
            session,
            focus_handle: cx.focus_handle(),
            palette: style.palette,
            font_family: style.font_family,
            font_size: style.font_size,
            option_as_meta: style.option_as_meta,
            metrics: Rc::new(Cell::new(Metrics::default())),
            marked_text: None,
            selecting: false,
            hovered_link: None,
            scroll_pixels: 0.0,
            tab_label,
        }
    }

    /// Applies new colors, font or Option-key behavior from a config reload.
    pub fn apply_style(&mut self, style: TerminalStyle, cx: &mut Context<Self>) {
        self.palette = style.palette;
        self.font_family = style.font_family;
        self.font_size = style.font_size;
        self.option_as_meta = style.option_as_meta;
        cx.notify();
    }

    /// The directory the shell is in. `None` if neither OSC 7 nor the OS can say.
    pub fn cwd(&self) -> Option<PathBuf> {
        self.session.cwd()
    }

    /// The tab label: the running command, or the last part of the directory.
    pub fn tab_title(&self) -> String {
        tab_label(&self.session)
    }

    fn handle_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Output => {
                let label = tab_label(&self.session);
                if label != self.tab_label {
                    self.tab_label = label;
                    cx.emit(TerminalEvent::TabTitleChanged);
                }
                cx.notify();
            }
            SessionEvent::Title(title) => cx.emit(TerminalEvent::Title(title)),
            SessionEvent::Exit => cx.emit(TerminalEvent::Exited),
        }
    }

    /// Writes user input to the shell and returns the view to the newest output.
    fn send_input(&self, bytes: &[u8]) {
        self.session.scroll_to_bottom();
        let _ = self.session.write(bytes);
    }

    /// Whether an Option key press sends Meta. Left and right are told apart
    /// with the keyboard state.
    fn option_sends_meta(&self) -> bool {
        match self.option_as_meta {
            OptionAsMeta::Off => false,
            OptionAsMeta::Both => true,
            OptionAsMeta::Left => option_keys_down().0,
            OptionAsMeta::Right => option_keys_down().1,
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.session.mode();
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;

        // Cmd belongs to the app's shortcuts (Cmd+Opt+arrows move between panes),
        // so it is never sent to the shell. Keys without a shortcut are dropped.
        if modifiers.platform {
            return;
        }

        // Option as Meta: send ESC before the key, instead of the character the
        // Option key composes (which is why this comes before the IME check below).
        if modifiers.alt && !modifiers.control && !modifiers.platform && self.option_sends_meta() {
            if let Some(esc) = to_esc_str(keystroke, mode, true) {
                self.send_input(esc.as_bytes());
                cx.stop_propagation();
                return;
            }
        }

        // Printable keys (including dead keys and IME composition) arrive through
        // the input handler, so they are not handled here.
        if event.prefer_character_input && event.keystroke.key_char.is_some() {
            return;
        }

        // Shift+PageUp and Shift+PageDown page through the scrollback. Full-screen
        // apps get the keys instead.
        if modifiers.shift
            && !modifiers.control
            && !modifiers.alt
            && !mode.contains(TermMode::ALT_SCREEN)
        {
            let page = self.metrics.get().rows.max(1) as i32;
            match keystroke.key.as_ref() {
                "pageup" => {
                    self.session.scroll(page);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "pagedown" => {
                    self.session.scroll(-page);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        if let Some(esc) = to_esc_str(keystroke, mode, false) {
            self.send_input(esc.as_bytes());
            cx.stop_propagation();
        }
    }

    /// Text committed by the keyboard or the input method.
    fn commit_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked_text = None;
        if !text.is_empty() {
            self.send_input(text.as_bytes());
        }
        cx.notify();
    }

    fn set_marked_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked_text = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn clear_marked_text(&mut self, cx: &mut Context<Self>) {
        if self.marked_text.take().is_some() {
            cx.notify();
        }
    }

    fn marked_text_range(&self) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn copy(&mut self, _: &CopySelection, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.session.selection_text().filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &PasteClipboard, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let bytes = if self.session.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        };
        self.send_input(&bytes);
    }

    /// Sends a command-line edit to the shell, or the matching key to a full-screen app.
    fn text_edit(&mut self, edit: TextEdit, cx: &mut Context<Self>) {
        if let Some(bytes) = text_edit_bytes(edit, self.session.mode()) {
            self.send_input(bytes.as_bytes());
        }
        cx.notify();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        cx.emit(TerminalEvent::Activated);

        let metrics = self.metrics.get();
        let (line, column, side) = metrics.cell_at(event.position);
        let mode = self.session.mode();

        if event.button == MouseButton::Left && event.modifiers.secondary() {
            if let Some(link) = self.session.url_at(line, column) {
                cx.open_url(&link.url);
                return;
            }
        }

        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            if let Some(bytes) =
                mouse_button_report((line, column), event.button, event.modifiers, true, mode)
            {
                let _ = self.session.write(&bytes);
            }
            return;
        }

        if event.button == MouseButton::Left {
            if event.modifiers.shift && self.session.selection_text().is_some() {
                self.session.update_selection(line, column, side);
            } else {
                let kind = match event.click_count {
                    2 => SelectionKind::Semantic,
                    3 => SelectionKind::Lines,
                    _ => SelectionKind::Simple,
                };
                self.session.start_selection(kind, line, column, side);
            }
            self.selecting = true;
        }
        cx.notify();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let metrics = self.metrics.get();
        let (line, column, _) = metrics.cell_at(event.position);
        let mode = self.session.mode();

        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            if let Some(bytes) =
                mouse_button_report((line, column), event.button, event.modifiers, false, mode)
            {
                let _ = self.session.write(&bytes);
            }
            return;
        }
        if event.button == MouseButton::Left {
            self.selecting = false;
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let metrics = self.metrics.get();
        let (line, column, side) = metrics.cell_at(event.position);
        let mode = self.session.mode();

        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            if let Some(bytes) =
                mouse_moved_report((line, column), event.pressed_button, event.modifiers, mode)
            {
                let _ = self.session.write(&bytes);
            }
            return;
        }

        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            self.session.update_selection(line, column, side);
            cx.notify();
        }

        let hovered = if event.modifiers.secondary() {
            self.session
                .url_at(line, column)
                .map(|link| HoveredLink { line, link })
        } else {
            None
        };
        if hovered.as_ref().map(|h| (h.line, &h.link))
            != self.hovered_link.as_ref().map(|h| (h.line, &h.link))
        {
            self.hovered_link = hovered;
            cx.notify();
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let metrics = self.metrics.get();
        let line_height = f32::from(metrics.line_height);
        self.scroll_pixels += f32::from(event.delta.pixel_delta(metrics.line_height).y);
        let lines = (self.scroll_pixels / line_height).trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_pixels -= lines as f32 * line_height;

        let (line, column, _) = metrics.cell_at(event.position);
        let mode = self.session.mode();
        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            for report in scroll_report((line, column), lines, event.modifiers, mode) {
                let _ = self.session.write(&report);
            }
        } else if mode.contains(TermMode::ALT_SCREEN)
            && mode.contains(TermMode::ALTERNATE_SCROLL)
            && !event.modifiers.shift
        {
            // Full-screen apps without mouse reporting (less, man) take arrow keys.
            let _ = self.session.write(&alt_scroll(lines));
        } else {
            self.session.scroll(lines);
        }
        cx.notify();
    }
}

/// The tab label for a session: the running command, or the last part of the directory.
fn tab_label(session: &Session) -> String {
    match session.current_command() {
        Some(command) => shorten(command.trim(), 32),
        None => session
            .cwd()
            .map_or_else(|| "shell".into(), |dir| dir_label(&dir)),
    }
}

/// Cuts `text` to `max` characters, ending with an ellipsis when it is longer.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// `~` for the home directory, `/` for the root, otherwise the last path part.
fn dir_label(dir: &Path) -> String {
    if std::env::var_os("HOME").is_some_and(|home| Path::new(&home) == dir) {
        return "~".into();
    }
    match dir.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => "/".into(),
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let hovered_link = self
            .hovered_link
            .as_ref()
            .map(|h| (h.line, h.link.first_column, h.link.last_column));

        div()
            .id("terminal")
            .size_full()
            .bg(self.palette.background)
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(
                cx.listener(|view, _: &TextWordBack, _, cx| view.text_edit(TextEdit::WordBack, cx)),
            )
            .on_action(cx.listener(|view, _: &TextWordForward, _, cx| {
                view.text_edit(TextEdit::WordForward, cx)
            }))
            .on_action(
                cx.listener(|view, _: &TextLineStart, _, cx| {
                    view.text_edit(TextEdit::LineStart, cx)
                }),
            )
            .on_action(
                cx.listener(|view, _: &TextLineEnd, _, cx| view.text_edit(TextEdit::LineEnd, cx)),
            )
            .on_action(cx.listener(|view, _: &TextDeleteWordBack, _, cx| {
                view.text_edit(TextEdit::DeleteWordBack, cx)
            }))
            .on_action(cx.listener(|view, _: &TextDeleteToLineStart, _, cx| {
                view.text_edit(TextEdit::DeleteToLineStart, cx)
            }))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll(cx.listener(Self::scroll_wheel))
            .child(TerminalElement {
                view: cx.entity(),
                session: self.session.clone(),
                metrics: self.metrics.clone(),
                palette: self.palette.clone(),
                font_family: self.font_family.clone(),
                font_size: self.font_size,
                focus: self.focus_handle.clone(),
                focused,
                marked_text: self.marked_text.clone(),
                hovered_link,
            })
    }
}

/// Connects the input method (dead keys, IME, the character viewer) to the view.
pub struct TerminalInputHandler {
    pub view: Entity<TerminalView>,
    /// The cursor cell, where the candidate window and the preedit appear.
    pub cursor_bounds: Option<Bounds<Pixels>>,
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        // Always a valid range, so the input method can position its window.
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.view.read(cx).marked_text_range()
    }

    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| view.commit_text(text, cx));
        window.invalidate_character_coordinates();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_marked_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut App,
    ) {
        self.view
            .update(cx, |view, cx| view.set_marked_text(new_text, cx));
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut App) {
        self.view.update(cx, |view, cx| view.clear_marked_text(cx));
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        // The cursor cell is one cell wide, so each preedit column moves right by a cell.
        let mut bounds = self.cursor_bounds?;
        bounds.origin.x += bounds.size.width * range.start as f32;
        Some(bounds)
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        None
    }
}
