use std::{cell::Cell, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Subscription, Window, px,
};
use gpui_component::{
    Disableable as _, IndexPath, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogFooter},
    form::{field, v_form},
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableListItem,
    select::{SearchableVec, Select, SelectEvent, SelectState},
};

use crate::domain::collection::{Collection, Location};

#[derive(Clone)]
pub struct LocationItem {
    location: Location,
    label: SharedString,
}

impl SearchableListItem for LocationItem {
    type Value = Location;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Location {
        &self.location
    }
}

type LocationSelect = SelectState<SearchableVec<LocationItem>>;

/// "New collection", then every collection and folder.
pub fn locations(collections: &[Collection]) -> Vec<LocationItem> {
    let new_collection = LocationItem {
        location: Location::NewCollection,
        label: "New collection".into(),
    };
    let folders = collections.iter().flat_map(|collection| {
        collection
            .folder_paths()
            .into_iter()
            .map(|(folder_id, path)| LocationItem {
                location: Location::Folder {
                    collection_id: collection.id,
                    folder_id,
                },
                label: path.into(),
            })
    });
    std::iter::once(new_collection).chain(folders).collect()
}

/// Asks for a name and a location from `locations`. `on_save` gets the
/// trimmed, non-empty name and the chosen location.
pub fn open_save_as_dialog(
    name: impl Into<SharedString>,
    locations: Vec<LocationItem>,
    selected: Location,
    on_save: impl Fn(String, Location, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
    let selected = locations
        .iter()
        .position(|item| item.location == selected)
        .unwrap_or_default();
    let location = cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(locations),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
        .searchable(true)
    });
    let footer = cx.new(|cx| SaveAsFooter::new(input.clone(), &location, cx));
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
        let (input_for_ok, location_for_ok, on_save) =
            (input.clone(), location.clone(), on_save.clone());
        dialog
            .title("Save As")
            .w(px(420.))
            .child(
                v_form()
                    .child(field().label("Name").child(Input::new(&input)))
                    .child(
                        field()
                            .label("Location")
                            .child(Select::new(&location).search_placeholder("Search folders")),
                    ),
            )
            .footer(footer.clone())
            .on_ok(move |_, window, cx| {
                let Some(name) = trimmed_value(&input_for_ok, cx) else {
                    return false;
                };
                let Some(location) = location_for_ok.read(cx).selected_value().copied() else {
                    return false;
                };
                on_save(name, location, window, cx);
                true
            })
    });
}

fn trimmed_value(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value();
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// A view of its own, so the Save button re-renders as the name changes.
struct SaveAsFooter {
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SaveAsFooter {
    fn new(
        input: Entity<InputState>,
        location: &Entity<LocationSelect>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
            cx.subscribe(location, |_, _, _: &SelectEvent<_>, cx| cx.notify()),
        ];
        Self {
            input,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SaveAsFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = trimmed_value(&self.input, cx).is_none();
        DialogFooter::new()
            .child(DialogClose::new().child(Button::new("save-as-cancel").label("Cancel")))
            .child(
                DialogAction::new().child(
                    Button::new("save-as-confirm")
                        .primary()
                        .label("Save")
                        .disabled(empty),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::collection::{Folder, Item};

    #[test]
    fn new_collection_comes_first_then_each_collection_and_folder() {
        let mut api = Collection::new("API");
        let users = Folder::new("Users");
        let users_id = users.id;
        api.add_item(None, Item::Folder(users));
        let other = Collection::new("Other");

        let items = locations(&[api.clone(), other.clone()]);
        let labels: Vec<_> = items.iter().map(|item| item.label.to_string()).collect();
        assert_eq!(labels, ["New collection", "API", "API / Users", "Other"]);
        assert_eq!(items[0].location, Location::NewCollection);
        assert_eq!(
            items[2].location,
            Location::Folder {
                collection_id: api.id,
                folder_id: Some(users_id),
            }
        );
        assert_eq!(
            items[3].location,
            Location::Folder {
                collection_id: other.id,
                folder_id: None,
            }
        );
    }
}
