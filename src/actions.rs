use gpui::{App, KeyBinding, actions};

actions!(
    posty_pete,
    [
        SendRequest,
        ToggleSidebar,
        OpenLibrary,
        OpenHistory,
        OpenTests,
        OpenSettings,
        FocusUrl,
        DeleteHistoryEntry
    ]
);

pub const WORKSPACE_CONTEXT: &str = "Workspace";
pub const HISTORY_PANEL_CONTEXT: &str = "HistoryPanel";
const INPUT_CONTEXT: &str = "Input";

// gpui-pre 0.3.11's `KeyBinding::new` can't parse "secondary-".
#[cfg(target_os = "macos")]
mod keys {
    pub const SEND: &str = "cmd-enter";
    pub const TOGGLE_SIDEBAR: &str = "cmd-b";
    pub const LIBRARY: &str = "cmd-1";
    pub const HISTORY: &str = "cmd-2";
    pub const HISTORY_ALT: &str = "cmd-y";
    pub const TESTS: &str = "cmd-3";
    pub const SETTINGS: &str = "cmd-,";
    pub const FOCUS_URL: &str = "cmd-l";
    pub const DELETE_HISTORY_ENTRY: &[&str] = &["backspace", "delete"];
}
#[cfg(not(target_os = "macos"))]
mod keys {
    pub const SEND: &str = "ctrl-enter";
    pub const TOGGLE_SIDEBAR: &str = "ctrl-b";
    pub const LIBRARY: &str = "ctrl-1";
    pub const HISTORY: &str = "ctrl-2";
    pub const HISTORY_ALT: &str = "ctrl-h";
    pub const TESTS: &str = "ctrl-3";
    pub const SETTINGS: &str = "ctrl-,";
    pub const FOCUS_URL: &str = "ctrl-l";
    pub const DELETE_HISTORY_ENTRY: &[&str] = &["delete"];
}

/// Must run after `gpui_component::init` so these override the input bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(keys::SEND, SendRequest, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SEND, SendRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::TOGGLE_SIDEBAR, ToggleSidebar, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::LIBRARY, OpenLibrary, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::HISTORY_ALT, OpenHistory, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::HISTORY, OpenHistory, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::TESTS, OpenTests, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SETTINGS, OpenSettings, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::FOCUS_URL, FocusUrl, Some(WORKSPACE_CONTEXT)),
    ]);
    cx.bind_keys(
        keys::DELETE_HISTORY_ENTRY
            .iter()
            .map(|key| KeyBinding::new(key, DeleteHistoryEntry, Some(HISTORY_PANEL_CONTEXT))),
    );
}
