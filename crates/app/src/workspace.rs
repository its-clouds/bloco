//! The window: tabs, panes, the sidebar, the command palette, the settings
//! panel and the config error bar.
//!
//! A tab holds a split tree of panes. Each pane is a `TerminalView` with its own
//! session. Settings are reloaded when `config.toml`, `keymap.toml` or a theme
//! file change (checked every half second). A file that does not parse keeps
//! the last good settings and shows its error in a bar.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use config::{Config, Files, Settings};
use gpui::{
    Action, AnyElement, App, Bounds, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathPromptOptions, Pixels, Render, Subscription, Window, WindowBackgroundAppearance, canvas, div, prelude::*, px, relative,
};
use term_session::{Color, NamedColor, Session, SessionEvent};

use crate::block_header::display_dir;
use crate::colors::{Chrome, Palette};
use crate::commands::{
    self, ClosePane, CloseTab, FocusDown, FocusLeft, FocusRight, FocusUp, ImportTheme, NewTab,
    NextTab, OpenSettings, PreviousTab, SelectTab, ShowFiles, ShowSessions, SplitDown, SplitRight,
    TogglePalette, ToggleSidebar, ZoomIn, ZoomOut, ZoomReset,
};
use crate::layout::{self, SavedLayout, SavedNode, SavedTab};
use crate::palette::{PaletteState, pretty_keys};
use crate::settings::{self as settings_rows, Setting};
use crate::sidebar::{self, Mode, SIDEBAR_WIDTH, Sidebar};
use crate::split_tree::{Axis, DIVIDER, Direction, MAX_RATIO, MIN_RATIO, Node, Rect, SplitTree};
use crate::terminal_view::{TerminalEvent, TerminalStyle, TerminalView};
use crate::zoom;

/// Used when the configured font is not installed.
const FALLBACK_FONT: &str = "Menlo";
/// The title bar holds the tabs. It is always shown.
const TAB_BAR_HEIGHT: f32 = 38.0;
const TAB_HEIGHT: f32 = 26.0;
/// Room on the left of the title bar for the macOS traffic lights.
const TRAFFIC_LIGHT_PADDING: f32 = 90.0;
/// The grab area around a divider. Wider than the 1px line it marks.
const DIVIDER_GRAB: f32 = 7.0;
/// The grab area over the sidebar's right edge. Same size as a divider's.
const SIDEBAR_GRAB: f32 = 7.0;
const PALETTE_WIDTH: f32 = 600.0;
const PALETTE_TOP: f32 = 72.0;
const PALETTE_ROWS: usize = 10;
const SETTINGS_POLL: Duration = Duration::from_millis(500);
const SETTINGS_TOP: f32 = 60.0;
const SETTINGS_WIDTH: f32 = 480.0;

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
    config: Config,
    /// The names `theme` can take: the built-ins, then the theme files.
    theme_names: Vec<String>,
    appearance: WindowBackgroundAppearance,
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
    /// The config as last applied. The settings panel shows its values.
    config: Config,
    /// The theme names the settings panel cycles through.
    theme_names: Vec<String>,
    /// The window background as the settings want it, and as the window has it.
    appearance: WindowBackgroundAppearance,
    shown_appearance: Option<WindowBackgroundAppearance>,
    sidebar: Sidebar,
    settings_open: bool,
    settings_focus: FocusHandle,
    palette: Option<PaletteState>,
    palette_focus: FocusHandle,
    divider_drag: Option<DividerDrag>,
    /// Whether the sidebar's edge is being dragged.
    sidebar_drag: bool,
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
            config: applied.config,
            theme_names: applied.theme_names,
            appearance: applied.appearance,
            shown_appearance: None,
            sidebar: Sidebar::default(),
            settings_open: false,
            settings_focus: cx.focus_handle(),
            palette: None,
            palette_focus: cx.focus_handle(),
            divider_drag: None,
            sidebar_drag: false,
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
                        workspace.reload_file_tree(cx);
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

    /// Follows the sidebar's edge while it is dragged. The sidebar starts at the
    /// window's left edge, so the width is the mouse's x.
    fn drag_sidebar(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.sidebar_drag {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.sidebar_drag = false;
            return;
        }
        let width = sidebar::clamp_width(f32::from(event.position.x));
        if width != self.sidebar.width {
            self.sidebar.width = width;
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
            sidebar_width: Some(self.sidebar.width),
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
        if let Some(width) = saved.sidebar_width {
            self.sidebar.width = sidebar::clamp_width(width);
        }
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
        self.config = applied.config;
        self.theme_names = applied.theme_names;
        self.appearance = applied.appearance;
        self.set_font_size(cx);
        cx.notify();
    }

    /// Writes `text` to `config.toml`, then reads the file back at once, so the
    /// settings panel and the window show the new value without waiting for the poll.
    fn save_config(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(dir) = config::default_dir() else {
            return;
        };
        let saved = std::fs::create_dir_all(&dir)
            .and_then(|()| config::edit::save(&dir.join("config.toml"), text));
        match saved {
            Ok(()) => self.poll_settings(cx),
            Err(e) => self.set_error(Some(format!("cannot save config.toml: {e}")), cx),
        }
    }

    /// The text of `config.toml` as the files last had it, to change one value in.
    /// `None` while the file has an error: a change then would write over a
    /// mistake the user has not seen yet.
    fn config_text(&mut self, cx: &mut Context<Self>) -> Option<String> {
        match &self.seen_files {
            Some(files) => Some(files.config.clone().unwrap_or_default()),
            None => {
                self.set_error(
                    Some("config.toml has an error. Fix it before changing settings here.".into()),
                    cx,
                );
                None
            }
        }
    }

    /// Changes one setting by one step, from the settings panel's arrows.
    fn change_setting(&mut self, setting: Setting, step: i32, cx: &mut Context<Self>) {
        let Some(text) = self.config_text(cx) else {
            return;
        };
        match settings_rows::step(&text, setting, &self.config, &self.theme_names, step) {
            Ok(text) => self.save_config(&text, cx),
            Err(e) => self.set_error(Some(e), cx),
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn settings_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close_settings(window, cx);
        }
        cx.stop_propagation();
    }

    /// Asks for a theme file, then copies it into `themes/` and selects it.
    fn import_theme(&mut self, _: &ImportTheme, _: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import theme".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |workspace, cx| workspace.install_theme(&path, cx))
                .ok();
        })
        .detach();
    }

    fn install_theme(&mut self, source: &Path, cx: &mut Context<Self>) {
        let Some(file_name) = source.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
            return;
        };
        let Some(name) = config::import::theme_name(&file_name).map(str::to_string) else {
            self.set_error(
                Some(format!("{file_name} is not a theme file (use .itermcolors or .yaml)")),
                cx,
            );
            return;
        };
        // Bytes, not text: iTerm2 exports a binary property list by default.
        let data = match std::fs::read(source) {
            Ok(data) => data,
            Err(e) => {
                self.set_error(Some(format!("{file_name}: {e}")), cx);
                return;
            }
        };
        // Checked before it is copied, so a broken theme never lands in themes/.
        if let Err(e) = config::import::parse(&file_name, &data) {
            self.set_error(Some(format!("{file_name}: {e}")), cx);
            return;
        }
        let Some(dir) = config::default_dir() else {
            return;
        };
        let themes = dir.join("themes");
        let copied = std::fs::create_dir_all(&themes)
            .and_then(|()| std::fs::write(themes.join(&file_name), &data));
        if let Err(e) = copied {
            self.set_error(Some(format!("cannot copy {file_name}: {e}")), cx);
            return;
        }
        if let Some(current) = self.config_text(cx) {
            match config::edit::set_theme(&current, &name) {
                Ok(text) => self.save_config(&text, cx),
                Err(e) => self.set_error(Some(e), cx),
            }
        }
    }

    /// The window's background: opaque, or see-through with or without blur.
    fn appearance_for(window: &config::WindowConfig) -> WindowBackgroundAppearance {
        match (window.background_opacity < 1.0, window.background_blur) {
            (false, _) => WindowBackgroundAppearance::Opaque,
            (true, false) => WindowBackgroundAppearance::Transparent,
            (true, true) => WindowBackgroundAppearance::Blurred,
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.open = !self.sidebar.open;
        cx.notify();
    }

    fn show_sessions(&mut self, _: &ShowSessions, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.open = true;
        self.sidebar.mode = Mode::Sessions;
        cx.notify();
    }

    fn show_files(&mut self, _: &ShowFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.open = true;
        self.sidebar.mode = Mode::Files;
        cx.notify();
    }

    /// Keeps the file tree on the active pane's directory. Called while the
    /// file tree is drawn, so a `cd` or a new tab shows at once.
    fn sync_file_tree(&mut self, cx: &App) {
        if self.sidebar.open && self.sidebar.mode == Mode::Files {
            let cwd = self.active_cwd(cx);
            self.sidebar.tree.set_root(cwd);
        }
    }

    fn reload_file_tree(&mut self, cx: &mut Context<Self>) {
        if self.sidebar.open && self.sidebar.mode == Mode::Files && self.sidebar.tree.reload() {
            cx.notify();
        }
    }

    /// Types a file's path at the active pane's prompt, quoted for the shell.
    fn insert_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_pane() {
            let text = format!("{} ", sidebar::shell_quote(&path.to_string_lossy()));
            pane.update(cx, |view, cx| view.insert_text(&text, cx));
        }
        self.focus_active(window, cx);
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

    /// The title bar: the sidebar toggle, then the tabs and the new-tab button.
    /// With the Sessions list open the tabs are in the sidebar, so the bar shows
    /// the active title instead. Its empty space drags the window, and a double
    /// click zooms it. The tabs and buttons stop their mouse-downs, so clicking
    /// them never drags.
    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let sessions_open = self.sidebar.open && self.sidebar.mode == Mode::Sessions;
        let toggle = self.render_sidebar_toggle(cx);
        let title = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.active.read(cx).tab_title())
            .unwrap_or_default();
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let active = index == self.active_tab;
            let title = tab.active.read(cx).tab_title();
            // Each tab is its own group, so its close button shows only while that tab is hovered.
            let group = format!("tab-{index}");
            div()
                .id(("tab", index))
                .group(group.clone())
                .flex()
                .items_center()
                .h(px(TAB_HEIGHT))
                .px_3()
                .min_w(px(90.))
                .max_w(px(200.))
                .rounded(px(6.))
                .overflow_hidden()
                .text_size(px(13.))
                .bg(if active { chrome.tab_active } else { chrome.tab_bar })
                .text_color(if active { chrome.text } else { chrome.muted })
                .when(!active, |d| {
                    d.hover(|d| d.bg(chrome.tab_hover).text_color(chrome.text))
                })
                .child(
                    // A terminal glyph in the terminal font, marking the tab as a shell.
                    div()
                        .flex_shrink_0()
                        .mr_1p5()
                        .font_family(self.style.font_family.clone())
                        .text_size(px(11.))
                        .text_color(if active { chrome.accent } else { chrome.muted })
                        .child(">_"),
                )
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
                        .size(px(16.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        // Inactive tabs show their close button only when hovered.
                        .text_color(if active {
                            chrome.muted
                        } else {
                            gpui::transparent_black()
                        })
                        .group_hover(group, |style| style.text_color(chrome.muted))
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
                        // Stops here so the title bar does not start a window drag.
                        cx.stop_propagation();
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
            .items_center()
            .flex_shrink_0()
            .h(px(TAB_BAR_HEIGHT))
            .gap_1()
            .px_2()
            .when(cfg!(target_os = "macos"), |d| {
                d.pl(px(TRAFFIC_LIGHT_PADDING))
            })
            .bg(chrome.tab_bar)
            .border_b_1()
            .border_color(chrome.divider)
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(toggle)
            .when(sessions_open, |bar| {
                bar.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_center()
                        .text_size(px(12.))
                        .text_color(chrome.muted)
                        .child(title),
                )
            })
            .when(!sessions_open, |bar| {
                bar.children(tabs).child(
                    div()
                        .id("new-tab")
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_color(chrome.muted)
                        .hover(|d| d.bg(chrome.tab_hover).text_color(chrome.text))
                        .child("+")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                ws.new_tab(&NewTab, window, cx)
                            }),
                        ),
                )
            })
    }

    /// The sidebar toggle in the title bar. Drawn with divs: a small box with a
    /// bar inside its left edge, in the accent colour while the sidebar is open.
    /// Clicking it runs the same code as the toggle-sidebar action.
    fn render_sidebar_toggle(&self, cx: &Context<Self>) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let color = if self.sidebar.open { chrome.accent } else { chrome.muted };
        let icon = div()
            .relative()
            .w(px(14.))
            .h(px(11.))
            .rounded(px(3.))
            .border(px(1.5))
            .border_color(color)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(3.))
                    .w(px(1.5))
                    .bg(color),
            );
        div()
            .id("sidebar-toggle")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .ml_2()
            .mr_0p5()
            .size(px(24.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|d| d.bg(chrome.tab_hover))
            .child(icon)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                    // The click must not reach the title bar, or it would start a window drag.
                    cx.stop_propagation();
                    ws.toggle_sidebar(&ToggleSidebar, window, cx)
                }),
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
                // The hairline is the visible part. The wider transparent strip
                // centred on it is what takes the mouse.
                let grab_offset = -(DIVIDER_GRAB - DIVIDER) / 2.0;
                let grab = div()
                    .absolute()
                    .when(horizontal, |d| {
                        d.top_0()
                            .bottom_0()
                            .left(px(grab_offset))
                            .w(px(DIVIDER_GRAB))
                            .cursor_ew_resize()
                    })
                    .when(!horizontal, |d| {
                        d.left_0()
                            .right_0()
                            .top(px(grab_offset))
                            .h(px(DIVIDER_GRAB))
                            .cursor_ns_resize()
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _: &MouseDownEvent, _, _| {
                            ws.divider_drag = Some(drag.clone());
                        }),
                    );
                let divider = div()
                    .relative()
                    .flex_shrink_0()
                    .bg(self.style.palette.chrome.divider)
                    .when(horizontal, |d| d.w(px(DIVIDER)).h_full())
                    .when(!horizontal, |d| d.h(px(DIVIDER)).w_full())
                    .child(grab);
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
        let mut rows: Vec<AnyElement> = state
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
                    .px_3()
                    .py_1p5()
                    .rounded(px(6.))
                    .text_size(px(13.))
                    .text_color(chrome.text)
                    .when(row == state.selected, |d| d.bg(chrome.selected))
                    .child(command.title)
                    .child(key_chips(&state.shortcuts[index], chrome))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                            if let Some(palette) = ws.palette.as_mut() {
                                palette.selected = row;
                            }
                            ws.run_selected(window, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            rows.push(
                div()
                    .px_3()
                    .py_1p5()
                    .text_size(px(13.))
                    .text_color(chrome.muted)
                    .child("No matching commands")
                    .into_any_element(),
            );
        }
        let query = if state.query.is_empty() {
            // The caret sits in front of the placeholder when nothing is typed.
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(caret(chrome))
                .child(div().text_color(chrome.muted).child("Search commands"))
        } else {
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(state.query.clone())
                .child(caret(chrome))
        };
        // The overlay covers the window with a light dim: a click outside the card closes the palette.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .pt(px(PALETTE_TOP))
            .bg(gpui::black().opacity(0.3))
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
                    .flex()
                    .flex_col()
                    .w(px(PALETTE_WIDTH))
                    .bg(chrome.overlay)
                    .border_1()
                    .border_color(chrome.divider)
                    .rounded(px(10.))
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .text_size(px(14.))
                            .border_b_1()
                            .border_color(chrome.divider)
                            .child(query),
                    )
                    .child(div().p_1p5().children(rows)),
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

impl Workspace {
    /// The sidebar: a switch between the session list and the file tree, then
    /// the list. In Sessions mode the `+` beside the switch opens a tab.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let (text, muted, divider, close_hover) = (
            chrome.text,
            chrome.muted,
            chrome.divider,
            chrome.close_hover,
        );
        let mode = self.sidebar.mode;
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .flex_shrink_0()
            .h(px(28.))
            .mb(px(6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .p(px(2.))
                    .rounded(px(6.))
                    .bg(chrome.surface)
                    .child(self.sidebar_segment("Sessions", Mode::Sessions, mode, cx))
                    .child(self.sidebar_segment("Files", Mode::Files, mode, cx)),
            )
            .when(mode == Mode::Sessions, |header| {
                header.child(
                    div()
                        .id("sidebar-new-tab")
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_color(muted)
                        .hover(|d| d.bg(chrome.tab_hover).text_color(text))
                        .child("+")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                                ws.new_tab(&NewTab, window, cx)
                            }),
                        ),
                )
            });

        let body: Vec<AnyElement> = match mode {
            Mode::Sessions => self
                .tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    let active = index == self.active_tab;
                    let pane = tab.active.read(cx);
                    let title = pane.tab_title();
                    let running = pane.is_running();
                    let branch = pane.branch();
                    // The directory's name, or its full path when that name is the title.
                    let dir = match pane.cwd() {
                        Some(dir) if dir_label(&dir) == title => display_dir(&dir),
                        Some(dir) => dir_label(&dir),
                        None => String::new(),
                    };
                    // Yellow while a command runs, the accent on the active row, muted otherwise.
                    let badge_text = if running {
                        self.style.palette.resolve(Color::Named(NamedColor::Yellow))
                    } else if active {
                        chrome.accent
                    } else {
                        muted
                    };
                    // Each row is its own group, so its close button shows only while that row is hovered.
                    let group = format!("session-{index}");
                    div()
                        .id(("session", index))
                        .group(group.clone())
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .px(px(8.))
                        .py(px(6.))
                        .mb(px(2.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(active, |row| row.bg(chrome.tab_active))
                        .hover(|row| row.bg(chrome.tab_hover))
                        .child(
                            // A terminal glyph in the terminal font, marking the session as a shell.
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .size(px(22.))
                                .rounded(px(5.))
                                .bg(if active { chrome.surface } else { chrome.tab_active })
                                .font_family(self.style.font_family.clone())
                                .text_size(px(9.))
                                .text_color(badge_text)
                                .child(">_"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(12.))
                                        .text_color(text)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_size(px(10.5))
                                        .text_color(muted)
                                        .child(
                                            div()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .child(dir),
                                        )
                                        .when_some(branch, |line, branch| {
                                            line.child(
                                                div()
                                                    .flex_shrink_0()
                                                    .text_color(chrome.success)
                                                    .child(format!(" · {branch}")),
                                            )
                                        })
                                        .when(running, |line| {
                                            line.child(div().flex_shrink_0().child(" · running"))
                                        }),
                                ),
                        )
                        .child(
                            // The shortcut for tabs 1 to 9. On hover the close button takes its place.
                            div()
                                .relative()
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .size(px(16.))
                                .when(index < 9, |slot| {
                                    slot.child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(muted)
                                            .opacity(0.7)
                                            .group_hover(group.clone(), |hint| hint.invisible())
                                            .child(format!("⌘{}", index + 1)),
                                    )
                                })
                                .child(
                                    div()
                                        .id(("session-close", index))
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(16.))
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .text_color(muted)
                                        .invisible()
                                        .group_hover(group, |button| button.visible())
                                        .hover(|button| button.bg(close_hover).text_color(text))
                                        .child("×")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                                                // The click must not reach the row, or it would select the tab first.
                                                cx.stop_propagation();
                                                ws.close_tab(index, window, cx);
                                            }),
                                        ),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                                ws.select_tab(index, window, cx)
                            }),
                        )
                        .into_any_element()
                })
                .collect(),
            Mode::Files => self
                .sidebar
                .tree
                .rows()
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let path = row.path.clone();
                    let is_dir = row.is_dir;
                    let marker = match (is_dir, row.expanded) {
                        (false, _) => "",
                        (true, true) => "▾",
                        (true, false) => "▸",
                    };
                    div()
                        .id(("file", index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .w_full()
                        .h(px(24.))
                        .pl(px(8.0 + 14.0 * row.depth as f32))
                        .pr_2()
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .hover(|line| line.bg(chrome.tab_hover))
                        .child(
                            div()
                                .w(px(12.))
                                .flex_shrink_0()
                                .text_size(px(9.))
                                .text_color(muted)
                                .opacity(0.7)
                                .child(marker),
                        )
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(if is_dir { text } else { muted })
                                .child(row.name.clone()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                                if is_dir {
                                    ws.sidebar.tree.toggle(&path);
                                    cx.notify();
                                } else {
                                    ws.insert_path(&path, window, cx);
                                }
                            }),
                        )
                        .into_any_element()
                })
                .collect(),
        };
        // The root of the file tree, above its rows.
        let root_label = match mode {
            Mode::Files => self.sidebar.tree.root().map(display_dir),
            Mode::Sessions => None,
        };
        let empty_note = match (mode, self.sidebar.tree.root()) {
            (Mode::Files, None) => Some("No folder for this pane"),
            _ => None,
        };

        let panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .child(header)
            .child(
                div()
                    .id("sidebar-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_1()
                    .when_some(root_label, |body, label| {
                        body.child(
                            div()
                                .px(px(8.))
                                .pb(px(6.))
                                .text_size(px(11.))
                                .text_color(muted)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(label),
                        )
                    })
                    .children(body)
                    .when_some(empty_note, |body, note| {
                        body.child(
                            div()
                                .px(px(8.))
                                .py_2()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(note),
                        )
                    }),
            );

        // The hairline is the visible part. The wider transparent strip centred
        // on it is what takes the mouse, as with the split dividers. Over the
        // edge it shows an accent line on hover and while dragging.
        let accent = chrome.accent.opacity(0.6);
        let dragging = self.sidebar_drag;
        let handle = div()
            .group("sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(SIDEBAR_GRAB / 2.0 - 0.5))
            .w(px(SIDEBAR_GRAB))
            .cursor_ew_resize()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px((SIDEBAR_GRAB - 2.0) / 2.0))
                    .w(px(2.))
                    .when(dragging, |d| d.bg(accent))
                    .group_hover("sidebar-resize", |s| s.bg(accent)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    // A double click puts the width back and does not start a drag.
                    if event.click_count >= 2 {
                        ws.sidebar.width = SIDEBAR_WIDTH;
                    } else {
                        ws.sidebar_drag = true;
                    }
                    cx.notify();
                }),
            );
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(self.sidebar.width))
            .h_full()
            .min_h_0()
            .px(px(6.))
            .pt(px(8.))
            .bg(chrome.tab_bar)
            .border_r_1()
            .border_color(divider)
            .child(panel)
            .child(handle)
    }

    /// One segment of the sidebar's switch. The active one is raised.
    fn sidebar_segment(
        &self,
        label: &'static str,
        target: Mode,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let active = target == mode;
        div()
            .id(label)
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .py(px(3.))
            .rounded(px(4.))
            .text_size(px(11.))
            .cursor_pointer()
            .text_color(if active { chrome.text } else { chrome.muted })
            .when(active, |segment| segment.bg(chrome.tab_active))
            .when(!active, |segment| {
                segment.hover(|segment| segment.text_color(chrome.text))
            })
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                    ws.sidebar.mode = target;
                    cx.notify();
                }),
            )
    }

    /// The settings panel: one row per setting, each with arrows that step it,
    /// and a row that imports a theme file. Escape closes it.
    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        let (text, muted, divider, close_hover, accent, overlay) = (
            chrome.text,
            chrome.muted,
            chrome.divider,
            chrome.close_hover,
            chrome.accent,
            chrome.overlay,
        );
        let arrow = |index: usize, label: &'static str, setting: Setting, step: i32, cx: &mut Context<Self>| {
            div()
                .id(("setting-step", index * 2 + usize::from(step > 0)))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|button| button.bg(close_hover))
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                        ws.change_setting(setting, step, cx)
                    }),
                )
        };
        let rows = settings_rows::ROWS.iter().enumerate().map(|(index, &setting)| {
            div()
                .id(("setting", index))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(divider)
                .child(setting.label())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(arrow(index, "‹", setting, -1, cx))
                        .child(
                            div()
                                .min_w(px(130.))
                                .flex()
                                .justify_center()
                                .child(setting.value_text(&self.config)),
                        )
                        .child(arrow(index, "›", setting, 1, cx)),
                )
        }).collect::<Vec<_>>();
        let path = config::default_dir()
            .map(|dir| dir.join("config.toml").display().to_string())
            .unwrap_or_default();

        div()
            .id("settings")
            .track_focus(&self.settings_focus)
            .on_key_down(cx.listener(Self::settings_key))
            .absolute()
            .top(px(SETTINGS_TOP))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(SETTINGS_WIDTH))
                    .flex()
                    .flex_col()
                    .bg(overlay)
                    .text_color(text)
                    .border_1()
                    .border_color(divider)
                    .rounded_md()
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(divider)
                            .child("Settings"),
                    )
                    .children(rows)
                    .child(
                        div()
                            .id("import-theme")
                            .px_3()
                            .py_2()
                            .text_color(accent)
                            .cursor_pointer()
                            .hover(|row| row.bg(close_hover))
                            .child("Import theme from a file…")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                                    ws.import_theme(&ImportTheme, window, cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Saved to {path}. Esc closes.")),
                    ),
            )
            .into_any_element()
    }
}

/// A directory as a short label: its last name, or the whole path for `/`.
fn dir_label(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.shown_appearance != Some(self.appearance) {
            window.set_background_appearance(self.appearance);
            self.shown_appearance = Some(self.appearance);
        }
        self.sync_file_tree(cx);
        // While the panel is open it keeps the keyboard, so Escape reaches it.
        if self.settings_open && !self.settings_focus.is_focused(window) {
            window.focus(&self.settings_focus, cx);
        }
        let palette = &self.style.palette;
        let mut body = div().flex().flex_row().flex_1().min_h_0().min_w_0();
        if self.sidebar.open {
            body = body.child(self.render_sidebar(cx));
        }
        body = body.child(self.render_content(cx));
        let mut root = div()
            .id("workspace")
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.chrome.text)
            .on_mouse_move(cx.listener(Self::drag_divider))
            .on_mouse_move(cx.listener(Self::drag_sidebar))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|ws, _: &MouseUpEvent, _, _| {
                    ws.divider_drag = None;
                    ws.sidebar_drag = false;
                }),
            )
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::show_sessions))
            .on_action(cx.listener(Self::show_files))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::import_theme))
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
            // The tab bar is the window's title bar, so it is always there.
            .child(self.render_tab_bar(cx))
            .child(body);
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .text_size(px(13.))
                    .bg(palette.chrome.error.opacity(0.12))
                    .border_t_1()
                    .border_color(palette.chrome.error)
                    .text_color(palette.chrome.error)
                    .child(error.clone()),
            );
        }
        if self.palette.is_some() {
            root = root.child(self.render_palette(cx));
        }
        if self.settings_open {
            root = root.child(self.render_settings(cx));
        }
        root
    }
}

/// Parses and checks the settings. Nothing is applied here.
fn compile(files: &Files, cx: &App) -> Result<Applied, String> {
    let settings = Settings::parse(files, &commands::default_keymap())?;
    let bindings = commands::key_bindings(&settings.keymap, cx)?;
    let font_family = resolve_font(&settings.config.font.family, cx);
    let window = &settings.config.window;
    Ok(Applied {
        style: TerminalStyle {
            palette: Rc::new(Palette::from_theme(&settings.theme, window.background_opacity)),
            font_family: font_family.into(),
            font_size: px(settings.config.font.size),
            option_as_meta: settings.config.option_as_meta,
        },
        base_font_size: settings.config.font.size,
        bindings,
        appearance: Workspace::appearance_for(window),
        theme_names: config::theme_names(files),
        config: settings.config.clone(),
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

/// The shortcut as key chips, one per stroke: `⇧⌘P` is one chip, `⌘K ⌘T` two.
fn key_chips(shortcut: &str, chrome: &Chrome) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap_1()
        .children(
            shortcut
                .split(' ')
                .filter(|stroke| !stroke.is_empty())
                .map(|stroke| {
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(chrome.divider)
                        .bg(chrome.surface)
                        .text_size(px(11.))
                        .text_color(chrome.muted)
                        .child(stroke.to_string())
                }),
        )
}

/// The caret of the palette query: a 1px accent bar.
fn caret(chrome: &Chrome) -> impl IntoElement {
    div().w(px(1.)).h(px(16.)).bg(chrome.accent)
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
