use gpui::{Context, IntoElement, Keystroke, ParentElement, Render, Styled, Window, div};
use gpui_component::{ActiveTheme, h_flex, kbd::Kbd};

use super::{group, page};
use crate::{actions::keys, ui::setting_row::SettingRow};

struct Shortcut {
    label: &'static str,
    keys: Vec<&'static str>,
}

struct ShortcutGroup {
    id: &'static str,
    title: &'static str,
    shortcuts: Vec<Shortcut>,
}

fn shortcut(label: &'static str, keys: impl IntoIterator<Item = &'static str>) -> Shortcut {
    Shortcut {
        label,
        keys: keys.into_iter().collect(),
    }
}

/// The bindings from `actions::bind_keys`, grouped for display.
fn shortcut_groups() -> Vec<ShortcutGroup> {
    vec![
        ShortcutGroup {
            id: "shortcuts-requests",
            title: "Requests",
            shortcuts: vec![
                shortcut("New request", [keys::NEW_REQUEST]),
                shortcut("Save", [keys::SAVE]),
                shortcut("Save as", [keys::SAVE_AS]),
                shortcut("Send request", [keys::SEND]),
            ],
        },
        ShortcutGroup {
            id: "shortcuts-navigation",
            title: "Navigation",
            shortcuts: vec![
                shortcut("Back", keys::GO_BACK.iter().copied()),
                shortcut("Forward", keys::GO_FORWARD.iter().copied()),
                shortcut("Command palette", [keys::OPEN_COMMAND_PALETTE]),
                shortcut("Quick open", [keys::QUICK_OPEN]),
                shortcut("Focus URL", [keys::FOCUS_URL]),
            ],
        },
        ShortcutGroup {
            id: "shortcuts-panels",
            title: "Panels",
            shortcuts: vec![
                shortcut("Toggle sidebar", [keys::TOGGLE_SIDEBAR]),
                shortcut("Library", [keys::LIBRARY]),
                shortcut("History", [keys::HISTORY, keys::HISTORY_ALT]),
                shortcut("Tests", [keys::TESTS]),
                shortcut("Send history entry again (in History)", [keys::SEND]),
                shortcut(
                    "Delete history entry",
                    keys::DELETE_HISTORY_ENTRY.iter().copied(),
                ),
            ],
        },
        ShortcutGroup {
            id: "shortcuts-app",
            title: "App",
            shortcuts: app_shortcuts(),
        },
    ]
}

#[cfg(target_os = "macos")]
fn app_shortcuts() -> Vec<Shortcut> {
    vec![
        shortcut("Settings", [keys::SETTINGS]),
        shortcut("Hide", [keys::HIDE]),
        shortcut("Hide others", [keys::HIDE_OTHERS]),
        shortcut("Quit", [keys::QUIT]),
    ]
}

#[cfg(not(target_os = "macos"))]
fn app_shortcuts() -> Vec<Shortcut> {
    vec![
        shortcut("Settings", [keys::SETTINGS]),
        shortcut("Quit", [keys::QUIT]),
    ]
}

/// The keystrokes to show, dropping ones that look the same (on macOS both
/// backspace and delete display as ⌫).
fn keystrokes(keys: &[&'static str]) -> Vec<Keystroke> {
    let mut shown: Vec<String> = Vec::new();
    let mut strokes = Vec::new();
    for key in keys {
        let Ok(stroke) = Keystroke::parse(key) else {
            continue;
        };
        let label = Kbd::format(&stroke);
        if !shown.contains(&label) {
            shown.push(label);
            strokes.push(stroke);
        }
    }
    strokes
}

pub struct ShortcutsPage {
    groups: Vec<ShortcutGroup>,
}

impl ShortcutsPage {
    pub fn new() -> Self {
        Self {
            groups: shortcut_groups(),
        }
    }
}

fn render_shortcut(shortcut: &Shortcut) -> SettingRow {
    SettingRow::new(shortcut.label).control(
        h_flex()
            .gap_1()
            .children(keystrokes(&shortcut.keys).into_iter().map(Kbd::new)),
    )
}

impl Render for ShortcutsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .children(
                self.groups.iter().map(|g| {
                    group(g.id, g.title).children(g.shortcuts.iter().map(render_shortcut))
                }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Changing shortcuts isn't available yet."),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(label: &str) -> Shortcut {
        shortcut_groups()
            .into_iter()
            .flat_map(|g| g.shortcuts)
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("no shortcut {label:?}"))
    }

    fn shown(label: &str) -> Vec<String> {
        keystrokes(&find(label).keys)
            .iter()
            .map(Kbd::format)
            .collect()
    }

    #[test]
    fn groups_are_in_order_and_not_empty() {
        let groups = shortcut_groups();
        let titles: Vec<_> = groups.iter().map(|g| g.title).collect();
        assert_eq!(titles, ["Requests", "Navigation", "Panels", "App"]);
        assert!(groups.iter().all(|g| !g.shortcuts.is_empty()));
    }

    #[test]
    fn every_key_parses() {
        for shortcut in shortcut_groups().iter().flat_map(|g| &g.shortcuts) {
            assert!(!shortcut.keys.is_empty(), "{} has no keys", shortcut.label);
            for key in &shortcut.keys {
                assert!(Keystroke::parse(key).is_ok(), "{key:?} doesn't parse");
            }
        }
    }

    #[test]
    fn lists_every_binding_of_an_action() {
        assert_eq!(find("History").keys, [keys::HISTORY, keys::HISTORY_ALT]);
        assert_eq!(find("Back").keys, keys::GO_BACK);
        assert_eq!(
            find("Delete history entry").keys,
            keys::DELETE_HISTORY_ENTRY
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn formats_with_mac_symbols() {
        assert_eq!(shown("Save as"), ["⇧⌘S"]);
        assert_eq!(shown("Send request"), ["⌘⏎"]);
        assert_eq!(shown("Hide others"), ["⌥⌘H"]);
        assert_eq!(shown("Settings"), ["⌘,"]);
        assert_eq!(shown("Command palette"), ["⌘K"]);
        assert_eq!(shown("Quick open"), ["⌘P"]);
        assert_eq!(shown("History"), ["⌘2", "⌘Y"]);
        assert_eq!(shown("Delete history entry"), ["⌫"]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn formats_with_key_names() {
        assert_eq!(shown("Save as"), ["Ctrl+Shift+S"]);
        assert_eq!(shown("Send request"), ["Ctrl+Enter"]);
        assert_eq!(shown("Back"), ["Ctrl+[", "Alt+Left"]);
        assert_eq!(shown("Command palette"), ["Ctrl+K"]);
        assert_eq!(shown("Quick open"), ["Ctrl+P"]);
        assert_eq!(shown("Delete history entry"), ["Delete"]);
    }
}
