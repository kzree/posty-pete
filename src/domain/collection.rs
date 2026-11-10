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

impl Collection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            items: Vec::new(),
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by open saved request (task 15)")
    )]
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
}
