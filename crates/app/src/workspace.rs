//! The window: tabs, panes, the command palette and the config error bar.
//!
//! A tab holds a split tree of panes. Each pane is a `TerminalView` with its own
//! session. Settings are reloaded when `config.toml` or `keymap.toml` change
//! (checked every half second). A file that does not parse keeps the last good
//! settings and shows its error in a bar.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use config::{Files, Settings};
use gpui::{
    Action, AnyElement, App, Bounds, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, Subscription, Window, canvas, div, prelude::*, px, relative,
};
use term_session::{Session, SessionEvent};

use crate::colors::Palette;
use crate::commands::{
    self, ClosePane, CloseTab, FocusDown, FocusLeft, FocusRight, FocusUp, NewTab, NextTab,
    PreviousTab, SelectTab, SplitDown, SplitRight, TogglePalette, ZoomIn, ZoomOut, ZoomReset,
};
use crate::layout::{self, SavedLayout, SavedNode, SavedTab};
use crate::palette::{PaletteState, pretty_keys};
use crate::split_tree::{Axis, DIVIDER, Direction, MAX_RATIO, MIN_RATIO, Node, Rect, SplitTree};
use crate::terminal_view::{TerminalEvent, TerminalStyle, TerminalView};
use crate::zoom;

/// Used when the configured font is not installed.
const FALLBACK_FONT: &str = "Menlo";
const TAB_BAR_HEIGHT: f32 = 30.0;
const PALETTE_WIDTH: f32 = 560.0;
const PALETTE_TOP: f32 = 80.0;
const PALETTE_ROWS: usize = 10;
const SETTINGS_POLL: Duration = Duration::from_millis(500);

/// One tab: its panes and the pane that has the keyboard.
struct Tab {
    panes: SplitTree<Entity<TerminalView>>,
    active: Entity<TerminalView>,
}

/// A divider being dragged: the path to its split in the tree.
#[derive(Clone)]
struct DividerDrag {
    path: Vec<usize>,
}

/// What a settings change installs. Built in full before anything changes, so
/// a bad file leaves the running settings alone.
struct Applied {
    /// Its `font_size` is the configured size. The zoom is added when it is applied.
    style: TerminalStyle,
    base_font_size: f32,
    bindings: Vec<KeyBinding>,
}

pub struct Workspace {
    tabs: Vec<Tab>,
    active_tab: usize,
    style: TerminalStyle,
    /// The font size in `config.toml`. `style.font_size` is this plus `zoom_offset`.
    base_font_size: f32,
    /// Zoom in points from the configured size. In memory only: it is not saved,
    /// and a restart starts at the configured size. See `zoom.rs`.
    zoom_offset: f32,
    /// Each pane's subscriptions, dropped when the pane closes.
    pane_subscriptions: HashMap<EntityId, Subscription>,
    /// The settings files as last read and applied. `None` after a read error.
    seen_files: Option<Files>,
    /// A settings or shell error, shown in a bar until it goes away.
    error: Option<String>,
    palette: Option<PaletteState>,
    palette_focus: FocusHandle,
    divider_drag: Option<DividerDrag>,
    /// The area that holds the panes, measured while painting.
    content_area: Rc<Cell<Bounds<Pixels>>>,
    /// The layout as last written to disk. Only a change is written again.
    last_saved: Option<SavedLayout>,
    /// Writes the layout when the app quits. Dropping it unsubscribes.
    _quit: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let quit = cx.on_app_quit(|workspace, cx| {
            workspace.save_layout(cx);
            std::future::ready(())
        });
        let (applied, seen_files, error) = match config::default_dir().map(|dir| Files::read(&dir)) {
            Some(Ok(files)) => match compile(&files, cx) {
                Ok(applied) => (applied, Some(files), None),
                Err(e) => (defaults(cx), Some(files), Some(e)),
            },
            Some(Err(e)) => (defaults(cx), None, Some(e)),
            None => (defaults(cx), Some(Files::default()), None),
        };

        cx.clear_key_bindings();
        cx.bind_keys(applied.bindings);

        let mut workspace = Self {
            tabs: Vec::new(),
            active_tab: 0,
            style: applied.style,
            base_font_size: applied.base_font_size,
            zoom_offset: 0.0,
            pane_subscriptions: HashMap::new(),
            seen_files,
            error,
            palette: None,
            palette_focus: cx.focus_handle(),
            divider_drag: None,
            content_area: Rc::new(Cell::new(Bounds::default())),
            last_saved: None,
            _quit: quit,
        };
        workspace.style.font_size = workspace.font_size();
        if !workspace.restore_layout(window, cx) {
            match workspace.add_pane(None, window, cx) {
                Ok(pane) => {
                    workspace.tabs.push(Tab {
                        panes: SplitTree::new(pane.clone()),
                        active: pane,
                    });
                    workspace.focus_active(window, cx);
                }
                Err(e) => workspace.error = Some(e),
            }
        }

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SETTINGS_POLL).await;
                if this
                    .update(cx, |workspace, cx| {
                        workspace.poll_settings(cx);
                        workspace.save_layout(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        workspace
    }

    /// Starts a shell in a new pane. Its events come back through `on_pane_event`.
    fn add_pane(
        &mut self,
        cwd: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<TerminalView>, String> {
        // A directory that is gone (removed since the last prompt) falls back to $HOME.
        let cwd = cwd.filter(|dir| dir.is_dir());
        let (tx, rx) = async_channel::unbounded::<SessionEvent>();
        let session = Session::spawn_in(80, 24, cwd, move |event| {
            let _ = tx.try_send(event);
        })
        .map_err(|e| format!("cannot start the shell: {e:#}"))?;
        let style = self.style.clone();
        let pane = cx.new(|cx| TerminalView::new(Arc::new(session), rx, style, cx));
        let subscription = cx.subscribe_in(&pane, window, Self::on_pane_event);
        self.pane_subscriptions.insert(pane.entity_id(), subscription);
        Ok(pane)
    }

    fn on_pane_event(
        &mut self,
        pane: &Entity<TerminalView>,
        event: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::Activated => {
                if let Some(index) = self.tab_index_of(pane) {
                    self.tabs[index].active = pane.clone();
                    self.active_tab = index;
                    cx.notify();
                }
            }
            TerminalEvent::TabTitleChanged => cx.notify(),
            TerminalEvent::Title(title) => {
                if self.active_pane().is_some_and(|active| active == *pane) {
                    window.set_window_title(title);
                }
            }
            TerminalEvent::Exited => self.close_pane(pane, window, cx),
        }
    }

    fn tab_index_of(&self, pane: &Entity<TerminalView>) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.panes.leaves().contains(pane))
    }

    fn active_pane(&self) -> Option<Entity<TerminalView>> {
        self.tabs.get(self.active_tab).map(|tab| tab.active.clone())
    }

    /// The directory of the active pane, for a new tab or split.
    fn active_cwd(&self, cx: &App) -> Option<PathBuf> {
        self.active_pane()?.read(cx).cwd()
    }

    /// Gives the keyboard to the active pane of the active tab.
    fn focus_active(&self, window: &mut Window, cx: &mut App) {
        if let Some(pane) = self.active_pane() {
            let handle = pane.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    /// Removes a pane. Its tab closes with its last pane, and the window closes
    /// with its last tab.
    fn close_pane(&mut self, pane: &Entity<TerminalView>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index_of(pane) else {
            return;
        };
        self.pane_subscriptions.remove(&pane.entity_id());
        let Tab { panes, active } = self.tabs.remove(index);
        let before = panes.leaves();
        let position = before.iter().position(|p| p == pane).unwrap_or(0);
        match panes.remove(pane) {
            Some(panes) => {
                let after = panes.leaves();
                // The pane that moves into the closed pane's place takes the focus.
                let active = if active == *pane {
                    after[position.min(after.len() - 1)].clone()
                } else {
                    active
                };
                self.tabs.insert(index, Tab { panes, active });
            }
            None if self.active_tab > index => self.active_tab -= 1,
            None => {}
        }
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Closes a tab and all its panes. Its shells are killed when the views drop.
    /// The palette's "Close tab" and the tab's × and middle-click all come here.
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = remove_tab(&mut self.tabs, &mut self.active_tab, index) else {
            return;
        };
        for pane in tab.panes.leaves() {
            self.pane_subscriptions.remove(&pane.entity_id());
        }
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.active_pane() else {
            return;
        };
        let cwd = target.read(cx).cwd();
        match self.add_pane(cwd.as_deref(), window, cx) {
            Ok(pane) => {
                let tab = &mut self.tabs[self.active_tab];
                if tab.panes.split(&target, axis, pane.clone()) {
                    tab.active = pane;
                    self.focus_active(window, cx);
                }
            }
            Err(e) => self.set_error(Some(e), cx),
        }
        cx.notify();
    }

    fn focus_direction(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let area = rect_of(self.content_area.get());
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if let Some(pane) = tab.panes.neighbor(&tab.active, direction, area) {
            tab.active = pane;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn drag_divider(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.divider_drag.clone() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.divider_drag = None;
            return;
        }
        let area = rect_of(self.content_area.get());
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        let Some((axis, rect)) = tab.panes.split_at(&drag.path, area) else {
            return;
        };
        let (position, size) = match axis {
            Axis::Horizontal => (f32::from(event.position.x) - rect.x, rect.width),
            Axis::Vertical => (f32::from(event.position.y) - rect.y, rect.height),
        };
        if size > 0.0 && tab.panes.set_ratio(&drag.path, position / size) {
            cx.notify();
        }
    }

    fn toggle_palette(&mut self, _: &TogglePalette, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        } else {
            let shortcuts = commands::COMMANDS
                .iter()
                .map(|command| {
                    commands::action_for(command.id)
                        .map(|action| shortcut_text(window, action.as_ref()))
                        .unwrap_or_default()
                })
                .collect();
            self.palette = Some(PaletteState::new(shortcuts));
            window.focus(&self.palette_focus, cx);
        }
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Runs the highlighted command. The palette closes first, so the action
    /// reaches the pane that had the keyboard.
    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let command = self.palette.as_ref().and_then(PaletteState::selected_command);
        self.close_palette(window, cx);
        if let Some(action) = command.and_then(|command| commands::action_for(command.id)) {
            window.dispatch_action(action, cx);
        }
    }

    fn palette_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        // Shortcuts (Cmd+Shift+P closes the palette) still reach the keymap.
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.close_palette(window, cx),
            "enter" => self.run_selected(window, cx),
            key => {
                if let Some(palette) = self.palette.as_mut() {
                    match key {
                        "up" => palette.move_selection(-1),
                        "down" => palette.move_selection(1),
                        "backspace" => palette.pop_char(),
                        _ => {
                            if let Some(text) = &keystroke.key_char {
                                palette.push_text(text);
                            }
                        }
                    }
                }
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    /// Checks the settings files and applies them if they changed.
    fn poll_settings(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = config::default_dir() else {
            return;
        };
        let files = match Files::read(&dir) {
            Ok(files) => files,
            Err(e) => {
                self.seen_files = None;
                self.set_error(Some(e), cx);
                return;
            }
        };
        if self.seen_files.as_ref() == Some(&files) {
            return;
        }
        self.seen_files = Some(files.clone());
        match compile(&files, cx) {
            Ok(applied) => {
                eprintln!("bloco: settings reloaded from {}", dir.display());
                self.set_error(None, cx);
                self.apply(applied, cx);
            }
            Err(e) => self.set_error(Some(e), cx),
        }
    }

    /// Writes the layout if it changed since the last write. Called on a timer
    /// and on quit. With no tabs open (the last one just closed) nothing is
    /// written, so the next launch starts fresh rather than with no tabs.
    fn save_layout(&mut self, cx: &App) {
        if self.tabs.is_empty() {
            return;
        }
        let layout = self.saved_layout(cx);
        if self.last_saved.as_ref() == Some(&layout) {
            return;
        }
        let Some(dir) = config::default_dir() else {
            return;
        };
        match layout::save(&dir, &layout) {
            Ok(()) => self.last_saved = Some(layout),
            Err(e) => eprintln!("bloco: cannot save the layout: {e}"),
        }
    }

    fn saved_layout(&self, cx: &App) -> SavedLayout {
        SavedLayout {
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    root: save_node(tab.panes.root(), cx),
                    active: tab
                        .panes
                        .leaves()
                        .iter()
                        .position(|pane| *pane == tab.active)
                        .unwrap_or(0),
                })
                .collect(),
            active_tab: self.active_tab,
        }
    }

    /// Opens the tabs from the saved layout, with a shell in each pane. Returns
    /// `false` when there is no usable layout, and the caller opens one tab.
    fn restore_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(saved) = config::default_dir().and_then(|dir| layout::load(&dir)) else {
            return false;
        };
        if saved.tabs.is_empty() {
            return false;
        }
        let mut tabs = Vec::new();
        for saved_tab in &saved.tabs {
            match self.restore_node(&saved_tab.root, window, cx) {
                Ok(root) => {
                    let panes = SplitTree::from_root(root);
                    let leaves = panes.leaves();
                    let active = leaves[saved_tab.active.min(leaves.len() - 1)].clone();
                    tabs.push(Tab { panes, active });
                }
                Err(e) => {
                    // Nothing is kept from a partial restore. Dropping the tabs drops their shells.
                    eprintln!("bloco: cannot restore the layout: {e}");
                    self.pane_subscriptions.clear();
                    return false;
                }
            }
        }
        self.tabs = tabs;
        self.active_tab = saved.active_tab.min(self.tabs.len() - 1);
        self.focus_active(window, cx);
        true
    }

    fn restore_node(
        &mut self,
        node: &SavedNode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Node<Entity<TerminalView>>, String> {
        match node {
            SavedNode::Pane { cwd } => {
                let pane = self.add_pane(cwd.as_deref(), window, cx)?;
                Ok(Node::Leaf(pane))
            }
            SavedNode::Split {
                axis,
                ratio,
                first,
                second,
            } => Ok(Node::Split {
                axis: *axis,
                ratio: ratio.clamp(MIN_RATIO, MAX_RATIO),
                first: Box::new(self.restore_node(first, window, cx)?),
                second: Box::new(self.restore_node(second, window, cx)?),
            }),
        }
    }

    fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if self.error != error {
            if let Some(error) = &error {
                eprintln!("bloco: {error}");
            }
            self.error = error;
            cx.notify();
        }
    }

    fn apply(&mut self, applied: Applied, cx: &mut Context<Self>) {
        cx.clear_key_bindings();
        cx.bind_keys(applied.bindings);
        self.style = applied.style;
        // The zoom offset is kept, and applied to the new configured size.
        self.base_font_size = applied.base_font_size;
        self.set_font_size(cx);
        cx.notify();
    }

    /// The terminal font size: the configured size plus the zoom, clamped.
    fn font_size(&self) -> Pixels {
        px(zoom::effective_size(self.base_font_size, self.zoom_offset))
    }

    /// Gives the current font size to every pane. Each pane re-measures its
    /// cells and resizes its grid and PTY, in tabs that are hidden too.
    fn set_font_size(&mut self, cx: &mut Context<Self>) {
        self.style.font_size = self.font_size();
        for tab in &self.tabs {
            for pane in tab.panes.leaves() {
                let style = self.style.clone();
                pane.update(cx, |view, cx| view.apply_style(style, cx));
            }
        }
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_offset = zoom::stepped_offset(self.base_font_size, self.zoom_offset, 1.0);
        self.set_font_size(cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_offset = zoom::stepped_offset(self.base_font_size, self.zoom_offset, -1.0);
        self.set_font_size(cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_offset = 0.0;
        self.set_font_size(cx);
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let active = index == self.active_tab;
            let title = tab.active.read(cx).tab_title();
            div()
                .id(("tab", index))
                .flex()
                .items_center()
                .h_full()
                .pl_3()
                .pr_1()
                .min_w(px(120.))
                .max_w(px(240.))
                .overflow_hidden()
                .bg(if active { chrome.tab_active } else { chrome.tab_inactive })
                .text_color(if active { chrome.text } else { chrome.muted })
                .when(active, |d| d.border_b_2().border_color(chrome.accent))
                .child(
                    // The title shrinks and ends in an ellipsis before the close button.
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .child(
                    div()
                        .id(("tab-close", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .ml_1()
                        .size(px(18.))
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|d| d.bg(chrome.close_hover).text_color(chrome.text))
                        .child("×")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                                // The click must not reach the tab, or it would select the tab first.
                                cx.stop_propagation();
                                ws.close_tab(index, window, cx);
                            }),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                        ws.select_tab(index, window, cx)
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                        ws.close_tab(index, window, cx)
                    }),
                )
        });
        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .h(px(TAB_BAR_HEIGHT))
            .bg(chrome.tab_bar)
            .children(tabs)
            .child(
                div()
                    .id("new-tab")
                    .flex()
                    .items_center()
                    .px_3()
                    .text_color(chrome.muted)
                    .child("+")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                            ws.new_tab(&NewTab, window, cx)
                        }),
                    ),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let area = self.content_area.clone();
        let mut content = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .child(
                canvas(
                    move |bounds, _, _| area.set(bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if let Some(tab) = self.tabs.get(self.active_tab) {
            content = content.child(self.render_node(tab.panes.root(), Vec::new(), cx));
        }
        content
    }

    /// Renders one node of the split tree with flexbox. The sizes match
    /// `split_tree::split_rect`: the first child takes `ratio` of the space,
    /// then the divider, then the second child.
    fn render_node(
        &self,
        node: &Node<Entity<TerminalView>>,
        path: Vec<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Leaf(pane) => div()
                .size_full()
                .min_w_0()
                .min_h_0()
                .child(pane.clone())
                .into_any_element(),
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let horizontal = *axis == Axis::Horizontal;
                let ratio = *ratio;
                let mut first_path = path.clone();
                first_path.push(0);
                let mut second_path = path.clone();
                second_path.push(1);
                let drag = DividerDrag { path };

                let first_el = div()
                    .flex_shrink_0()
                    .when(horizontal, |d| d.w(relative(ratio)).h_full())
                    .when(!horizontal, |d| d.h(relative(ratio)).w_full())
                    .child(self.render_node(first, first_path, cx));
                let divider = div()
                    .flex_shrink_0()
                    .bg(self.style.palette.chrome.divider)
                    .when(horizontal, |d| d.w(px(DIVIDER)).h_full().cursor_ew_resize())
                    .when(!horizontal, |d| d.h(px(DIVIDER)).w_full().cursor_ns_resize())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _: &MouseDownEvent, _, _| {
                            ws.divider_drag = Some(drag.clone());
                        }),
                    );
                let second_el = div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.render_node(second, second_path, cx));
                div()
                    .flex()
                    .size_full()
                    .when(!horizontal, |d| d.flex_col())
                    .child(first_el)
                    .child(divider)
                    .child(second_el)
                    .into_any_element()
            }
        }
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = &self.palette else {
            return div().into_any_element();
        };
        let chrome = &self.style.palette.chrome;
        let rows = state
            .matches
            .iter()
            .take(PALETTE_ROWS)
            .enumerate()
            .map(|(row, &index)| {
                let command = &commands::COMMANDS[index];
                div()
                    .id(("palette-row", row))
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .when(row == state.selected, |d| d.bg(chrome.selected))
                    .child(command.title)
                    .child(
                        div()
                            .text_color(chrome.muted)
                            .child(state.shortcuts[index].clone()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                            if let Some(palette) = ws.palette.as_mut() {
                                palette.selected = row;
                            }
                            ws.run_selected(window, cx);
                        }),
                    )
            });
        // The overlay covers the window: a click outside the card closes the palette.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .pt(px(PALETTE_TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, _: &MouseDownEvent, window, cx| ws.close_palette(window, cx)),
            )
            .child(
                div()
                    .id("palette")
                    .track_focus(&self.palette_focus)
                    .on_key_down(cx.listener(Self::palette_key))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(PALETTE_WIDTH))
                    .p_2()
                    .bg(chrome.overlay)
                    .border_1()
                    .border_color(chrome.divider)
                    .rounded_md()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .child(format!("> {}", state.query)),
                    )
                    .children(rows),
            )
            .into_any_element()
    }

    // Action handlers. The keymap and the palette both call these.

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
        match self.add_pane(cwd.as_deref(), window, cx) {
            Ok(pane) => {
                self.tabs.push(Tab {
                    panes: SplitTree::new(pane.clone()),
                    active: pane,
                });
                self.active_tab = self.tabs.len() - 1;
                self.focus_active(window, cx);
            }
            Err(e) => self.set_error(Some(e), cx),
        }
        cx.notify();
    }

    fn close_tab_action(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active_tab, window, cx);
    }

    fn close_pane_action(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_pane() {
            self.close_pane(&pane, window, cx);
        }
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            let next = (self.active_tab + 1) % self.tabs.len();
            self.select_tab(next, window, cx);
        }
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            let previous = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
            self.select_tab(previous, window, cx);
        }
    }

    fn select_tab_action(&mut self, action: &SelectTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab(action.0, window, cx);
    }

    fn split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        self.split(Axis::Horizontal, window, cx);
    }

    fn split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.split(Axis::Vertical, window, cx);
    }

    fn focus_left(&mut self, _: &FocusLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(Direction::Left, window, cx);
    }

    fn focus_right(&mut self, _: &FocusRight, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(Direction::Right, window, cx);
    }

    fn focus_up(&mut self, _: &FocusUp, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(Direction::Up, window, cx);
    }

    fn focus_down(&mut self, _: &FocusDown, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(Direction::Down, window, cx);
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = &self.style.palette;
        let mut root = div()
            .id("workspace")
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.chrome.text)
            .on_mouse_move(cx.listener(Self::drag_divider))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|ws, _: &MouseUpEvent, _, _| ws.divider_drag = None),
            )
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab_action))
            .on_action(cx.listener(Self::close_pane_action))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::select_tab_action))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::focus_left))
            .on_action(cx.listener(Self::focus_right))
            .on_action(cx.listener(Self::focus_up))
            .on_action(cx.listener(Self::focus_down))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .when(self.tabs.len() > 1, |root| root.child(self.render_tab_bar(cx)))
            .child(self.render_content(cx));
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .bg(palette.chrome.error)
                    .text_color(palette.background)
                    .child(error.clone()),
            );
        }
        if self.palette.is_some() {
            root = root.child(self.render_palette(cx));
        }
        root
    }
}

/// Parses and checks the settings. Nothing is applied here.
fn compile(files: &Files, cx: &App) -> Result<Applied, String> {
    let settings = Settings::parse(files, &commands::default_keymap())?;
    let bindings = commands::key_bindings(&settings.keymap, cx)?;
    let font_family = resolve_font(&settings.config.font.family, cx);
    Ok(Applied {
        style: TerminalStyle {
            palette: Rc::new(Palette::from_theme(&settings.theme)),
            font_family: font_family.into(),
            font_size: px(settings.config.font.size),
            option_as_meta: settings.config.option_as_meta,
        },
        base_font_size: settings.config.font.size,
        bindings,
    })
}

/// The built-in settings, for when the files cannot be used.
fn defaults(cx: &App) -> Applied {
    compile(&Files::default(), cx).expect("the built-in settings are valid")
}

fn resolve_font(family: &str, cx: &App) -> String {
    if cx.text_system().all_font_names().iter().any(|name| name == family) {
        family.to_string()
    } else {
        eprintln!("bloco: font {family:?} is not installed, using {FALLBACK_FONT}");
        FALLBACK_FONT.to_string()
    }
}

/// The keys bound to an action, for the palette. Empty when it has none.
fn shortcut_text(window: &Window, action: &dyn Action) -> String {
    if window.bindings_for_action(action).is_empty() {
        String::new()
    } else {
        pretty_keys(&window.keystroke_text_for(action))
    }
}

/// The saved form of a split tree. The directory is read from each pane's shell.
fn save_node(node: &Node<Entity<TerminalView>>, cx: &App) -> SavedNode {
    match node {
        Node::Leaf(pane) => SavedNode::Pane {
            cwd: pane.read(cx).cwd(),
        },
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => SavedNode::Split {
            axis: *axis,
            ratio: *ratio,
            first: Box::new(save_node(first, cx)),
            second: Box::new(save_node(second, cx)),
        },
    }
}

fn rect_of(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

/// Removes the tab at `index` and keeps `active` on the tab it was on. When the
/// active tab is removed, the tab after it takes its place, or the one before it
/// when it was the last. `None` when there is no tab at `index`.
fn remove_tab<T>(tabs: &mut Vec<T>, active: &mut usize, index: usize) -> Option<T> {
    if index >= tabs.len() {
        return None;
    }
    let tab = tabs.remove(index);
    if index < *active {
        *active -= 1;
    }
    *active = (*active).min(tabs.len().saturating_sub(1));
    Some(tab)
}

#[cfg(test)]
mod tests {
    use super::remove_tab;

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_tab() {
        let mut tabs = vec!["a", "b", "c"];
        let mut active = 2;
        assert_eq!(remove_tab(&mut tabs, &mut active, 0), Some("a"));
        assert_eq!(tabs, ["b", "c"]);
        assert_eq!(active, 1);

        let mut tabs = vec!["a", "b", "c"];
        let mut active = 0;
        assert_eq!(remove_tab(&mut tabs, &mut active, 2), Some("c"));
        assert_eq!(tabs, ["a", "b"]);
        assert_eq!(active, 0);
    }

    #[test]
    fn closing_the_active_tab_selects_the_next_one() {
        let mut tabs = vec!["a", "b", "c"];
        let mut active = 1;
        assert_eq!(remove_tab(&mut tabs, &mut active, 1), Some("b"));
        assert_eq!(tabs, ["a", "c"]);
        assert_eq!(active, 1);
    }

    #[test]
    fn closing_the_last_tab_selects_the_previous_one() {
        let mut tabs = vec!["a", "b", "c"];
        let mut active = 2;
        assert_eq!(remove_tab(&mut tabs, &mut active, 2), Some("c"));
        assert_eq!(tabs, ["a", "b"]);
        assert_eq!(active, 1);
    }

    #[test]
    fn closing_the_only_tab_empties_the_list() {
        let mut tabs = vec!["a"];
        let mut active = 0;
        assert_eq!(remove_tab(&mut tabs, &mut active, 0), Some("a"));
        assert!(tabs.is_empty());
    }

    #[test]
    fn an_index_past_the_end_changes_nothing() {
        let mut tabs = vec!["a", "b"];
        let mut active = 1;
        assert_eq!(remove_tab(&mut tabs, &mut active, 2), None);
        assert_eq!(tabs, ["a", "b"]);
        assert_eq!(active, 1);
    }
}
