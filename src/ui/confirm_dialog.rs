use std::rc::Rc;

use gpui::{App, SharedString, Window};
use gpui_component::{WindowExt as _, button::ButtonVariant};

/// Opens an alert dialog for a destructive action: Cancel and a danger
/// `confirm_label` button that runs `on_confirm`.
pub fn open_confirm_dialog(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, message, confirm_label) = (title.into(), message.into(), confirm_label.into());
    let on_confirm = Rc::new(on_confirm);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_confirm = on_confirm.clone();
        alert
            .title(title.clone())
            .description(message.clone())
            .show_cancel(true)
            .ok_text(confirm_label.clone())
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
    });
}
