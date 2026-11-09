use gpui::{App, KeyBinding, actions};

actions!(posty_pete, [SendRequest, ToggleSidebar]);

pub const WORKSPACE_CONTEXT: &str = "Workspace";
const INPUT_CONTEXT: &str = "Input";

// gpui-pre 0.3.11's `KeyBinding::new` can't parse "secondary-".
#[cfg(target_os = "macos")]
mod keys {
    pub const SEND: &str = "cmd-enter";
    pub const TOGGLE_SIDEBAR: &str = "cmd-b";
}
#[cfg(not(target_os = "macos"))]
mod keys {
    pub const SEND: &str = "ctrl-enter";
    pub const TOGGLE_SIDEBAR: &str = "ctrl-b";
}

/// Must run after `gpui_component::init` so these override the input bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(keys::SEND, SendRequest, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SEND, SendRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::TOGGLE_SIDEBAR, ToggleSidebar, Some(WORKSPACE_CONTEXT)),
    ]);
}
