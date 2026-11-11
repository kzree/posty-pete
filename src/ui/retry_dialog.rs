use std::rc::Rc;

use gpui::{App, ParentElement as _, SharedString, Styled as _, Window};
use gpui_component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryChoice {
    Retry,
    Proceed,
    Cancel,
}

pub struct RetryDialog {
    pub title: SharedString,
    pub description: SharedString,
    pub retry_label: SharedString,
    /// The danger action that goes ahead despite the failure.
    pub proceed_label: SharedString,
}

/// Asks what to do after something failed. Laid out like the save changes
/// dialog: proceed on the left, Cancel and retry on the right. Enter picks
/// retry and Escape picks Cancel. The dialog is closed before `on_choice` runs.
pub fn open_retry_dialog(
    dialog: RetryDialog,
    on_choice: impl Fn(RetryChoice, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let on_choice = Rc::new(on_choice);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let choose = |choice| {
            let on_choice = on_choice.clone();
            move |window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                on_choice(choice, window, cx);
            }
        };
        let (retry, proceed, cancel) = (
            choose(RetryChoice::Retry),
            choose(RetryChoice::Proceed),
            choose(RetryChoice::Cancel),
        );
        let (on_retry, on_cancel) = (retry.clone(), cancel.clone());
        alert
            .title(dialog.title.clone())
            .description(dialog.description.clone())
            // Both close the dialog themselves, so the default close is skipped.
            .on_ok(move |_, window, cx| {
                on_retry(window, cx);
                false
            })
            .on_cancel(move |_, window, cx| {
                on_cancel(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .justify_between()
                    .child(
                        Button::new("proceed")
                            .label(dialog.proceed_label.clone())
                            .danger()
                            .on_click(move |_, window, cx| proceed(window, cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("cancel")
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| cancel(window, cx)),
                            )
                            .child(
                                Button::new("retry")
                                    .label(dialog.retry_label.clone())
                                    .primary()
                                    .on_click(move |_, window, cx| retry(window, cx)),
                            ),
                    ),
            )
    });
}
