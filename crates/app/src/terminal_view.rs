//! The terminal view: keyboard, mouse, clipboard and IME handling, and the find
//! bar. The grid itself is drawn by `terminal_element.rs`.
//!
//! Key handling follows `zed-industries/zed`, crates/terminal_view/src/terminal_view.rs
//! (GPL-3.0-or-later). See NOTICE.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use async_channel::Receiver;
use config::OptionAsMeta;
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InputHandler, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollEvent, SharedString, Task, UTF16Selection, Window,
    div, prelude::*, px, relative,
};
use input_editor::{Buffer, HistorySearch, wrap};
use term_session::{Block, FindQuery, Link, Row, SelectionKind, Session, SessionEvent, TermMode};

use crate::block_header::shorten;
use crate::colors::Palette;
use crate::commands::{
    BlockCopyCommand, BlockCopyOutput, BlockNext, BlockPrevious, BlockToggleCollapse,
    CopySelection, FindClose, FindNext, FindOpen, FindPrevious, PasteClipboard,
    TextDeleteToLineStart, TextDeleteWordBack, TextLineEnd, TextLineStart, TextWordBack,
    TextWordForward,
};
use crate::find::{FindState, Pick};
use crate::keys::{TextEdit, text_edit_bytes, text_edit_for, to_esc_str};
use crate::mouse::{alt_scroll, mouse_button_report, mouse_moved_report, scroll_report};
use crate::option_key::option_keys_down;
use crate::prompt::{PromptState, is_text_input, submit_bytes};
use crate::terminal_element::{Metrics, TerminalElement, cell_metrics, grid_size};

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

/// How long new output waits before it reruns an open search. Output during the
/// wait joins the same run, so a busy shell searches a few times a second at most.
const FIND_REFRESH: Duration = Duration::from_millis(150);
const FIND_BAR_WIDTH: f32 = 400.0;
/// Half a blink cycle: the cursor is shown for this long, then hidden for as long.
const CURSOR_BLINK: Duration = Duration::from_millis(530);
/// How long the visual bell tints the pane.
const BELL_FLASH: Duration = Duration::from_millis(150);

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
    /// The find bar, while it is open.
    find: Option<FindState>,
    find_focus: FocusHandle,
    /// The query of the last closed find bar, offered again when it reopens.
    last_find: FindQuery,
    /// The search that is running. Replacing it cancels the older one.
    find_task: Option<Task<()>>,
    /// New output is waiting to rerun the search.
    find_refresh_pending: bool,
    /// Whether the cursor is in the shown half of its blink.
    cursor_on: bool,
    /// The blink timer. A keypress replaces it, so the cursor stays solid while typing.
    blink_task: Option<Task<()>>,
    /// The visual bell is tinting the pane.
    bell_on: bool,
    /// Ends the bell flash. A new bell replaces it.
    bell_task: Option<Task<()>>,
    /// What each visual row of the last frame shows. Mouse positions are mapped
    /// through it. Written by the element.
    layout: Rc<RefCell<Vec<Row>>>,
    /// The block the user selected: navigation starts from it, and copy and
    /// collapse act on it. Cleared by typing, a click elsewhere, or Escape.
    selected_block: Cell<Option<u64>>,
    /// The input editor at the prompt. It is used while the shell is at its prompt.
    prompt: PromptState,
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
        let mut view = Self {
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
            find: None,
            find_focus: cx.focus_handle(),
            last_find: FindQuery::default(),
            find_task: None,
            find_refresh_pending: false,
            cursor_on: true,
            blink_task: None,
            bell_on: false,
            bell_task: None,
            layout: Rc::new(RefCell::new(Vec::new())),
            selected_block: Cell::new(None),
            prompt: PromptState::new(),
        };
        view.restart_cursor_blink(cx);
        view
    }

    /// Shows the cursor and starts a new blink cycle from now. Called on each
    /// keypress and on start. Replacing `blink_task` cancels the old cycle.
    fn restart_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_on = true;
        self.blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK).await;
                let toggled = this.update(cx, |view, cx| {
                    view.cursor_on = !view.cursor_on;
                    cx.notify();
                });
                if toggled.is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// Tints the pane for a moment. A bell during a flash restarts it.
    fn ring_bell(&mut self, cx: &mut Context<Self>) {
        self.bell_on = true;
        self.bell_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BELL_FLASH).await;
            let _ = this.update(cx, |view, cx| {
                view.bell_on = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Applies new colors, font or Option-key behavior from a config reload or a zoom.
    pub fn apply_style(&mut self, style: TerminalStyle, cx: &mut Context<Self>) {
        self.palette = style.palette;
        self.font_family = style.font_family;
        self.font_size = style.font_size;
        self.option_as_meta = style.option_as_meta;
        self.fit_grid(cx);
        cx.notify();
    }

    /// Re-measures the cells at the current font and resizes the grid and the PTY
    /// to the pane's last size. The element does this on each frame it paints. A
    /// pane in a hidden tab does not paint, so the change is made here as well.
    /// A pane that has never been painted has no size yet, and is left alone.
    fn fit_grid(&self, cx: &App) {
        let metrics = self.metrics.get();
        if f32::from(metrics.size.width) <= 0.0 {
            return;
        }
        let (cell_width, line_height) = cell_metrics(cx, &self.font_family, self.font_size);
        let (columns, rows) = grid_size(metrics.size, cell_width, line_height);
        if (columns, rows) != (metrics.columns, metrics.rows) {
            let _ = self.session.resize(columns as u16, rows as u16);
        }
        self.metrics.set(Metrics {
            cell_width,
            line_height,
            columns,
            rows,
            ..metrics
        });
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
                self.schedule_find_refresh(cx);
                cx.notify();
            }
            SessionEvent::Title(title) => cx.emit(TerminalEvent::Title(title)),
            SessionEvent::Exit => cx.emit(TerminalEvent::Exited),
            SessionEvent::Clipboard(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            SessionEvent::Bell => self.ring_bell(cx),
        }
    }

    /// Writes user input to the shell and returns the view to the newest output.
    fn send_input(&self, bytes: &[u8]) {
        self.selected_block.set(None);
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
        self.restart_cursor_blink(cx);
        let mode = self.session.mode();
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;

        // While the shell is at its prompt, the input editor takes the keys first.
        if self.owns_input() && self.prompt_key(event, cx) {
            return;
        }

        // Cmd belongs to the app's shortcuts (Cmd+Opt+arrows move between panes),
        // so it is never sent to the shell. Keys without a shortcut are dropped.
        if modifiers.platform {
            return;
        }

        // Escape clears a selected block. Without one it goes to the shell.
        if keystroke.key == "escape" && self.selected_block.get().is_some() {
            self.selected_block.set(None);
            cx.stop_propagation();
            cx.notify();
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
        self.restart_cursor_blink(cx);
        self.marked_text = None;
        if self.owns_input() {
            self.insert_typed(text);
        } else if !text.is_empty() {
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
        if self.owns_input() {
            let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\x1b', "");
            self.edit_buffer(|buffer| buffer.insert_str(&text));
            cx.notify();
            return;
        }
        let bytes = if self.session.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        };
        self.restart_cursor_blink(cx);
        self.send_input(&bytes);
    }

    /// Sends a command-line edit to the shell, or the matching key to a full-screen app.
    fn text_edit(&mut self, edit: TextEdit, cx: &mut Context<Self>) {
        self.restart_cursor_blink(cx);
        if self.owns_input() {
            match edit {
                TextEdit::DeleteWordBack => self.edit_buffer(Buffer::delete_word_back),
                TextEdit::DeleteToLineStart => self.edit_buffer(Buffer::delete_to_line_start),
                TextEdit::WordBack => self.prompt.buffer.word_back(),
                TextEdit::WordForward => self.prompt.buffer.word_forward(),
                TextEdit::LineStart => self.prompt.buffer.line_start(),
                TextEdit::LineEnd => self.prompt.buffer.line_end(),
            }
            cx.notify();
            return;
        }
        if let Some(bytes) = text_edit_bytes(edit, self.session.mode()) {
            self.send_input(bytes.as_bytes());
        }
        cx.notify();
    }

    /// Cmd+F: opens the find bar, or selects its query when it is open. A
    /// selection in the terminal (its first line) becomes the query.
    fn open_find(&mut self, _: &FindOpen, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut() {
            find.all_selected = !find.query.text.is_empty();
            window.focus(&self.find_focus, cx);
            cx.notify();
            return;
        }
        let selected = self
            .session
            .selection_text()
            .and_then(|text| text.lines().next().map(str::to_string))
            .filter(|line| !line.trim().is_empty());
        let mut query = self.last_find.clone();
        let all_selected = match selected {
            Some(text) => {
                query.text = text;
                false
            }
            None => !query.text.is_empty(),
        };
        self.find = Some(FindState::new(query, all_selected));
        window.focus(&self.find_focus, cx);
        self.run_find(Pick::Newest, cx);
        cx.notify();
    }

    /// Esc or ×: closes the bar and clears the highlights. The view stays where
    /// it is, on the last current match.
    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = self.find.take() {
            self.last_find = find.query;
        }
        self.find_task = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        self.step_find(true, window, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.step_find(false, window, cx);
    }

    /// Moves to the next (older) or previous (newer) match and scrolls to it.
    /// With the bar closed, opens it first.
    fn step_find(&mut self, older: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            self.open_find(&FindOpen, window, cx);
            return;
        };
        let found = if older { find.next() } else { find.previous() };
        if let Some(found) = found {
            self.session.scroll_to_line(found.start.line);
        }
        cx.notify();
    }

    /// Searches the buffer on a background thread with the current query.
    fn run_find(&mut self, pick: Pick, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let query = find.query.clone();
        let generation = find.generation;
        let finder = self.session.finder();
        self.find_task = Some(cx.spawn(async move |this, cx| {
            let results = cx
                .background_spawn(async move { finder.find(&query) })
                .await;
            let _ = this.update(cx, |view, cx| {
                let Some(find) = view.find.as_mut() else {
                    return;
                };
                if !find.set_results(generation, results, pick) {
                    return;
                }
                if pick == Pick::Newest {
                    if let Some(found) = find.current_match() {
                        view.session.scroll_to_line(found.start.line);
                    }
                }
                cx.notify();
            });
        }));
    }

    /// New output while the bar is open reruns the search after a short wait.
    fn schedule_find_refresh(&mut self, cx: &mut Context<Self>) {
        let searching = self.find.as_ref().is_some_and(|f| !f.query.text.is_empty());
        if !searching || self.find_refresh_pending {
            return;
        }
        self.find_refresh_pending = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIND_REFRESH).await;
            let _ = this.update(cx, |view, cx| {
                view.find_refresh_pending = false;
                view.run_find(Pick::Keep, cx);
            });
        })
        .detach();
    }

    /// Changes the query through `edit` and searches again.
    fn edit_find(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut FindState)) {
        if let Some(find) = self.find.as_mut() {
            edit(find);
            self.run_find(Pick::Newest, cx);
            cx.notify();
        }
    }

    /// Keys typed into the find bar. They never reach the shell.
    fn find_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform {
            // Cmd+A selects the query. Other Cmd keys are shortcuts (the keymap
            // has already had them).
            if keystroke.key == "a" {
                if let Some(find) = self.find.as_mut() {
                    find.all_selected = !find.query.text.is_empty();
                    cx.notify();
                }
                cx.stop_propagation();
            }
            return;
        }
        cx.stop_propagation();
        if modifiers.control {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.close_find(window, cx),
            "enter" => self.step_find(!modifiers.shift, window, cx),
            "backspace" => self.edit_find(cx, FindState::pop_char),
            "tab" | "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown" => {}
            _ => {
                if let Some(text) = keystroke.key_char.clone() {
                    self.edit_find(cx, |find| find.push_text(&text));
                }
            }
        }
    }

    fn paste_into_find(&mut self, _: &PasteClipboard, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        // A query is one line: a pasted line break ends it.
        let line = text.lines().next().unwrap_or_default().to_string();
        self.edit_find(cx, |find| find.push_text(&line));
    }

    fn render_find_bar(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let chrome = &self.palette.chrome;
        let focused = self.find_focus.is_focused(window);
        let empty = find.query.text.is_empty();

        let input = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                div()
                    .when(empty, |d| d.text_color(chrome.muted).child("Find"))
                    .when(!empty, |d| d.child(find.query.text.clone()))
                    .when(find.all_selected, |d| d.bg(self.palette.selection)),
            )
            .when(focused && !find.all_selected, |d| {
                d.child(div().flex_shrink_0().w(px(1.)).h(px(14.)).bg(chrome.text))
            });

        let toggle = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .flex_shrink_0()
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if on { chrome.accent } else { chrome.muted })
                .when(on, |d| d.bg(chrome.selected))
                .hover(|d| d.bg(chrome.close_hover))
                .child(label)
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .flex_shrink_0()
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(chrome.muted)
                .hover(|d| d.bg(chrome.close_hover).text_color(chrome.text))
                .child(label)
        };

        Some(
            div()
                .id("find-bar")
                .absolute()
                .top(px(6.))
                .right(px(12.))
                .w(px(FIND_BAR_WIDTH))
                .max_w(relative(0.9))
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .text_sm()
                .bg(chrome.overlay)
                .text_color(chrome.text)
                .border_1()
                .border_color(if find.invalid { chrome.error } else { chrome.divider })
                .rounded_md()
                .track_focus(&self.find_focus)
                .key_context("FindBar")
                .on_key_down(cx.listener(Self::find_key))
                .on_action(cx.listener(Self::paste_into_find))
                .on_action(cx.listener(|view, _: &TextDeleteWordBack, _, cx| {
                    view.edit_find(cx, FindState::delete_word)
                }))
                .on_action(cx.listener(|view, _: &TextDeleteToLineStart, _, cx| {
                    view.edit_find(cx, FindState::clear)
                }))
                // The input has no cursor to move, and these keys must not reach the shell.
                .on_action(|_: &TextWordBack, _, _| {})
                .on_action(|_: &TextWordForward, _, _| {})
                .on_action(|_: &TextLineStart, _, _| {})
                .on_action(|_: &TextLineEnd, _, _| {})
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        window.focus(&view.find_focus, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(input)
                .child(toggle("find-regex", ".*", find.query.regex).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.focus(&view.find_focus, cx);
                        view.edit_find(cx, FindState::toggle_regex);
                    }),
                ))
                .child(toggle("find-case", "Aa", find.query.case_sensitive).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.focus(&view.find_focus, cx);
                        view.edit_find(cx, FindState::toggle_case);
                    }),
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w(px(64.))
                        .text_color(if find.invalid { chrome.error } else { chrome.muted })
                        .child(find.counter()),
                )
                .child(button("find-next", "↓").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        view.step_find(true, window, cx);
                    }),
                ))
                .child(button("find-previous", "↑").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        view.step_find(false, window, cx);
                    }),
                ))
                .child(button("find-close", "×").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        view.close_find(window, cx);
                    }),
                ))
                .into_any_element(),
        )
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        cx.emit(TerminalEvent::Activated);

        let metrics = self.metrics.get();
        let (line, column, side) = metrics.cell_at(event.position);
        let mode = self.session.mode();
        let row = self.row_at(line);

        if event.button == MouseButton::Left
            && !event.modifiers.shift
            && self.owns_input()
            && self.place_cursor(line, column)
        {
            cx.notify();
            return;
        }

        if event.button == MouseButton::Left && event.modifiers.secondary() {
            if let Some(Row::Line(buffer_line)) = row {
                if let Some(link) = self.session.url_at(buffer_line, column) {
                    cx.open_url(&link.url);
                    return;
                }
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
            if let Some(Row::Header(id)) = row {
                // A click on a block's header selects it and folds or unfolds it.
                self.selected_block.set(Some(id));
                self.session.toggle_block(id);
            } else {
                self.selected_block.set(None);
            }
            match row {
                Some(Row::Line(buffer_line)) => {
                    if event.modifiers.shift && self.session.selection_text().is_some() {
                        self.session.update_selection(buffer_line, column, side);
                    } else {
                        let kind = match event.click_count {
                            2 => SelectionKind::Semantic,
                            3 => SelectionKind::Lines,
                            _ => SelectionKind::Simple,
                        };
                        self.session.start_selection(kind, buffer_line, column, side);
                    }
                    self.selecting = true;
                }
                _ => {}
            }
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
            if let Some(Row::Line(buffer_line)) = self.row_at(line) {
                self.session.update_selection(buffer_line, column, side);
                cx.notify();
            }
        }

        let hovered = match self.row_at(line) {
            Some(Row::Line(buffer_line)) if event.modifiers.secondary() => self
                .session
                .url_at(buffer_line, column)
                .map(|link| HoveredLink { line, link }),
            _ => None,
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

impl TerminalView {
    /// What the visual row at `line` shows. `None` before the first frame.
    fn row_at(&self, line: usize) -> Option<Row> {
        self.layout.borrow().get(line).copied()
    }

    /// The buffer line at the top of the view. For a header row, the block's prompt line.
    fn top_buffer_line(&self) -> Option<usize> {
        match self.row_at(0)? {
            Row::Line(line) => Some(line),
            Row::Header(id) => self.session.blocks().iter().find(|b| b.id == id)?.prompt_line,
        }
    }

    /// The block the user selected, or else the block at the top of the view.
    fn target_block(&self) -> Option<Block> {
        match self.selected_block.get() {
            Some(id) => self.session.blocks().into_iter().find(|b| b.id == id),
            None => self.block_at_top(),
        }
    }

    /// The block at the top of the view: the last one that starts at or above
    /// it, or the first block when none does.
    fn block_at_top(&self) -> Option<Block> {
        let blocks = self.session.blocks();
        let top = self.top_buffer_line()?;
        blocks
            .iter()
            .rev()
            .find(|b| b.prompt_line.or(b.output_line).is_some_and(|line| line <= top))
            .or(blocks.first())
            .cloned()
    }

    /// Selects the previous or next block and scrolls its header to the top of the
    /// view. From a selected block, the neighbour of it. Without one, previous is the
    /// newest block above the bottom of the view, and next the first one below its
    /// top. Next past the last block clears the selection and shows the newest output.
    fn step_block(&mut self, next: bool, cx: &mut Context<Self>) {
        let blocks = self.session.blocks();
        let selected = self
            .selected_block
            .get()
            .and_then(|id| blocks.iter().find(|b| b.id == id))
            .and_then(|b| b.prompt_line);
        let top = self.session.top_line();
        let target = match (selected, next) {
            (Some(line), true) => self.session.next_block(line),
            (Some(line), false) => self.session.previous_block(line),
            (None, true) => self.session.next_block(top),
            (None, false) => self.session.previous_block(top + self.metrics.get().rows),
        };
        match target.and_then(|block| Some((block.id, block.prompt_line?))) {
            Some((id, line)) => {
                self.selected_block.set(Some(id));
                self.session.scroll_top_to(line);
            }
            None if next => {
                self.selected_block.set(None);
                self.session.scroll_to_bottom();
            }
            None => {}
        }
        cx.notify();
    }

    fn copy_block_command(
        &mut self,
        _: &BlockCopyCommand,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = self
            .target_block()
            .and_then(|block| self.session.block_command(block.id));
        if let Some(command) = command {
            cx.write_to_clipboard(ClipboardItem::new_string(command));
        }
    }

    fn copy_block_output(
        &mut self,
        _: &BlockCopyOutput,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let output = self
            .target_block()
            .and_then(|block| self.session.block_output(block.id));
        if let Some(output) = output {
            cx.write_to_clipboard(ClipboardItem::new_string(output));
        }
    }

    fn toggle_block(
        &mut self,
        _: &BlockToggleCollapse,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(block) = self.target_block() {
            self.session.toggle_block(block.id);
            cx.notify();
        }
    }
}

/// The input editor's key handling, and the calls that feed it.
impl TerminalView {
    /// Whether the input editor takes the keys: the shell is at its prompt.
    fn owns_input(&self) -> bool {
        self.session.input_mode().owns_input()
    }

    /// Keys the input editor takes while the prompt owns the keyboard. Returns
    /// false for keys that `key_down` still handles: printable text (which
    /// arrives through the input handler), keys bound to edit actions, and Cmd
    /// shortcuts other than undo and redo.
    fn prompt_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_ref();

        if modifiers.platform {
            match key {
                "z" if modifiers.shift => self.prompt.buffer.redo(),
                "z" => self.prompt.buffer.undo(),
                _ => return false,
            };
            return Self::consumed(cx);
        }
        if text_edit_for(keystroke).is_some() {
            return false;
        }
        if self.prompt.search.is_some() {
            return self.search_key(event, cx);
        }
        if is_text_input(keystroke.key_char.as_deref(), modifiers.control) {
            return false;
        }

        match key {
            "enter" if modifiers.shift => self.edit_buffer(|buffer| buffer.insert_char('\n')),
            "enter" => self.submit(),
            "backspace" => self.edit_buffer(Buffer::backspace),
            "delete" => self.edit_buffer(Buffer::delete_forward),
            "left" => self.prompt.buffer.move_left(),
            "right" => self.prompt.buffer.move_right(),
            "up" if self.prompt.buffer.is_first_line() => self.recall(true),
            "up" => self.prompt.buffer.move_up(),
            "down" if self.prompt.buffer.is_last_line() => self.recall(false),
            "down" => self.prompt.buffer.move_down(),
            "home" => self.prompt.buffer.line_start(),
            "end" => self.prompt.buffer.line_end(),
            "escape" => self.selected_block.set(None),
            // Shift+PageUp and Shift+PageDown scroll the pane, as they do everywhere.
            "pageup" | "pagedown" if modifiers.shift => return false,
            _ if modifiers.control => self.control_key(key),
            // Other keys (function keys, Tab until completion lands) are not sent to
            // the shell: its line editor is not in use while this one is.
            _ => {}
        }
        Self::consumed(cx)
    }

    /// Ctrl shortcuts of the input editor. Ctrl+D on an empty line still reaches
    /// the shell, so it can end the session. Ctrl+L clears the screen in the shell.
    fn control_key(&mut self, key: &str) {
        match key {
            "a" => self.prompt.buffer.line_start(),
            "e" => self.prompt.buffer.line_end(),
            "b" => self.prompt.buffer.move_left(),
            "f" => self.prompt.buffer.move_right(),
            "h" => self.edit_buffer(Buffer::backspace),
            "u" => self.edit_buffer(Buffer::delete_to_line_start),
            "w" => self.edit_buffer(Buffer::delete_word_back),
            "d" if self.prompt.buffer.is_empty() => self.send_input(b"\x04"),
            "d" => self.edit_buffer(Buffer::delete_forward),
            "c" => self.edit_buffer(Buffer::clear),
            "l" => self.send_input(b"\x0c"),
            "r" => {
                if self.prompt.search.is_none() {
                    self.prompt.search = Some(HistorySearch::new(&self.prompt.history));
                } else if let Some(search) = self.prompt.search.as_mut() {
                    search.older();
                }
            }
            _ => {}
        }
    }

    /// Keys while the Ctrl+R search is open. Typed text goes to the search
    /// through the input handler, so only the keys that drive it are handled here.
    fn search_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if is_text_input(event.keystroke.key_char.as_deref(), modifiers.control) {
            return false;
        }
        match (event.keystroke.key.as_ref(), modifiers.control) {
            ("enter", _) => {
                let found = self
                    .prompt
                    .search
                    .as_ref()
                    .and_then(|search| search.current(&self.prompt.history))
                    .map(str::to_string);
                if let Some(text) = found {
                    self.prompt.buffer.set_text(&text);
                }
                self.prompt.search = None;
            }
            ("escape", _) | ("g", true) | ("c", true) => self.prompt.search = None,
            ("backspace", _) => {
                if let Some(search) = self.prompt.search.as_mut() {
                    search.backspace(&self.prompt.history);
                }
            }
            ("r", true) => {
                if let Some(search) = self.prompt.search.as_mut() {
                    search.older();
                }
            }
            _ => {}
        }
        Self::consumed(cx)
    }

    /// Stops the key from reaching anything else, and redraws.
    fn consumed(cx: &mut Context<Self>) -> bool {
        cx.stop_propagation();
        cx.notify();
        true
    }

    /// Text typed into the editor, or into the search while it is open.
    fn insert_typed(&mut self, text: &str) {
        if let Some(search) = self.prompt.search.as_mut() {
            for c in text.chars() {
                search.type_char(&self.prompt.history, c);
            }
            return;
        }
        self.edit_buffer(|buffer| {
            for c in text.chars() {
                buffer.insert_char(c);
            }
        });
    }

    /// Runs an edit on the editor's buffer. Editing ends a walk through the
    /// history, so the next Up starts again from the edited line.
    fn edit_buffer(&mut self, edit: impl FnOnce(&mut Buffer)) {
        self.prompt.walk.reset();
        edit(&mut self.prompt.buffer);
    }

    /// Up or Down through the history, replacing the line with the entry.
    fn recall(&mut self, older: bool) {
        let entry = if older {
            let current = self.prompt.buffer.text();
            self.prompt.walk.older(&self.prompt.history, &current)
        } else {
            self.prompt.walk.newer(&self.prompt.history)
        };
        if let Some(entry) = entry {
            self.prompt.buffer.set_text(&entry);
        }
    }

    /// Sends the editor's line to the shell to run it. The line joins the history.
    fn submit(&mut self) {
        let bracketed = self.session.mode().contains(TermMode::BRACKETED_PASTE);
        let text = self.prompt.buffer.take();
        self.prompt.walk.reset();
        self.prompt.history.push(&text);
        self.send_input(&submit_bytes(&text, bracketed));
    }

    /// Moves the editor's cursor to a click on the editor. Returns false when the
    /// click is not on the editor, so the pane handles it as before.
    fn place_cursor(&mut self, line: usize, column: usize) -> bool {
        if self.prompt.search.is_some() {
            return false;
        }
        let Some((origin_line, origin_column)) = self.prompt.origin.get() else {
            return false;
        };
        let Some(row) = line.checked_sub(origin_line) else {
            return false;
        };
        let columns = self.metrics.get().columns;
        let chars: Vec<char> = self.prompt.buffer.text().chars().collect();
        let rows = wrap(&chars, columns.saturating_sub(origin_column), columns);
        let Some(range) = rows.get(row) else {
            return false;
        };
        let start = if row == 0 { origin_column } else { 0 };
        let offset = column.saturating_sub(start).min(range.len());
        self.prompt.buffer.set_cursor(range.start + offset);
        true
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
        let find = self
            .find
            .as_ref()
            .filter(|find| !find.query.text.is_empty())
            .map(|find| (find.matches.clone(), find.current));
        let find_bar = self.render_find_bar(window, cx);

        div()
            .id("terminal")
            .relative()
            .size_full()
            .bg(self.palette.background)
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::open_find))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(|view, _: &FindClose, window, cx| view.close_find(window, cx)))
            .on_action(cx.listener(|view, _: &BlockPrevious, _, cx| view.step_block(false, cx)))
            .on_action(cx.listener(|view, _: &BlockNext, _, cx| view.step_block(true, cx)))
            .on_action(cx.listener(Self::copy_block_command))
            .on_action(cx.listener(Self::copy_block_output))
            .on_action(cx.listener(Self::toggle_block))
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
                find,
                cursor_on: self.cursor_on,
                bell: self.bell_on,
                rows: self.layout.clone(),
                selected: self.selected_block.get(),
                editor: self.owns_input().then(|| self.prompt.draw()),
                editor_origin: self.prompt.origin.clone(),
            })
            .children(find_bar)
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
