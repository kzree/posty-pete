use std::{cell::Cell, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Subscription, Window, px,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogFooter},
    input::{Input, InputEvent, InputState},
};

/// Opens a dialog with a single name input, prefilled with `initial` and
/// selected. `on_save` gets the trimmed, non-empty name.
pub fn open_rename_dialog(
    title: impl Into<SharedString>,
    initial: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    on_save: impl Fn(String, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title = title.into();
    let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
    let footer = cx.new(|cx| RenameFooter::new(input.clone(), confirm_label.into(), cx));
    let on_save = Rc::new(on_save);
    // The builder reruns on every rebuild of the dialog layer.
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        let (input_for_ok, on_save) = (input.clone(), on_save.clone());
        dialog
            .title(title.clone())
            .w(px(360.))
            .child(Input::new(&input))
            .footer(footer.clone())
            .on_ok(move |_, window, cx| {
                let Some(name) = trimmed_value(&input_for_ok, cx) else {
                    return false;
                };
                on_save(name, window, cx);
                true
            })
    });
}

fn trimmed_value(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value();
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// A view of its own, so the confirm button re-renders as the input changes.
struct RenameFooter {
    input: Entity<InputState>,
    confirm_label: SharedString,
    _subscription: Subscription,
}

impl RenameFooter {
    fn new(input: Entity<InputState>, confirm_label: SharedString, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        });
        Self {
            input,
            confirm_label,
            _subscription: subscription,
        }
    }
}

impl Render for RenameFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = trimmed_value(&self.input, cx).is_none();
        DialogFooter::new()
            .child(DialogClose::new().child(Button::new("rename-dialog-cancel").label("Cancel")))
            .child(
                DialogAction::new().child(
                    Button::new("rename-dialog-confirm")
                        .primary()
                        .label(self.confirm_label.clone())
                        .disabled(empty),
                ),
            )
    }
}
