use domain::settings_index::{Setting, SettingsGroup, SettingsSection, Shortcut};
use gpui::{
    App, Context, Entity, IntoElement, Keystroke, ParentElement, Render, Styled, Window, div,
};
use gpui_component::{ActiveTheme, h_flex, kbd::Kbd};

use super::{Reveal, group, page, row};
use crate::actions::keys;

/// The bindings from `actions::bind_keys` for `shortcut`.
fn bindings(shortcut: Shortcut) -> Vec<&'static str> {
    match shortcut {
        Shortcut::NewRequest => vec![keys::NEW_REQUEST],
        Shortcut::Save => vec![keys::SAVE],
        Shortcut::SaveAs => vec![keys::SAVE_AS],
        Shortcut::SendRequest | Shortcut::SendHistoryEntry => vec![keys::SEND],
        Shortcut::FormatJsonBody => vec![keys::FORMAT_JSON_BODY],
        Shortcut::Back => keys::GO_BACK.to_vec(),
        Shortcut::Forward => keys::GO_FORWARD.to_vec(),
        Shortcut::CommandPalette => vec![keys::OPEN_COMMAND_PALETTE],
        Shortcut::QuickOpen => vec![keys::QUICK_OPEN],
        Shortcut::FocusUrl => vec![keys::FOCUS_URL],
        Shortcut::ToggleSidebar => vec![keys::TOGGLE_SIDEBAR],
        Shortcut::Library => vec![keys::LIBRARY],
        Shortcut::History => vec![keys::HISTORY, keys::HISTORY_ALT],
        Shortcut::Tests => vec![keys::TESTS],
        Shortcut::DeleteHistoryEntry => keys::DELETE_HISTORY_ENTRY.to_vec(),
        Shortcut::Settings => vec![keys::SETTINGS],
        Shortcut::Quit => vec![keys::QUIT],
        Shortcut::Hide | Shortcut::HideOthers => platform_keys(shortcut),
    }
}

#[cfg(target_os = "macos")]
fn platform_keys(shortcut: Shortcut) -> Vec<&'static str> {
    match shortcut {
        Shortcut::Hide => vec![keys::HIDE],
        Shortcut::HideOthers => vec![keys::HIDE_OTHERS],
        _ => Vec::new(),
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_keys(_shortcut: Shortcut) -> Vec<&'static str> {
    Vec::new()
}

/// The page's groups with their shortcuts, in page order.
fn shortcut_groups() -> Vec<(SettingsGroup, Vec<Shortcut>)> {
    SettingsGroup::ALL
        .into_iter()
        .filter(|group| group.section() == SettingsSection::Shortcuts)
        .map(|group| {
            let shortcuts = Shortcut::all().filter(|s| s.group() == group).collect();
            (group, shortcuts)
        })
        .collect()
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
    reveal: Entity<Reveal>,
}

impl ShortcutsPage {
    pub fn new(reveal: Entity<Reveal>) -> Self {
        Self { reveal }
    }

    fn render_shortcut(&self, shortcut: Shortcut, cx: &App) -> impl IntoElement {
        row(&self.reveal, Setting::Shortcut(shortcut), cx).control(
            h_flex()
                .gap_1()
                .children(keystrokes(&bindings(shortcut)).into_iter().map(Kbd::new)),
        )
    }
}

impl Render for ShortcutsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .children(shortcut_groups().into_iter().map(|(g, shortcuts)| {
                group(g).children(shortcuts.into_iter().map(|s| self.render_shortcut(s, cx)))
            }))
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
        Shortcut::all()
            .find(|s| s.label() == label)
            .unwrap_or_else(|| panic!("no shortcut {label:?}"))
    }

    fn shown(label: &str) -> Vec<String> {
        keystrokes(&bindings(find(label)))
            .iter()
            .map(Kbd::format)
            .collect()
    }

    #[test]
    fn groups_are_in_order_and_not_empty() {
        let groups = shortcut_groups();
        let titles: Vec<_> = groups.iter().map(|(g, _)| g.title()).collect();
        assert_eq!(titles, ["Requests", "Navigation", "Panels", "App"]);
        assert!(groups.iter().all(|(_, shortcuts)| !shortcuts.is_empty()));
    }

    #[test]
    fn every_key_parses() {
        for shortcut in Shortcut::all() {
            let keys = bindings(shortcut);
            assert!(!keys.is_empty(), "{} has no keys", shortcut.label());
            for key in &keys {
                assert!(Keystroke::parse(key).is_ok(), "{key:?} doesn't parse");
            }
        }
    }

    #[test]
    fn lists_every_binding_of_an_action() {
        assert_eq!(
            bindings(find("History")),
            [keys::HISTORY, keys::HISTORY_ALT]
        );
        assert_eq!(bindings(find("Back")), keys::GO_BACK);
        assert_eq!(
            bindings(find("Delete history entry")),
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
