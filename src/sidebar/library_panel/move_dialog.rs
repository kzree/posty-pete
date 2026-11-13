use std::{cell::Cell, rc::Rc};

use domain::collection::{Destination, Location};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Subscription, Window, px,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogDescription, DialogFooter},
    form::{field, v_form},
    select::SelectEvent,
};

use super::{LibraryPanel, LibraryPanelEvent, menu::MenuTarget};
use crate::ui::location_select::{
    LocationItem, LocationSelect, confirm_on_enter, folder_locations, location_of, location_select,
    new_location_select,
};

impl LibraryPanel {
    pub(super) fn move_to(
        &mut self,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delegate = self.list.read(cx).delegate();
        let (title, skip) = match target {
            MenuTarget::Request { .. } => ("Move request", None),
            MenuTarget::Folder { folder_id, .. } => ("Move folder", Some(folder_id)),
            MenuTarget::Collection(_) => return,
        };
        let collections = delegate.collections();
        let (Some(name), Some(current)) = (
            delegate.target_name(target),
            location_of(collections, target.collection_id(), target.id()),
        ) else {
            return;
        };
        let locations = folder_locations(collections, skip);
        let panel = cx.weak_entity();
        open_move_dialog(
            title,
            name,
            locations,
            current,
            move |to, _, cx| {
                if let Some(event) = move_event(target, to) {
                    panel.update(cx, |_, cx| cx.emit(event)).ok();
                }
            },
            window,
            cx,
        );
    }
}

/// The event moving `target` to `to`, as a drop there would; `None` for a collection.
fn move_event(target: MenuTarget, to: Destination) -> Option<LibraryPanelEvent> {
    let collection_id = target.collection_id();
    match target {
        MenuTarget::Request { request_id, .. } => Some(LibraryPanelEvent::MoveRequest {
            collection_id,
            request_id,
            to,
        }),
        MenuTarget::Folder { folder_id, .. } => Some(LibraryPanelEvent::MoveFolder {
            collection_id,
            folder_id,
            to,
        }),
        MenuTarget::Collection(_) => None,
    }
}

/// Asks where to move `name`, from `locations` with `current` selected.
/// `on_move` gets the chosen container, with the item going at its end.
fn open_move_dialog(
    title: impl Into<SharedString>,
    name: impl Into<SharedString>,
    locations: Vec<LocationItem>,
    current: Location,
    on_move: impl Fn(Destination, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, name) = (title.into(), name.into());
    let select = new_location_select(locations, current, window, cx);
    let footer = cx.new(|cx| MoveFooter::new(select.clone(), current, cx));
    let confirm = Rc::new({
        let select = select.clone();
        move |window: &mut Window, cx: &mut App| {
            let Some(to) = destination(&select, current, cx) else {
                return false;
            };
            on_move(to, window, cx);
            true
        }
    });
    // The builder reruns on every rebuild of the dialog layer.
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            select.focus_handle(cx).focus(window, cx);
        }
        let (confirm_on_ok, confirm_on_enter_key) = (confirm.clone(), confirm.clone());
        dialog
            .title(title.clone())
            .w(px(420.))
            .child(DialogDescription::new().child(SharedString::from(format!(
                "“{name}” goes at the end of the chosen folder or collection."
            ))))
            .child(
                v_form().child(field().label("Location").child(confirm_on_enter(
                    location_select(&select),
                    move |window, cx| {
                        if confirm_on_enter_key(window, cx) {
                            window.close_dialog(cx);
                        }
                    },
                ))),
            )
            .footer(footer.clone())
            .on_ok(move |_, window, cx| confirm_on_ok(window, cx))
    });
}

/// The selected container, unless it's still `current`.
fn destination(
    select: &Entity<LocationSelect>,
    current: Location,
    cx: &App,
) -> Option<Destination> {
    let selected = select.read(cx).selected_value().copied()?;
    let Location::Folder {
        collection_id,
        folder_id,
    } = selected
    else {
        return None;
    };
    (selected != current).then_some(Destination {
        collection_id,
        folder_id,
        before: None,
    })
}

/// A view of its own, so the Move button re-renders as the selection changes.
struct MoveFooter {
    select: Entity<LocationSelect>,
    current: Location,
    _subscription: Subscription,
}

impl MoveFooter {
    fn new(select: Entity<LocationSelect>, current: Location, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&select, |_, _, _: &SelectEvent<_>, cx| cx.notify());
        Self {
            select,
            current,
            _subscription: subscription,
        }
    }
}

impl Render for MoveFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let unchanged = destination(&self.select, self.current, cx).is_none();
        DialogFooter::new()
            .child(DialogClose::new().child(Button::new("move-dialog-cancel").label("Cancel")))
            .child(
                DialogAction::new().child(
                    Button::new("move-dialog-confirm")
                        .primary()
                        .label("Move")
                        .disabled(unchanged),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{TestAppContext, VisualTestContext, div};
    use uuid::Uuid;

    use super::*;
    use crate::{sidebar::library_panel::test_support::fixture, test_support::open_view};

    struct Blank;

    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    type Moves = Rc<RefCell<Vec<Destination>>>;

    /// Opens the dialog for the folder Inner, which sits in Outer.
    fn open(cx: &mut TestAppContext) -> (Moves, Uuid, &mut VisualTestContext) {
        let (_, cx) = open_view(cx, |_, _| Blank);
        cx.update(|_, cx| crate::actions::bind_keys(cx));
        let (collections, ids) = fixture();
        let moves = Moves::default();
        let sink = moves.clone();
        cx.update(|window, cx| {
            open_move_dialog(
                "Move folder",
                "Inner",
                folder_locations(&collections, Some(ids.inner)),
                location_of(&collections, ids.collection, ids.inner).unwrap(),
                move |to, _, _| sink.borrow_mut().push(to),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        (moves, ids.collection, cx)
    }

    fn dialog_open(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    #[gpui::test]
    fn enter_moves_once_another_location_is_picked(cx: &mut TestAppContext) {
        let (moves, collection_id, cx) = open(cx);

        cx.simulate_keystrokes("enter");
        assert!(dialog_open(cx));
        assert!(moves.borrow().is_empty());

        cx.simulate_keystrokes("down");
        cx.simulate_input("API");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(dialog_open(cx));
        assert!(moves.borrow().is_empty());

        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            *moves.borrow(),
            [Destination {
                collection_id,
                folder_id: None,
                before: None,
            }]
        );
        assert!(!dialog_open(cx));
    }

    #[gpui::test]
    fn escape_cancels(cx: &mut TestAppContext) {
        let (moves, _, cx) = open(cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!dialog_open(cx));
        assert!(moves.borrow().is_empty());
    }
}
