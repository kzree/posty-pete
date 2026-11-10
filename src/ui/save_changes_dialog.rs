use std::rc::Rc;

use gpui::{App, ParentElement as _, SharedString, Styled as _, Window};
use gpui_component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// Asks whether to save changes to `name` before they're replaced. Laid out
/// like macOS: Discard on the left, Cancel and Save on the right. Enter picks
/// Save and Escape picks Cancel. The dialog is closed before `on_choice` runs.
pub fn open_save_changes_dialog(
    name: impl Into<SharedString>,
    on_choice: impl Fn(SaveChoice, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title: SharedString = format!("Save changes to “{}”?", name.into()).into();
    let on_choice = Rc::new(on_choice);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let choose = |choice| {
            let on_choice = on_choice.clone();
            move |window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                on_choice(choice, window, cx);
            }
        };
        let (save, discard, cancel) = (
            choose(SaveChoice::Save),
            choose(SaveChoice::Discard),
            choose(SaveChoice::Cancel),
        );
        let (on_save, on_cancel) = (save.clone(), cancel.clone());
        alert
            .title(title.clone())
            .description("Your changes will be lost if you don't save them.")
            // Both close the dialog themselves, so the default close is skipped.
            .on_ok(move |_, window, cx| {
                on_save(window, cx);
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
                        Button::new("discard")
                            .label("Discard")
                            .danger()
                            .on_click(move |_, window, cx| discard(window, cx)),
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
                                Button::new("save")
                                    .label("Save")
                                    .primary()
                                    .on_click(move |_, window, cx| save(window, cx)),
                            ),
                    ),
            )
    });
}
