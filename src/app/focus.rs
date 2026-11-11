use gpui::{Context, Subscription, Window};
use gpui_component::WindowExt;

use super::Workspace;

impl Workspace {
    /// Without focus the Workspace's action handlers aren't on the dispatch
    /// path, so its menu items grey out.
    pub(super) fn keep_focus(window: &mut Window, cx: &mut Context<Self>) -> Vec<Subscription> {
        vec![
            cx.on_focus_lost(window, |this, window, cx| {
                if Self::has_modal(window, cx) {
                    if let Some(target) = window.focus_lost_restore_target(cx) {
                        window.focus(&target, cx);
                    }
                } else {
                    window.focus(&this.focus_handle, cx);
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active()
                    && window.focused(cx).is_none()
                    && !Self::has_modal(window, cx)
                {
                    window.focus(&this.focus_handle, cx);
                }
            }),
        ]
    }

    fn has_modal(window: &mut Window, cx: &mut Context<Self>) -> bool {
        window.has_active_dialog(cx) || window.has_active_sheet(cx)
    }
}
