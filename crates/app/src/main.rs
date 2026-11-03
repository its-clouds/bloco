//! Bloco: a window with tabs, split panes and a command palette. M2 of
//! `docs/roadmap.md`.

mod block_header;
mod colors;
mod commands;
mod completion;
mod find;
mod git;
mod prompt;
mod keys;
mod layout;
mod mouse;
mod option_key;
mod palette;
mod settings;
mod sidebar;
mod split_tree;
mod terminal_element;
mod terminal_view;
mod workspace;
mod zoom;

use gpui::{
    App, AppContext, Bounds, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowOptions, point,
    px, size,
};
use gpui_platform::application;

use commands::{
    ClosePane, CloseTab, CopySelection, FindNext, FindOpen, FindPrevious, ImportTheme, NewTab,
    OpenSettings, PasteClipboard, QuitApp, SplitDown, SplitRight, ToggleSidebar,
};
use workspace::Workspace;

fn main() {
    application().run(|cx: &mut App| {
        cx.on_action(|_: &QuitApp, cx| cx.quit());
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.set_menus(vec![
            Menu {
                name: "Bloco".into(),
                items: vec![
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit Bloco", QuitApp),
                ],
                disabled: false,
            },
            Menu {
                name: "Shell".into(),
                items: vec![
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::action("Close Pane", ClosePane),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("Split Down", SplitDown),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Import Theme…", ImportTheme),
                ],
                disabled: false,
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Copy", CopySelection),
                    MenuItem::action("Paste", PasteClipboard),
                    MenuItem::separator(),
                    MenuItem::action("Find…", FindOpen),
                    MenuItem::action("Find Next", FindNext),
                    MenuItem::action("Find Previous", FindPrevious),
                ],
                disabled: false,
            },
        ]);

        let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // The tab bar is the title bar, so the system one is transparent.
                titlebar: Some(TitlebarOptions {
                    title: Some("Bloco".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(13.))),
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .expect("open window");
        cx.activate(true);
    });
}
