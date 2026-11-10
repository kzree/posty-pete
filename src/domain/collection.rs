use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::request::Request;

/// A named tree of folders and saved requests, stored as one JSON file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Item {
    Folder(Folder),
    Request(SavedRequest),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedRequest {
    pub id: Uuid,
    pub name: String,
    pub request: Request,
}

pub const NEW_COLLECTION_NAME: &str = "New Collection";
pub const NEW_REQUEST_NAME: &str = "New Request";

/// Where a new request goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    /// A collection created for it, named [`NEW_COLLECTION_NAME`].
    NewCollection,
    /// The folder, or the collection's root when `folder_id` is `None`.
    Folder {
        collection_id: Uuid,
        folder_id: Option<Uuid>,
    },
}

/// Adds a request at `location`, creating the collection for
/// `Location::NewCollection`. Returns the collection and request ids, or
/// `None` (changing nothing) if the collection or folder doesn't exist.
pub fn add_request_at(
    collections: &mut Vec<Collection>,
    location: Location,
    name: impl Into<String>,
    request: Request,
) -> Option<(Uuid, Uuid)> {
    let saved = SavedRequest::new(name, request);
    let request_id = saved.id;
    let item = Item::Request(saved);
    match location {
        Location::NewCollection => {
            let mut collection = Collection::new(NEW_COLLECTION_NAME);
            collection.items.push(item);
            let collection_id = collection.id;
            collections.push(collection);
            Some((collection_id, request_id))
        }
        Location::Folder {
            collection_id,
            folder_id,
        } => collections
            .iter_mut()
            .find(|collection| collection.id == collection_id)?
            .add_item(folder_id, item)
            .then_some((collection_id, request_id)),
    }
}

/// The name and location Save As starts with. A saved request (`linked`, as
/// collection and request ids) goes next to itself, named `typed_name` or
/// else its stored name; a draft goes to the first collection's root.
pub fn save_as_defaults(
    collections: &[Collection],
    linked: Option<(Uuid, Uuid)>,
    typed_name: Option<String>,
) -> (String, Location) {
    let (name, location) = match linked {
        Some((collection_id, request_id)) => {
            let collection = collections
                .iter()
                .find(|collection| collection.id == collection_id);
            let name = typed_name.or_else(|| {
                collection
                    .and_then(|collection| collection.find_request(request_id))
                    .map(|saved| saved.name.clone())
            });
            let folder_id = collection
                .and_then(|collection| collection.ancestors(request_id))
                .and_then(|ancestors| ancestors.last().copied());
            let location = Location::Folder {
                collection_id,
                folder_id,
            };
            (name, location)
        }
        None => {
            let location = collections
                .first()
                .map_or(Location::NewCollection, |collection| Location::Folder {
                    collection_id: collection.id,
                    folder_id: None,
                });
            (None, location)
        }
    };
    (
        name.unwrap_or_else(|| NEW_REQUEST_NAME.to_string()),
        location,
    )
}

impl Collection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            items: Vec::new(),
        }
    }

    pub fn find_request(&self, id: Uuid) -> Option<&SavedRequest> {
        find_request_in(&self.items, id)
    }

    pub fn find_folder(&self, id: Uuid) -> Option<&Folder> {
        find_folder_in(&self.items, id)
    }

    pub fn find_request_mut(&mut self, id: Uuid) -> Option<&mut SavedRequest> {
        let (items, index) = locate(&mut self.items, id)?;
        match &mut items[index] {
            Item::Request(request) => Some(request),
            Item::Folder(_) => None,
        }
    }

    /// Adds `item` to the folder `parent`, or to the root when `parent` is `None`.
    /// Returns false if `parent` isn't a folder in this collection.
    pub fn add_item(&mut self, parent: Option<Uuid>, item: Item) -> bool {
        let Some(parent) = parent else {
            self.items.push(item);
            return true;
        };
        let Some((items, index)) = locate(&mut self.items, parent) else {
            return false;
        };
        match &mut items[index] {
            Item::Folder(folder) => {
                folder.items.push(item);
                true
            }
            Item::Request(_) => false,
        }
    }

    /// The root and every folder, depth-first, with paths like "API / Users / Admin".
    pub fn folder_paths(&self) -> Vec<(Option<Uuid>, String)> {
        let mut paths = vec![(None, self.name.clone())];
        collect_folder_paths(&self.items, &self.name, &mut paths);
        paths
    }

    /// The folders from the root down to the one holding `id`; empty if `id`
    /// is at the root, `None` if it isn't in this collection.
    pub fn ancestors(&self, id: Uuid) -> Option<Vec<Uuid>> {
        let mut path = Vec::new();
        ancestors_in(&self.items, id, &mut path).then_some(path)
    }

    pub fn rename(&mut self, id: Uuid, name: impl Into<String>) -> bool {
        let Some((items, index)) = locate(&mut self.items, id) else {
            return false;
        };
        match &mut items[index] {
            Item::Folder(folder) => folder.name = name.into(),
            Item::Request(request) => request.name = name.into(),
        }
        true
    }

    pub fn remove(&mut self, id: Uuid) -> Option<Item> {
        let (items, index) = locate(&mut self.items, id)?;
        Some(items.remove(index))
    }
}

impl Item {
    fn id(&self) -> Uuid {
        match self {
            Self::Folder(folder) => folder.id,
            Self::Request(request) => request.id,
        }
    }
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            items: Vec::new(),
        }
    }
}

impl SavedRequest {
    pub fn new(name: impl Into<String>, request: Request) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            request,
        }
    }
}

/// The list holding the item with `id`, at any depth, and the item's index in it.
fn locate(items: &mut Vec<Item>, id: Uuid) -> Option<(&mut Vec<Item>, usize)> {
    if let Some(index) = items.iter().position(|item| item.id() == id) {
        return Some((items, index));
    }
    items.iter_mut().find_map(|item| match item {
        Item::Folder(folder) => locate(&mut folder.items, id),
        Item::Request(_) => None,
    })
}

fn collect_folder_paths(items: &[Item], prefix: &str, paths: &mut Vec<(Option<Uuid>, String)>) {
    for item in items {
        if let Item::Folder(folder) = item {
            let path = format!("{prefix} / {}", folder.name);
            paths.push((Some(folder.id), path.clone()));
            collect_folder_paths(&folder.items, &path, paths);
        }
    }
}

fn ancestors_in(items: &[Item], id: Uuid, path: &mut Vec<Uuid>) -> bool {
    items.iter().any(|item| {
        if item.id() == id {
            return true;
        }
        let Item::Folder(folder) = item else {
            return false;
        };
        path.push(folder.id);
        if ancestors_in(&folder.items, id, path) {
            return true;
        }
        path.pop();
        false
    })
}

fn find_folder_in(items: &[Item], id: Uuid) -> Option<&Folder> {
    items.iter().find_map(|item| match item {
        Item::Folder(folder) if folder.id == id => Some(folder),
        Item::Folder(folder) => find_folder_in(&folder.items, id),
        Item::Request(_) => None,
    })
}

fn find_request_in(items: &[Item], id: Uuid) -> Option<&SavedRequest> {
    items.iter().find_map(|item| match item {
        Item::Request(request) if request.id == id => Some(request),
        Item::Request(_) => None,
        Item::Folder(folder) => find_request_in(&folder.items, id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{Auth, Body, HttpMethod, KeyValue};

    fn request(url: &str) -> Request {
        Request {
            method: HttpMethod::Post,
            url: url.into(),
            params: vec![KeyValue {
                enabled: true,
                key: "q".into(),
                value: "1".into(),
            }],
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Bearer { token: "t".into() },
        }
    }

    struct Fixture {
        collection: Collection,
        root_request: Uuid,
        outer: Uuid,
        inner: Uuid,
        deep_request: Uuid,
    }

    /// root request, plus outer folder > inner folder > deep request.
    fn fixture() -> Fixture {
        let mut collection = Collection::new("API");
        let root = SavedRequest::new("Root", request("example.com/root"));
        let outer = Folder::new("Outer");
        let inner = Folder::new("Inner");
        let deep = SavedRequest::new("Deep", request("example.com/deep"));
        let ids = (root.id, outer.id, inner.id, deep.id);

        assert!(collection.add_item(None, Item::Request(root)));
        assert!(collection.add_item(None, Item::Folder(outer)));
        assert!(collection.add_item(Some(ids.1), Item::Folder(inner)));
        assert!(collection.add_item(Some(ids.2), Item::Request(deep)));

        Fixture {
            collection,
            root_request: ids.0,
            outer: ids.1,
            inner: ids.2,
            deep_request: ids.3,
        }
    }

    fn folder(collection: &Collection, path: &[usize]) -> Folder {
        let mut items = &collection.items;
        let mut found = None;
        for &index in path {
            let Item::Folder(folder) = &items[index] else {
                panic!("expected a folder at {path:?}");
            };
            items = &folder.items;
            found = Some(folder);
        }
        found.expect("empty path").clone()
    }

    #[test]
    fn json_round_trip_keeps_nested_tree() {
        let f = fixture();
        let json = serde_json::to_string_pretty(&f.collection).unwrap();
        let loaded: Collection = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.id, f.collection.id);
        assert_eq!(loaded.name, "API");
        assert_eq!(loaded.find_request(f.root_request).unwrap().name, "Root");
        let deep = loaded.find_request(f.deep_request).unwrap();
        assert_eq!(deep.name, "Deep");
        assert_eq!(deep.request.url, "example.com/deep");
        assert_eq!(deep.request.method, HttpMethod::Post);
        assert_eq!(folder(&loaded, &[1]).id, f.outer);
        assert_eq!(folder(&loaded, &[1, 0]).id, f.inner);

        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(&f.collection).unwrap()
        );
    }

    #[test]
    fn items_are_tagged_by_type() {
        let f = fixture();
        let value = serde_json::to_value(&f.collection).unwrap();
        assert_eq!(value["items"][0]["type"], "request");
        assert_eq!(value["items"][1]["type"], "folder");
        assert_eq!(value["items"][1]["items"][0]["items"][0]["name"], "Deep");
    }

    #[test]
    fn add_item_into_root_and_nested_folder() {
        let mut f = fixture();
        let extra = SavedRequest::new("Extra", request("example.com/extra"));
        let extra_id = extra.id;
        assert!(f.collection.add_item(None, Item::Request(extra)));
        assert_eq!(f.collection.items.len(), 3);

        let nested = Folder::new("Nested");
        assert!(f.collection.add_item(Some(f.inner), Item::Folder(nested)));
        assert_eq!(folder(&f.collection, &[1, 0]).items.len(), 2);
        assert!(f.collection.find_request(extra_id).is_some());
    }

    #[test]
    fn add_item_into_missing_parent_leaves_tree_unchanged() {
        let mut f = fixture();
        let before = serde_json::to_value(&f.collection).unwrap();

        let item = Item::Folder(Folder::new("Lost"));
        assert!(!f.collection.add_item(Some(Uuid::new_v4()), item));
        let item = Item::Folder(Folder::new("Under a request"));
        assert!(!f.collection.add_item(Some(f.root_request), item));

        assert_eq!(serde_json::to_value(&f.collection).unwrap(), before);
    }

    #[test]
    fn rename_nested_folder_and_request() {
        let mut f = fixture();
        assert!(f.collection.rename(f.inner, "Renamed folder"));
        assert!(f.collection.rename(f.deep_request, "Renamed request"));
        assert!(!f.collection.rename(Uuid::new_v4(), "Nobody"));

        assert_eq!(folder(&f.collection, &[1, 0]).name, "Renamed folder");
        let deep = f.collection.find_request(f.deep_request).unwrap();
        assert_eq!(deep.name, "Renamed request");
    }

    #[test]
    fn remove_folder_removes_its_children() {
        let mut f = fixture();
        let Some(Item::Folder(removed)) = f.collection.remove(f.outer) else {
            panic!("expected the outer folder");
        };
        assert_eq!(removed.id, f.outer);
        assert!(f.collection.find_request(f.deep_request).is_none());
        assert!(f.collection.find_request(f.root_request).is_some());
        assert_eq!(f.collection.items.len(), 1);
        assert!(f.collection.remove(f.outer).is_none());
    }

    #[test]
    fn find_folder_at_any_depth() {
        let f = fixture();
        assert_eq!(f.collection.find_folder(f.outer).unwrap().name, "Outer");
        assert_eq!(f.collection.find_folder(f.inner).unwrap().name, "Inner");
        assert!(f.collection.find_folder(f.deep_request).is_none());
        assert!(f.collection.find_folder(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_request_two_folders_deep() {
        let mut f = fixture();
        assert_eq!(
            f.collection.find_request(f.deep_request).unwrap().name,
            "Deep"
        );
        assert!(f.collection.find_request(f.inner).is_none());

        let deep = f.collection.find_request_mut(f.deep_request).unwrap();
        deep.request.url = "example.com/changed".into();
        assert_eq!(
            f.collection
                .find_request(f.deep_request)
                .unwrap()
                .request
                .url,
            "example.com/changed"
        );
        assert!(f.collection.find_request_mut(f.outer).is_none());
    }

    #[test]
    fn folder_paths_list_root_then_folders_depth_first() {
        let mut f = fixture();
        let sibling = Folder::new("Sibling");
        let sibling_id = sibling.id;
        assert!(f.collection.add_item(None, Item::Folder(sibling)));

        assert_eq!(
            f.collection.folder_paths(),
            vec![
                (None, "API".to_string()),
                (Some(f.outer), "API / Outer".to_string()),
                (Some(f.inner), "API / Outer / Inner".to_string()),
                (Some(sibling_id), "API / Sibling".to_string()),
            ]
        );
    }

    #[test]
    fn ancestors_lead_down_to_the_item() {
        let f = fixture();
        assert_eq!(
            f.collection.ancestors(f.deep_request),
            Some(vec![f.outer, f.inner])
        );
        assert_eq!(f.collection.ancestors(f.inner), Some(vec![f.outer]));
        assert_eq!(f.collection.ancestors(f.root_request), Some(vec![]));
        assert_eq!(f.collection.ancestors(Uuid::new_v4()), None);
    }

    #[test]
    fn add_request_at_a_folder() {
        let f = fixture();
        let collection_id = f.collection.id;
        let mut collections = vec![f.collection];
        let location = Location::Folder {
            collection_id,
            folder_id: Some(f.inner),
        };

        let (added_to, id) = add_request_at(
            &mut collections,
            location,
            "Copy",
            request("example.com/copy"),
        )
        .unwrap();

        assert_eq!(added_to, collection_id);
        let saved = collections[0].find_request(id).unwrap();
        assert_eq!(saved.name, "Copy");
        assert_eq!(saved.request.url, "example.com/copy");
        assert_eq!(collections[0].ancestors(id), Some(vec![f.outer, f.inner]));
        assert_eq!(collections.len(), 1);
    }

    #[test]
    fn add_request_at_the_root() {
        let f = fixture();
        let collection_id = f.collection.id;
        let mut collections = vec![f.collection];
        let location = Location::Folder {
            collection_id,
            folder_id: None,
        };

        let (_, id) =
            add_request_at(&mut collections, location, "Top", Request::default()).unwrap();

        assert_eq!(collections[0].items.len(), 3);
        assert_eq!(collections[0].ancestors(id), Some(vec![]));
    }

    #[test]
    fn add_request_at_a_new_collection() {
        let mut collections = vec![fixture().collection];

        let (collection_id, id) = add_request_at(
            &mut collections,
            Location::NewCollection,
            "First",
            request("example.com/first"),
        )
        .unwrap();

        assert_eq!(collections.len(), 2);
        let created = &collections[1];
        assert_eq!(created.id, collection_id);
        assert_eq!(created.name, NEW_COLLECTION_NAME);
        assert_eq!(created.items.len(), 1);
        assert_eq!(created.find_request(id).unwrap().name, "First");
    }

    #[test]
    fn add_request_at_a_missing_location_changes_nothing() {
        let f = fixture();
        let collection_id = f.collection.id;
        let mut collections = vec![f.collection];
        let before = serde_json::to_value(&collections).unwrap();

        let missing_folder = Location::Folder {
            collection_id,
            folder_id: Some(Uuid::new_v4()),
        };
        let missing_collection = Location::Folder {
            collection_id: Uuid::new_v4(),
            folder_id: None,
        };
        let under_request = Location::Folder {
            collection_id,
            folder_id: Some(f.root_request),
        };
        for location in [missing_folder, missing_collection, under_request] {
            assert!(
                add_request_at(&mut collections, location, "Lost", Request::default()).is_none()
            );
        }

        assert_eq!(serde_json::to_value(&collections).unwrap(), before);
    }

    #[test]
    fn save_as_defaults_put_a_nested_request_in_its_folder_under_its_stored_name() {
        let f = fixture();
        let collection_id = f.collection.id;
        let (name, location) =
            save_as_defaults(&[f.collection], Some((collection_id, f.deep_request)), None);
        assert_eq!(name, "Deep");
        assert_eq!(
            location,
            Location::Folder {
                collection_id,
                folder_id: Some(f.inner),
            }
        );
    }

    #[test]
    fn save_as_defaults_prefer_the_typed_name_and_root_for_a_root_request() {
        let f = fixture();
        let collection_id = f.collection.id;
        let (name, location) = save_as_defaults(
            &[f.collection],
            Some((collection_id, f.root_request)),
            Some("Edited".into()),
        );
        assert_eq!(name, "Edited");
        assert_eq!(
            location,
            Location::Folder {
                collection_id,
                folder_id: None,
            }
        );
    }

    #[test]
    fn save_as_defaults_put_a_draft_at_the_first_collection_root() {
        let first = fixture().collection;
        let second = Collection::new("Other");
        let first_id = first.id;
        let (name, location) = save_as_defaults(&[first, second], None, Some("Ignored".into()));
        assert_eq!(name, NEW_REQUEST_NAME);
        assert_eq!(
            location,
            Location::Folder {
                collection_id: first_id,
                folder_id: None,
            }
        );
    }

    #[test]
    fn save_as_defaults_without_collections_offer_a_new_collection() {
        let (name, location) = save_as_defaults(&[], None, None);
        assert_eq!(name, NEW_REQUEST_NAME);
        assert_eq!(location, Location::NewCollection);
    }
}
