use gpui::{App, Menu, MenuItem, OsAction};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

use crate::actions::{
    FocusUrl, GoBack, GoForward, Hide, HideOthers, NewRequest, OpenHistory, OpenLibrary,
    OpenSettings, QuickOpen, Quit, RevertRequest, SaveRequest, SendRequest, ShowAll, ToggleSidebar,
};

/// Must run after `actions::bind_keys`: the menus read their shortcuts from the keymap.
pub fn init(app_name: &str, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

    cx.set_menus([
        Menu::new(app_name.to_string()).items(app_menu_items(app_name)),
        Menu::new("File").items([
            MenuItem::action("New Request", NewRequest),
            MenuItem::action("Save", SaveRequest),
            MenuItem::action("Revert Changes…", RevertRequest),
            MenuItem::separator(),
            MenuItem::action("Send Request", SendRequest),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::separator(),
            MenuItem::action("Library", OpenLibrary),
            MenuItem::action("History", OpenHistory),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::separator(),
            MenuItem::action("Quick Open…", QuickOpen),
            MenuItem::action("Focus URL", FocusUrl),
        ]),
    ]);
}

#[cfg(target_os = "macos")]
fn app_menu_items(app_name: &str) -> Vec<MenuItem> {
    use gpui::SystemMenuType;

    vec![
        MenuItem::action("Settings…", OpenSettings),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(format!("Hide {app_name}"), Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action(format!("Quit {app_name}"), Quit),
    ]
}

#[cfg(not(target_os = "macos"))]
fn app_menu_items(_app_name: &str) -> Vec<MenuItem> {
    vec![
        MenuItem::action("Settings", OpenSettings),
        MenuItem::separator(),
        MenuItem::action("Quit", Quit),
    ]
}
