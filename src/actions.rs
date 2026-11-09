use gpui::{App, KeyBinding, actions};

actions!(
    posty_pete,
    [
        SendRequest,
        ToggleSidebar,
        OpenLibrary,
        OpenHistory,
        OpenSettings,
        FocusUrl
    ]
);

pub const WORKSPACE_CONTEXT: &str = "Workspace";
const INPUT_CONTEXT: &str = "Input";

// gpui-pre 0.3.11's `KeyBinding::new` can't parse "secondary-".
#[cfg(target_os = "macos")]
mod keys {
    pub const SEND: &str = "cmd-enter";
    pub const TOGGLE_SIDEBAR: &str = "cmd-b";
    pub const LIBRARY: &str = "cmd-1";
    pub const HISTORY: &str = "cmd-2";
    pub const HISTORY_ALT: &str = "cmd-y";
    pub const SETTINGS: &str = "cmd-,";
    pub const FOCUS_URL: &str = "cmd-l";
}
#[cfg(not(target_os = "macos"))]
mod keys {
    pub const SEND: &str = "ctrl-enter";
    pub const TOGGLE_SIDEBAR: &str = "ctrl-b";
    pub const LIBRARY: &str = "ctrl-1";
    pub const HISTORY: &str = "ctrl-2";
    pub const HISTORY_ALT: &str = "ctrl-h";
    pub const SETTINGS: &str = "ctrl-,";
    pub const FOCUS_URL: &str = "ctrl-l";
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
        KeyBinding::new(keys::SETTINGS, OpenSettings, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::FOCUS_URL, FocusUrl, Some(WORKSPACE_CONTEXT)),
    ]);
}
