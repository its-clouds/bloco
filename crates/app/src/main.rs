//! Bloco: a window with tabs, split panes and a command palette. M2 of
//! `docs/roadmap.md`.

mod colors;
mod commands;
mod keys;
mod mouse;
mod option_key;
mod palette;
mod split_tree;
mod terminal_element;
mod terminal_view;
mod workspace;

use gpui::{App, AppContext, Bounds, Menu, MenuItem, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

use commands::{
    ClosePane, CloseTab, CopySelection, NewTab, PasteClipboard, QuitApp, SplitDown, SplitRight,
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
                items: vec![MenuItem::action("Quit Bloco", QuitApp)],
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
                ],
                disabled: false,
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Copy", CopySelection),
                    MenuItem::action("Paste", PasteClipboard),
                ],
                disabled: false,
            },
        ]);

        let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .expect("open window");
        cx.activate(true);
    });
}
