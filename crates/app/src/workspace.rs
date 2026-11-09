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
    Action, AnyElement, App, Bounds, Context, ElementId, Entity, EntityId, FocusHandle, Focusable, IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathPromptOptions, Pixels, Render, Subscription, Window, WindowBackgroundAppearance, canvas, div, prelude::*, px, relative,
};
use term_session::{Color, NamedColor, Session, SessionEvent};

use crate::block_header::display_dir;
use crate::colors::{Chrome, Palette};
use crate::file_viewer::{FileViewer, FileViewerEvent};
use crate::commands::{
    self, ClosePane, ClosePreview, CloseTab, FocusDown, FocusLeft, FocusRight, FocusUp, ImportTheme, MoveTabLeft,
    MoveTabRight, MoveTabToGroup, NewGroup, NewTab, NextTab, OpenSettings, PreviousTab, RenameGroup,
    RenameTab, RenamePane, DeleteGroup, ToggleGroup, SelectTab, ShowFiles, ShowSessions, SplitDown, SplitRight, TogglePalette,
    ToggleSidebar, ZoomIn, ZoomOut, ZoomReset, MovePaneToNewTab, MovePaneToTab,
};
use crate::layout::{self, SavedGroup, SavedLayout, SavedNode, SavedTab};
use crate::palette::{PaletteState, pretty_keys};
use crate::settings::{self as settings_rows, Setting};
use crate::sidebar::{self, Mode, SIDEBAR_WIDTH, Sidebar};
use crate::split_tree::{Axis, DIVIDER, Direction, MAX_RATIO, MIN_RATIO, Node, Rect, SplitTree};
use crate::tab_groups::{self, Group, GroupId, Grouped};
use crate::terminal_view::{TerminalEvent, TerminalStyle, TerminalView};
use crate::zoom;

/// Used when the configured font is not installed.
const FALLBACK_FONT: &str = "Menlo";
/// The title bar holds the tabs. It is always shown.
const TAB_BAR_HEIGHT: f32 = 38.0;
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

/// A file open beside a tab's panes. The subscription is dropped with it.
struct Preview {
    viewer: Entity<FileViewer>,
    _close: Subscription,
}

/// One tab: its panes, the pane that has the keyboard, and the name and group
/// the user gave it.
struct Tab {
    panes: SplitTree<Entity<TerminalView>>,
    active: Entity<TerminalView>,
    /// The name the user gave the tab. `None` shows the automatic title.
    name: Option<String>,
    group: Option<GroupId>,
    /// The file preview panel, when one is open. Not saved in the layout.
    preview: Option<Preview>,
}

impl Tab {
    fn new(panes: SplitTree<Entity<TerminalView>>, active: Entity<TerminalView>) -> Self {
        Self {
            panes,
            active,
            name: None,
            group: None,
            preview: None,
        }
    }

    /// The name the user gave the tab, or else the title of its active pane.
    fn title(&self, cx: &App) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.active.read(cx).tab_title())
    }
}

impl Grouped for Tab {
    fn group(&self) -> Option<GroupId> {
        self.group
    }

    fn set_group(&mut self, group: Option<GroupId>) {
        self.group = group;
    }
}

/// Where a session dragged in the sidebar goes when the button is released:
/// the last place the mouse was over, so a release over a gap still counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionDrop {
    /// Takes this row's place and group.
    Row(usize),
    /// Joins this group, at the end of its run.
    Group(GroupId),
    /// Leaves its group.
    NoGroup,
}

/// A name being typed in a sidebar row, in place of the title.
#[derive(Debug, Clone, PartialEq)]
enum Renaming {
    Tab { index: usize, text: String },
    Group { id: GroupId, text: String },
    /// A session's name, typed in its tab's title. `pane` is the session.
    Pane { pane: EntityId, text: String },
}

/// What a row of a picker does when it is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    /// Puts the active tab in this group, or in no group.
    Group(Option<GroupId>),
    NewGroup,
    /// Moves the active pane into this tab, beside its active pane.
    Tab(usize),
}

/// A list over the workspace, drawn where the palette is: "Move Tab to Group"
/// and "Move Session to Tab". Each row has a label and what it does.
#[derive(Debug)]
struct Picker {
    rows: Vec<(String, Pick)>,
    selected: usize,
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
    /// The name being typed, if a tab is being renamed.
    renaming: Option<Renaming>,
    /// The index of the tab whose row is being dragged in the sidebar, while the button is held.
    session_drag: Option<usize>,
    /// Where that row drops if the button is released now.
    session_drop: Option<SessionDrop>,
    /// The tab groups, in the order they were made.
    groups: Vec<Group>,
    /// The id the next group gets. Ids are not reused.
    next_group_id: GroupId,
    /// Open while choosing a group or a tab for the active tab or pane.
    picker: Option<Picker>,
    rename_focus: FocusHandle,
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
            renaming: None,
            session_drag: None,
            session_drop: None,
            groups: Vec::new(),
            next_group_id: 1,
            picker: None,
            rename_focus: cx.focus_handle(),
            sidebar_drag: false,
            content_area: Rc::new(Cell::new(Bounds::default())),
            last_saved: None,
            _quit: quit,
        };
        workspace.style.font_size = workspace.font_size();
        if !workspace.restore_layout(window, cx) {
            match workspace.add_pane(None, window, cx) {
                Ok(pane) => {
                    workspace.tabs.push(Tab::new(SplitTree::new(pane.clone()), pane));
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
            TerminalEvent::OpenPreview(path) => {
                if let Some(index) = self.tab_index_of(pane) {
                    self.open_preview(index, path.clone(), window, cx);
                }
            }
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

    /// The session with this id, in any tab.
    fn pane_with_id(&self, id: EntityId) -> Option<Entity<TerminalView>> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes.leaves())
            .find(|pane| pane.entity_id() == id)
    }

    /// Starts renaming the active session. The editor opens in the active tab's title.
    fn rename_pane_action(&mut self, _: &RenamePane, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane() else {
            return;
        };
        self.reveal_sessions();
        let text = pane.read(cx).tab_title();
        self.renaming = Some(Renaming::Pane {
            pane: pane.entity_id(),
            text,
        });
        window.focus(&self.rename_focus, cx);
        cx.notify();
    }

    /// Opens the sidebar on the Sessions list, where the rename editors are drawn.
    fn reveal_sessions(&mut self) {
        self.sidebar.open = true;
        self.sidebar.mode = Mode::Sessions;
    }

    /// Starts renaming the active session. The editor opens in its sidebar row.
    fn rename_tab_action(&mut self, _: &RenameTab, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.active_tab;
        if let Some(tab) = self.tabs.get(index) {
            let text = tab.name.clone().unwrap_or_else(|| tab.title(cx));
            self.reveal_sessions();
            self.renaming = Some(Renaming::Tab { index, text });
            window.focus(&self.rename_focus, cx);
            cx.notify();
        }
    }

    /// Moves the active session one place up or down in the Sessions list. Its group
    /// follows the neighbour it passes, as in `tab_groups::move_tab`.
    fn move_active_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let from = self.active_tab;
        let Some(to) = from.checked_add_signed(delta).filter(|&to| to < self.tabs.len()) else {
            return;
        };
        self.reorder_tab(from, to, cx);
    }

    fn move_tab_left(&mut self, _: &MoveTabLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_active_tab(-1, cx);
    }

    fn move_tab_right(&mut self, _: &MoveTabRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_active_tab(1, cx);
    }

    /// Moves the tab at `from` to `to` and keeps the active tab selected.
    fn reorder_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.active_tab = tab_groups::follow(self.active_tab, from, to);
        tab_groups::move_tab(&mut self.tabs, from, to);
        cx.notify();
    }

    /// Ends the rename in progress. `commit` keeps the typed name (blank clears
    /// it). Otherwise the name is left as it was.
    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        match renaming {
            Renaming::Tab { index, text } => {
                if let (true, Some(tab)) = (commit, self.tabs.get_mut(index)) {
                    tab.name = tab_groups::clean_name(&text);
                }
            }
            Renaming::Group { id, text } => {
                // A group keeps its old name when the new one is blank.
                let name = tab_groups::clean_name(&text);
                if let (true, Some(name), Some(group)) =
                    (commit, name, self.groups.iter_mut().find(|group| group.id == id))
                {
                    group.name = name;
                }
            }
            Renaming::Pane { pane, text } => {
                // A blank name clears the session's name, so its automatic title comes back.
                if let (true, Some(pane)) = (commit, self.pane_with_id(pane)) {
                    let name = tab_groups::clean_name(&text);
                    pane.update(cx, |view, cx| view.set_custom_title(name, cx));
                }
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Types into the rename editor. Enter commits, Escape cancels.
    fn rename_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        // Shortcuts still reach the keymap, as in the palette.
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.finish_rename(false, window, cx),
            "enter" => self.finish_rename(true, window, cx),
            key => {
                if let Some(Renaming::Tab { text, .. } | Renaming::Group { text, .. } | Renaming::Pane { text, .. }) =
                    self.renaming.as_mut()
                {
                    match key {
                        "backspace" => {
                            text.pop();
                        }
                        _ => {
                            if let Some(typed) = &keystroke.key_char {
                                text.extend(typed.chars().filter(|c| !c.is_control()));
                            }
                        }
                    }
                }
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    /// Adds a group named after its id. Returns the id.
    fn create_group(&mut self) -> GroupId {
        let id = self.next_group_id;
        self.next_group_id += 1;
        self.groups.push(Group {
            id,
            name: format!("Group {id}"),
            collapsed: false,
        });
        id
    }

    /// Drops the tab at `from` into `group`, or onto the tab at `onto` (taking
    /// its place and group). Follows `tab_groups::move_to_group`, and the
    /// active tab stays selected.
    fn drop_tab(&mut self, from: usize, group: Option<GroupId>, onto: Option<usize>, cx: &mut Context<Self>) {
        if from >= self.tabs.len() {
            return;
        }
        let to = tab_groups::move_to_group(&mut self.tabs, from, group, onto);
        self.active_tab = tab_groups::follow(self.active_tab, from, to);
        cx.notify();
    }

    /// While a sidebar row is dragged, the mouse is over `target`: a release
    /// drops the row there. Does nothing when no row is dragged.
    fn hover_drop(&mut self, target: SessionDrop, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.session_drag.is_none() || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if self.session_drop != Some(target) {
            self.session_drop = Some(target);
            cx.notify();
        }
    }

    /// Ends a sidebar drag: the dragged row goes where the mouse last was.
    fn finish_session_drag(&mut self, cx: &mut Context<Self>) {
        let (Some(from), Some(target)) = (self.session_drag.take(), self.session_drop.take()) else {
            return;
        };
        match target {
            SessionDrop::Row(index) => self.drop_tab(from, None, Some(index), cx),
            SessionDrop::Group(id) => self.drop_tab(from, Some(id), None, cx),
            SessionDrop::NoGroup => self.drop_tab(from, None, None, cx),
        }
    }

    /// Puts the tab at `index` in `group` (`None` for no group). Its place
    /// follows `tab_groups::set_group`, and the tab stays the active one.
    fn set_tab_group(&mut self, index: usize, group: Option<GroupId>, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let to = tab_groups::set_group(&mut self.tabs, index, group);
        self.active_tab = tab_groups::follow(self.active_tab, index, to);
        cx.notify();
    }

    /// The group of the active tab, if it has one.
    fn active_group(&self) -> Option<GroupId> {
        self.tabs.get(self.active_tab)?.group
    }

    fn new_group_action(&mut self, _: &NewGroup, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.create_group();
        self.set_tab_group(self.active_tab, Some(id), cx);
    }

    /// Renames the group of the active tab, in its sidebar header.
    fn rename_group_action(&mut self, _: &RenameGroup, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.active_group() {
            self.start_group_rename(id, window, cx);
        }
    }

    /// Opens the rename editor on group `id`'s name, in its sidebar header.
    fn start_group_rename(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.groups.iter().find(|group| group.id == id).map(|group| group.name.clone()) {
            self.reveal_sessions();
            self.renaming = Some(Renaming::Group { id, text: name });
            window.focus(&self.rename_focus, cx);
            cx.notify();
        }
    }

    /// Hides or shows the sessions of the active tab's group in the sidebar.
    fn toggle_group_action(&mut self, _: &ToggleGroup, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.active_group() {
            self.reveal_sessions();
            self.toggle_group(id, cx);
        }
    }

    /// Hides or shows group `id`'s sessions in the sidebar. Its header stays,
    /// and so does the active session. Next/previous session and Cmd+1…9 skip
    /// the hidden ones.
    fn toggle_group(&mut self, id: GroupId, cx: &mut Context<Self>) {
        if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
            group.collapsed = !group.collapsed;
            cx.notify();
        }
    }

    /// Deletes the group of the active tab. Its tabs stay open, in no group.
    fn delete_group_action(&mut self, _: &DeleteGroup, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.active_group() {
            self.delete_group(id, cx);
        }
    }

    fn delete_group(&mut self, id: GroupId, cx: &mut Context<Self>) {
        if self.group_text(id).is_some() {
            self.renaming = None;
        }
        self.groups.retain(|group| group.id != id);
        tab_groups::remove_group(&mut self.tabs, id);
        cx.notify();
    }

    /// Moves the active pane into tab `target`, beside that tab's active
    /// pane, or into a tab of its own when `target` is `None`. The pane keeps
    /// its shell. A tab it leaves with no panes is closed.
    fn move_active_pane(&mut self, target: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane() else {
            return;
        };
        let Some(source) = self.tab_index_of(&pane) else {
            return;
        };
        match target {
            Some(index) => {
                if index == source || index >= self.tabs.len() {
                    return;
                }
                let tab = &mut self.tabs[index];
                let beside = tab.active.clone();
                if !tab.panes.split(&beside, Axis::Horizontal, pane.clone()) {
                    return;
                }
                tab.active = pane.clone();
                let removed = self.detach_pane(source, &pane);
                // The target moves down one place when the tab before it is closed.
                self.active_tab = if removed && source < index { index - 1 } else { index };
            }
            None => {
                // A tab with one pane has nowhere to send it.
                if self.tabs[source].panes.leaves().len() < 2 {
                    return;
                }
                self.detach_pane(source, &pane);
                self.tabs.push(Tab::new(SplitTree::new(pane.clone()), pane.clone()));
                self.active_tab = self.tabs.len() - 1;
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Takes `pane` out of tab `index`. The tab keeps its other panes, and its
    /// active pane moves to the neighbour that takes the place. Returns `true`
    /// when the tab had no other pane and was removed.
    fn detach_pane(&mut self, index: usize, pane: &Entity<TerminalView>) -> bool {
        let Tab {
            panes,
            active,
            name,
            group,
            preview,
        } = self.tabs.remove(index);
        let position = panes.leaves().iter().position(|p| p == pane).unwrap_or(0);
        match panes.remove(pane) {
            Some(panes) => {
                let after = panes.leaves();
                let active = if active == *pane {
                    after[position.min(after.len() - 1)].clone()
                } else {
                    active
                };
                self.tabs.insert(
                    index,
                    Tab {
                        panes,
                        active,
                        name,
                        group,
                        preview,
                    },
                );
                false
            }
            None => {
                if self.active_tab > index {
                    self.active_tab -= 1;
                }
                true
            }
        }
    }

    fn move_pane_to_new_tab(&mut self, _: &MovePaneToNewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.move_active_pane(None, window, cx);
    }

    fn open_tab_picker(&mut self, _: &MovePaneToTab, window: &mut Window, cx: &mut Context<Self>) {
        let rows: Vec<(String, Pick)> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.active_tab)
            .map(|(index, tab)| (tab.title(cx), Pick::Tab(index)))
            .collect();
        if !rows.is_empty() {
            self.open_picker(rows, window, cx);
        }
    }

    fn open_group_picker(&mut self, _: &MoveTabToGroup, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let mut rows = vec![("No group".to_string(), Pick::Group(None))];
        rows.extend(
            self.groups
                .iter()
                .map(|group| (group.name.clone(), Pick::Group(Some(group.id)))),
        );
        rows.push(("New group".to_string(), Pick::NewGroup));
        self.open_picker(rows, window, cx);
    }

    fn open_picker(&mut self, rows: Vec<(String, Pick)>, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = Some(Picker { rows, selected: 0 });
        window.focus(&self.palette_focus, cx);
        cx.notify();
    }

    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Does what row `row` of the picker says, and closes it.
    fn choose(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pick) = self
            .picker
            .take()
            .and_then(|picker| picker.rows.get(row).map(|(_, pick)| *pick))
        else {
            return;
        };
        match pick {
            Pick::Group(group) => self.set_tab_group(self.active_tab, group, cx),
            Pick::NewGroup => {
                let id = self.create_group();
                self.set_tab_group(self.active_tab, Some(id), cx);
            }
            Pick::Tab(index) => self.move_active_pane(Some(index), window, cx),
        }
        self.focus_active(window, cx);
    }

    fn picker_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.close_picker(window, cx),
            "enter" => {
                let row = self.picker.as_ref().map(|picker| picker.selected);
                if let Some(row) = row {
                    self.choose(row, window, cx);
                }
            }
            "up" | "down" => {
                if let Some(picker) = self.picker.as_mut() {
                    let rows = picker.rows.len();
                    picker.selected = if keystroke.key == "up" {
                        (picker.selected + rows - 1) % rows
                    } else {
                        (picker.selected + 1) % rows
                    };
                }
                cx.notify();
            }
            _ => {}
        }
        cx.stop_propagation();
    }

    /// The picker card, drawn where the palette is.
    fn render_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = &self.picker else {
            return div().into_any_element();
        };
        let chrome = &self.style.palette.chrome;
        let rows = picker.rows.iter().enumerate().map(|(row, (label, _))| {
            div()
                .id(("picker-row", row))
                .px_3()
                .py_1p5()
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(chrome.text)
                .when(row == picker.selected, |d| d.bg(chrome.selected))
                .child(label.clone())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _: &MouseDownEvent, window, cx| ws.choose(row, window, cx)),
                )
        });
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
                cx.listener(|ws, _: &MouseDownEvent, window, cx| ws.close_picker(window, cx)),
            )
            .child(
                div()
                    .id("picker")
                    .track_focus(&self.palette_focus)
                    .on_key_down(cx.listener(Self::picker_key))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .w(px(PALETTE_WIDTH))
                    .p_1p5()
                    .bg(chrome.overlay)
                    .border_1()
                    .border_color(chrome.divider)
                    .rounded(px(10.))
                    .shadow_lg()
                    .children(rows),
            )
            .into_any_element()
    }

    /// Shows `path` in a panel beside the panes of tab `index`, replacing the
    /// file open there. The keyboard stays where it is.
    fn open_preview(&mut self, index: usize, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let style = self.style.clone();
        let viewer = cx.new(|cx| FileViewer::new(path, style, cx));
        let close = cx.subscribe_in(&viewer, window, Self::on_preview_event);
        self.tabs[index].preview = Some(Preview {
            viewer,
            _close: close,
        });
        cx.notify();
    }

    fn on_preview_event(
        &mut self,
        viewer: &Entity<FileViewer>,
        event: &FileViewerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FileViewerEvent::Close => {
                let closed = self
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.preview.as_ref().is_some_and(|p| p.viewer == *viewer))
                    .map(|tab| tab.preview = None)
                    .is_some();
                if closed {
                    self.focus_active(window, cx);
                    cx.notify();
                }
            }
        }
    }

    fn close_preview(&mut self, _: &ClosePreview, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab).filter(|tab| tab.preview.is_some()) {
            tab.preview = None;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// Removes a pane. Its tab closes with its last pane, and the window closes
    /// with its last tab.
    fn close_pane(&mut self, pane: &Entity<TerminalView>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index_of(pane) else {
            return;
        };
        self.pane_subscriptions.remove(&pane.entity_id());
        let Tab {
            panes,
            active,
            name,
            group,
            preview,
        } = self.tabs.remove(index);
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
                self.tabs.insert(
                    index,
                    Tab {
                        panes,
                        active,
                        name,
                        group,
                        preview,
                    },
                );
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
        // Selecting another tab ends a rename in progress, keeping what was typed.
        self.finish_rename(true, window, cx);
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
                    name: tab.name.clone(),
                    group: tab.group,
                })
                .collect(),
            active_tab: self.active_tab,
            sidebar_width: Some(self.sidebar.width),
            groups: self
                .groups
                .iter()
                .map(|group| SavedGroup {
                    id: group.id,
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                })
                .collect(),
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
        let groups: Vec<Group> = saved
            .groups
            .iter()
            .map(|group| Group {
                id: group.id,
                name: group.name.clone(),
                collapsed: group.collapsed,
            })
            .collect();
        let mut tabs = Vec::new();
        for saved_tab in &saved.tabs {
            match self.restore_node(&saved_tab.root, window, cx) {
                Ok(root) => {
                    let panes = SplitTree::from_root(root);
                    let leaves = panes.leaves();
                    let active = leaves[saved_tab.active.min(leaves.len() - 1)].clone();
                    let mut tab = Tab::new(panes, active);
                    tab.name = saved_tab.name.clone();
                    // A group that is not in the saved list is dropped, so no tab points at nothing.
                    tab.group = saved_tab
                        .group
                        .filter(|id| groups.iter().any(|group| group.id == *id));
                    tabs.push(tab);
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
        self.next_group_id = groups.iter().map(|group| group.id).max().unwrap_or(0) + 1;
        self.groups = groups;
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
            SavedNode::Pane { cwd, name } => {
                let pane = self.add_pane(cwd.as_deref(), window, cx)?;
                if name.is_some() {
                    pane.update(cx, |view, cx| view.set_custom_title(name.clone(), cx));
                }
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

    /// The title bar: the sidebar toggle and the active session's title. The
    /// sessions themselves are listed in the sidebar. Its empty space drags the
    /// window, and a double click zooms it.
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = &self.style.palette.chrome;
        let title = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.title(cx))
            .unwrap_or_default();
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
            .child(self.render_sidebar_toggle(cx))
            .child(
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
            // Balances the toggle, so the title sits in the middle of the window.
            .child(div().flex_shrink_0().w(px(30.)))
    }

    /// A group's name. Clicking it starts a rename; while renaming it is the editor.
    fn group_label(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        if let Some(text) = self.group_text(id) {
            return self.rename_editor(("group-rename", id), text, cx);
        }
        let name = self
            .groups
            .iter()
            .find(|group| group.id == id)
            .map(|group| group.name.clone())
            .unwrap_or_default();
        div()
            .id(("group-name", id))
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(chrome.text)
            .child(name)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    ws.start_group_rename(id, window, cx);
                }),
            )
            .into_any_element()
    }

    /// The arrow before a group's name: ▾ while expanded, ▸ while collapsed.
    /// Clicking it hides or shows the group's sessions.
    fn group_chevron(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        let collapsed = self.groups.iter().any(|group| group.id == id && group.collapsed);
        div()
            .id(("group-toggle", id))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(px(14.))
            .rounded(px(4.))
            .cursor_pointer()
            .text_color(chrome.muted)
            .hover(|d| d.bg(chrome.close_hover).text_color(chrome.text))
            .child(if collapsed { "▸" } else { "▾" })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    ws.toggle_group(id, cx);
                }),
            )
            .into_any_element()
    }

    /// How many sessions a collapsed group hides, for its header. `None` while
    /// it is expanded or hides nothing.
    fn hidden_count(&self, id: GroupId) -> Option<usize> {
        let collapsed = self.groups.iter().any(|group| group.id == id && group.collapsed);
        let hidden = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| tab.group == Some(id) && *index != self.active_tab)
            .count();
        (collapsed && hidden > 0).then_some(hidden)
    }

    /// The × that deletes a group. Its tabs stay open, in no group.
    fn group_delete_button(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        div()
            .id(("group-delete", id))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(px(14.))
            .rounded(px(4.))
            .cursor_pointer()
            .text_color(chrome.muted)
            .hover(|d| d.bg(chrome.close_hover).text_color(chrome.text))
            .child("×")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    ws.delete_group(id, cx);
                }),
            )
            .into_any_element()
    }

    /// The name being typed for group `id`, if that group is being renamed.
    fn group_text(&self, id: GroupId) -> Option<String> {
        match &self.renaming {
            Some(Renaming::Group { id: editing, text }) if *editing == id => Some(text.clone()),
            _ => None,
        }
    }

    /// The inline editor that stands in for a name while it is typed. It keeps
    /// the mouse, so a click in it does not select the tab or start a drag.
    fn rename_editor(&self, id: impl Into<ElementId>, text: String, cx: &Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        div()
            .id(id)
            .track_focus(&self.rename_focus)
            .on_key_down(cx.listener(Self::rename_key))
            .flex()
            .flex_1()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(text)
            .child(caret(chrome))
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| cx.stop_propagation())
            .into_any_element()
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
            // A session dragged out of the sidebar over the panes has no drop target, so a release cancels it.
            .on_mouse_move(cx.listener(|ws, _: &MouseMoveEvent, _, cx| {
                if ws.session_drop.take().is_some() {
                    cx.notify();
                }
            }))
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
        // With a file open, the panes take the left half and the file the right.
        let Some(preview) = self.tabs.get(self.active_tab).and_then(|tab| tab.preview.as_ref()) else {
            return content.into_any_element();
        };
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(content)
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(1.))
                    .h_full()
                    .bg(self.style.palette.chrome.divider),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .h_full()
                    .child(preview.viewer.clone()),
            )
            .into_any_element()
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
                    name: None,
                    group: None,
                    preview: None,
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
        // Follow the order the sidebar shows, not the order of `self.tabs`.
        if let Some(next) = tab_groups::step_in_display_order(&self.tabs, &self.groups, self.active_tab, 1) {
            self.select_tab(next, window, cx);
        }
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) =
            tab_groups::step_in_display_order(&self.tabs, &self.groups, self.active_tab, -1)
        {
            self.select_tab(previous, window, cx);
        }
    }

    /// Cmd+N picks the Nth session as the sidebar shows it (hidden ones not
    /// counted), not the Nth in `self.tabs`.
    fn select_tab_action(&mut self, action: &SelectTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&index) = tab_groups::visible_order(&self.tabs, &self.groups, self.active_tab).get(action.0) {
            self.select_tab(index, window, cx);
        }
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
                .child(
                    // New group: takes the active session into a new group and names it.
                    div()
                        .id("sidebar-new-group")
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(muted)
                        .hover(|d| d.bg(chrome.tab_hover).text_color(text))
                        .child("⊞")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                                ws.new_group_action(&NewGroup, window, cx);
                                if let Some(id) = ws.active_group() {
                                    ws.start_group_rename(id, window, cx);
                                }
                            }),
                        ),
                )
            });

        let body: Vec<AnyElement> = match mode {
            Mode::Sessions => {
                // The rows the sidebar shows: sessions of collapsed groups are left out.
                let visible = tab_groups::visible_order(&self.tabs, &self.groups, self.active_tab);
                let rows: Vec<(Option<GroupId>, AnyElement)> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| visible.contains(index))
                    .map(|(index, tab)| {
                        // The Cmd+N this row answers to, counted in sidebar order.
                        let position = visible.iter().position(|&i| i == index).unwrap_or(usize::MAX);
                        let active = index == self.active_tab;
                        let pane = tab.active.read(cx);
                        let title = tab.title(cx);
                        // The name being typed, when this session (or its active pane) is being renamed.
                        let renaming = match &self.renaming {
                            Some(Renaming::Tab { index: editing, text }) if *editing == index => Some(text.clone()),
                            Some(Renaming::Pane { pane, text }) if *pane == tab.active.entity_id() => Some(text.clone()),
                            _ => None,
                        };
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
                        // `Some(true)` draws the drop line below this row, `Some(false)` above it.
                        let drop_line = match (self.session_drag, self.session_drop) {
                            (Some(from), Some(SessionDrop::Row(target))) if target == index && from != index => {
                                Some(index > from)
                            }
                            _ => None,
                        };
                        // Each row is its own group, so its close button shows only while that row is hovered.
                        let group = format!("session-{index}");
                        let row = div()
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
                            .relative()
                            .when(active, |row| row.bg(chrome.tab_active))
                            .hover(|row| row.bg(chrome.tab_hover))
                            // The row a drag would land on shows an accent line on the side it lands:
                            // below it when the dragged row comes from above, above it otherwise.
                            .when_some(drop_line, |row, below| {
                                row.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .h(px(2.))
                                        .rounded(px(1.))
                                        .bg(chrome.accent)
                                        .when(below, |line| line.bottom(px(-2.)))
                                        .when(!below, |line| line.top(px(-2.))),
                                )
                            })
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
                                    .child(match renaming {
                                        Some(name) => div()
                                            .flex()
                                            .text_size(px(12.))
                                            .text_color(text)
                                            .child(self.rename_editor(("session-rename", index), name, cx))
                                            .into_any_element(),
                                        None => div()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_size(px(12.))
                                            .text_color(text)
                                            .child(title)
                                            .into_any_element(),
                                    })
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
                                    .when(position < 9, |slot| {
                                        slot.child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(muted)
                                                .opacity(0.7)
                                                .group_hover(group.clone(), |hint| hint.invisible())
                                                .child(format!("⌘{}", position + 1)),
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
                                cx.listener(move |ws, event: &MouseDownEvent, window, cx| {
                                    ws.session_drag = Some(index);
                                    ws.session_drop = None;
                                    ws.select_tab(index, window, cx);
                                    // A double click renames the session.
                                    if event.click_count == 2 {
                                        ws.session_drag = None;
                                        ws.rename_tab_action(&RenameTab, window, cx);
                                    }
                                }),
                            )
                            // Dragged over, this row becomes the drop target. The drop happens on release, anywhere.
                            .on_mouse_move(cx.listener(move |ws, event: &MouseMoveEvent, _, cx| {
                                ws.hover_drop(SessionDrop::Row(index), event, cx);
                            }))
                            .into_any_element();
                        (tab.group, row)
                    })
                    .collect();
                self.sidebar_sessions(rows, cx)
            }
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
                            cx.listener(move |ws, event: &MouseDownEvent, window, cx| {
                                if is_dir {
                                    ws.sidebar.tree.toggle(&path);
                                    cx.notify();
                                } else if event.modifiers.secondary() {
                                    let index = ws.active_tab;
                                    ws.open_preview(index, path.clone(), window, cx);
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

    /// The Sessions list in sections. Tabs in no group come first, in tab order.
    /// Then each group, with its header and its tabs. Empty groups show their header.
    /// Keep in step with `tab_groups::display_order`, which next/previous session and Cmd+1…9 follow.
    fn sidebar_sessions(
        &self,
        rows: Vec<(Option<GroupId>, AnyElement)>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (ungrouped, mut pending): (Vec<_>, Vec<_>) =
            rows.into_iter().partition(|(group, _)| group.is_none());
        let mut body: Vec<AnyElement> = ungrouped.into_iter().map(|(_, row)| row).collect();
        for group in &self.groups {
            let (mine, rest): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|(in_group, _)| *in_group == Some(group.id));
            pending = rest;
            body.push(self.render_group_header(group.id, cx));
            body.extend(mine.into_iter().map(|(_, row)| row));
        }
        body.extend(pending.into_iter().map(|(_, row)| row));
        body.push(self.render_no_group_zone(cx));
        body
    }

    /// The space below the Sessions list. Dragging a session in a group over
    /// it shows "Drop to leave the group", and a drop there takes it out. So a
    /// session can leave a group even when every session is in one.
    fn render_no_group_zone(&self, cx: &mut Context<Self>) -> AnyElement {
        let chrome = &self.style.palette.chrome;
        let grouped_drag = self
            .session_drag
            .and_then(|from| self.tabs.get(from))
            .is_some_and(|tab| tab.group.is_some());
        let target = self.session_drop == Some(SessionDrop::NoGroup);
        div()
            .id("sidebar-no-group")
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h(px(64.))
            .mt(px(6.))
            .rounded(px(6.))
            .text_size(px(11.))
            .text_color(chrome.muted)
            .when(grouped_drag, |zone| {
                zone.border_1()
                    .border_dashed()
                    .border_color(if target { chrome.accent } else { chrome.divider })
                    .when(target, |zone| zone.bg(chrome.accent.opacity(0.15)).text_color(chrome.text))
                    .child("Drop to leave the group")
            })
            .on_mouse_move(cx.listener(|ws, event: &MouseMoveEvent, _, cx| {
                ws.hover_drop(SessionDrop::NoGroup, event, cx);
            }))
            .into_any_element()
    }

    /// A group's header in the sidebar: its name, and a × that deletes the group.
    fn render_group_header(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let accent = self.style.palette.chrome.accent;
        let target = self.session_drop == Some(SessionDrop::Group(id));
        div()
            .id(("sidebar-group", id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.))
            .mt(px(6.))
            .mb(px(2.))
            .px(px(8.))
            .rounded(px(4.))
            .text_size(px(11.))
            .text_color(self.style.palette.chrome.muted)
            // The header a drag would land on is tinted: a drop joins the group.
            .when(target, |header| header.bg(accent.opacity(0.15)))
            .child(self.group_chevron(id, cx))
            .child(self.group_label(id, cx))
            .when_some(self.hidden_count(id), |header, count| {
                header.child(div().flex_shrink_0().child(count.to_string()))
            })
            .child(self.group_delete_button(id, cx))
            .on_mouse_move(cx.listener(move |ws, event: &MouseMoveEvent, _, cx| {
                ws.hover_drop(SessionDrop::Group(id), event, cx);
            }))
            .into_any_element()
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
        if let Some(viewer) = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| tab.preview.as_ref())
            .map(|preview| preview.viewer.clone())
        {
            let style = self.style.clone();
            viewer.update(cx, |viewer, cx| viewer.set_style(style, cx));
        }
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
                cx.listener(|ws, _: &MouseUpEvent, _, cx| {
                    ws.divider_drag = None;
                    ws.sidebar_drag = false;
                    ws.finish_session_drag(cx);
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
            .on_action(cx.listener(Self::rename_tab_action))
            .on_action(cx.listener(Self::rename_pane_action))
            .on_action(cx.listener(Self::move_tab_left))
            .on_action(cx.listener(Self::move_tab_right))
            .on_action(cx.listener(Self::new_group_action))
            .on_action(cx.listener(Self::rename_group_action))
            .on_action(cx.listener(Self::delete_group_action))
            .on_action(cx.listener(Self::toggle_group_action))
            .on_action(cx.listener(Self::open_group_picker))
            .on_action(cx.listener(Self::move_pane_to_new_tab))
            .on_action(cx.listener(Self::open_tab_picker))
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
            .on_action(cx.listener(Self::close_preview))
            // The title bar is always there. Sessions are in the sidebar.
            .child(self.render_title_bar(cx))
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
        if self.picker.is_some() {
            root = root.child(self.render_picker(cx));
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
            clipboard_write: settings.config.terminal.clipboard_write,
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
            name: pane.read(cx).custom_title().map(str::to_string),
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
