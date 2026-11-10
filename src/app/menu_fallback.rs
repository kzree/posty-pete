use gpui::{Action, App, Window};
use gpui_component::{Root, WindowExt};

use super::Workspace;
use crate::actions::{
    FocusUrl, GoBack, GoForward, NewRequest, OpenHistory, OpenLibrary, OpenSettings, QuickOpen,
    SaveRequest, SendRequest, ToggleSidebar,
};

/// Menu items dispatch to the focused element, so with focus outside the
/// Workspace (nothing focused, a toast) its handlers wouldn't run and the items
/// would be disabled. These app-level handlers only run when nothing on the
/// focus path handled the action; they focus the Workspace and dispatch again.
/// `RevertRequest` isn't forwarded: a global handler would keep its menu item
/// enabled even when there's nothing to revert.
pub fn init(cx: &mut App) {
    forward::<SendRequest>(cx);
    forward::<SaveRequest>(cx);
    forward::<NewRequest>(cx);
    forward::<GoBack>(cx);
    forward::<GoForward>(cx);
    forward::<ToggleSidebar>(cx);
    forward::<OpenLibrary>(cx);
    forward::<OpenHistory>(cx);
    forward::<OpenSettings>(cx);
    forward::<FocusUrl>(cx);
    forward::<QuickOpen>(cx);
}

fn forward<A: Action>(cx: &mut App) {
    cx.on_action(|action: &A, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        let action = action.boxed_clone();
        // The window is still borrowed by the dispatch that got us here.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| redispatch(action, window, cx))
                .ok();
        });
    });
}

fn redispatch(action: Box<dyn Action>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let Some(Some(root)) = window.root::<Root>() else {
        return;
    };
    let Ok(workspace) = root.read(cx).view().clone().downcast::<Workspace>() else {
        return;
    };
    let focus_handle = workspace.read(cx).focus_handle.clone();
    // Also guards against bouncing back here forever.
    if !window.is_action_available_in(action.as_ref(), &focus_handle) {
        return;
    }
    window.focus(&focus_handle, cx);
    window.dispatch_action(action, cx);
}
