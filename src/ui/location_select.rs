use std::sync::Arc;

use gpui::{
    App, AppContext as _, Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Window, actions, div,
};
use gpui_component::{
    IndexPath,
    searchable_list::SearchableListItem,
    select::{SearchableVec, Select, SelectState},
};
use uuid::Uuid;

use domain::collection::{Collection, Location};

actions!(location_select, [ConfirmLocation]);

/// Wraps the select in `confirm_on_enter`; see `actions::bind_keys`.
pub const CONTEXT: &str = "LocationSelect";

#[derive(Clone)]
pub struct LocationItem {
    location: Location,
    label: SharedString,
}

impl LocationItem {
    pub fn new(location: Location, label: impl Into<SharedString>) -> Self {
        Self {
            location,
            label: label.into(),
        }
    }

    #[cfg(test)]
    pub fn location(&self) -> Location {
        self.location
    }

    #[cfg(test)]
    pub fn label(&self) -> &SharedString {
        &self.label
    }
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

pub type LocationSelect = SelectState<SearchableVec<LocationItem>>;

/// Every collection root and folder, as paths, leaving out the folder `skip`
/// and the folders inside it.
pub fn folder_locations(collections: &[Arc<Collection>], skip: Option<Uuid>) -> Vec<LocationItem> {
    collections
        .iter()
        .flat_map(|collection| {
            collection
                .folder_paths()
                .into_iter()
                .filter(move |(folder_id, _)| {
                    let Some((folder_id, skip)) = folder_id.zip(skip) else {
                        return true;
                    };
                    folder_id != skip
                        && !collection
                            .ancestors(folder_id)
                            .is_some_and(|ancestors| ancestors.contains(&skip))
                })
                .map(|(folder_id, path)| {
                    LocationItem::new(
                        Location::Folder {
                            collection_id: collection.id,
                            folder_id,
                        },
                        path,
                    )
                })
        })
        .collect()
}

/// The collection root or folder holding the request or folder `item_id`.
pub fn location_of(
    collections: &[Arc<Collection>],
    collection_id: Uuid,
    item_id: Uuid,
) -> Option<Location> {
    let collection = collections.iter().find(|c| c.id == collection_id)?;
    if item_id == collection_id {
        return None;
    }
    let ancestors = collection.ancestors(item_id)?;
    Some(Location::Folder {
        collection_id,
        folder_id: ancestors.last().copied(),
    })
}

/// A searchable select over `locations` with `selected` chosen, or the first
/// item if it isn't listed.
pub fn new_location_select(
    locations: Vec<LocationItem>,
    selected: Location,
    window: &mut Window,
    cx: &mut App,
) -> Entity<LocationSelect> {
    let selected = locations
        .iter()
        .position(|item| item.location == selected)
        .unwrap_or_default();
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(locations),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
        .searchable(true)
    })
}

pub fn location_select(state: &Entity<LocationSelect>) -> Select<SearchableVec<LocationItem>> {
    Select::new(state).search_placeholder("Search folders")
}

/// Enter on the closed select runs `on_confirm` instead of opening it, so it
/// can confirm a dialog. Space and the arrow keys still open the list.
pub fn confirm_on_enter(
    select: impl IntoElement,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) -> Div {
    div()
        .key_context(CONTEXT)
        .on_action(move |_: &ConfirmLocation, window, cx| on_confirm(window, cx))
        .child(select)
}

#[cfg(test)]
mod tests {
    use domain::collection::{Folder, Item, SavedRequest};
    use domain::request::Request;

    use super::*;

    struct Fixture {
        collections: Vec<Arc<Collection>>,
        api: Uuid,
        other: Uuid,
        outer: Uuid,
        inner: Uuid,
        sibling: Uuid,
        root_request: Uuid,
        deep_request: Uuid,
    }

    /// API: Root request, Outer > Inner > Deep request, Sibling; Other.
    fn fixture() -> Fixture {
        let mut api = Collection::new("API");
        let (outer, inner, sibling) = (
            Folder::new("Outer"),
            Folder::new("Inner"),
            Folder::new("Sibling"),
        );
        let root_request = SavedRequest::new("Root", Request::default());
        let deep_request = SavedRequest::new("Deep", Request::default());
        let f = Fixture {
            collections: Vec::new(),
            api: api.id,
            other: Uuid::nil(),
            outer: outer.id,
            inner: inner.id,
            sibling: sibling.id,
            root_request: root_request.id,
            deep_request: deep_request.id,
        };
        api.add_item(None, Item::Request(root_request));
        api.add_item(None, Item::Folder(outer));
        api.add_item(Some(f.outer), Item::Folder(inner));
        api.add_item(Some(f.inner), Item::Request(deep_request));
        api.add_item(None, Item::Folder(sibling));
        let other = Collection::new("Other");
        Fixture {
            other: other.id,
            collections: vec![Arc::new(api), Arc::new(other)],
            ..f
        }
    }

    fn labels(items: &[LocationItem]) -> Vec<&str> {
        items.iter().map(|item| item.label().as_ref()).collect()
    }

    #[test]
    fn every_collection_and_folder_without_new_collection() {
        let f = fixture();
        let items = folder_locations(&f.collections, None);
        assert_eq!(
            labels(&items),
            [
                "API",
                "API / Outer",
                "API / Outer / Inner",
                "API / Sibling",
                "Other"
            ]
        );
        assert!(
            items
                .iter()
                .all(|item| item.location() != Location::NewCollection)
        );
        assert_eq!(
            items[2].location(),
            Location::Folder {
                collection_id: f.api,
                folder_id: Some(f.inner),
            }
        );
        assert_eq!(
            items[4].location(),
            Location::Folder {
                collection_id: f.other,
                folder_id: None,
            }
        );
    }

    #[test]
    fn skipping_a_folder_leaves_out_its_subtree() {
        let f = fixture();
        assert_eq!(
            labels(&folder_locations(&f.collections, Some(f.outer))),
            ["API", "API / Sibling", "Other"]
        );
        assert_eq!(
            labels(&folder_locations(&f.collections, Some(f.inner))),
            ["API", "API / Outer", "API / Sibling", "Other"]
        );
    }

    #[test]
    fn location_of_is_the_parent_folder_or_root() {
        let f = fixture();
        let folder = |folder_id| {
            Some(Location::Folder {
                collection_id: f.api,
                folder_id,
            })
        };
        let at = |id| location_of(&f.collections, f.api, id);
        assert_eq!(at(f.root_request), folder(None));
        assert_eq!(at(f.deep_request), folder(Some(f.inner)));
        assert_eq!(at(f.inner), folder(Some(f.outer)));
        assert_eq!(at(f.outer), folder(None));
        assert_eq!(at(f.sibling), folder(None));
        assert_eq!(at(f.api), None);
        assert_eq!(at(Uuid::new_v4()), None);
        assert_eq!(location_of(&f.collections, f.other, f.root_request), None);
    }
}
