use gpui::{App, KeyBinding, actions};

actions!(posty_pete, [SendRequest, ToggleSidebar]);

/// Key context set on the root view (`BaseLayout`).
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// gpui-component's input key context.
const INPUT_CONTEXT: &str = "Input";

// `KeyBinding::new` can't parse the `secondary-` modifier in gpui-pre 0.3.11
// (it needs the OS, which `new` doesn't pass), so pick cmd/ctrl at compile time.
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

/// Call after `gpui_component::init`: bindings added later win at equal depth.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(keys::SEND, SendRequest, Some(WORKSPACE_CONTEXT)),
        // Inputs bind cmd-enter themselves (a newline in multi-line editors); this
        // overrides it so Cmd+Enter sends from any input, including the body editor.
        KeyBinding::new(keys::SEND, SendRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::TOGGLE_SIDEBAR, ToggleSidebar, Some(WORKSPACE_CONTEXT)),
    ]);
}
