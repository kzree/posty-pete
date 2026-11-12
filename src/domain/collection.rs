use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    environment::{Environment, NEW_ENVIRONMENT_NAME, copy_of, unique_name},
    layers::Layers,
    request::{Auth, KeyValue, Request},
};

/// A named tree of folders and saved requests, stored as one JSON file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub variables: Vec<KeyValue>,
    #[serde(default)]
    pub environments: Vec<Environment>,
    /// Inherit means the same as `None` here: there's nothing to inherit from.
    #[serde(default = "no_auth", skip_serializing_if = "sets_no_auth")]
    pub auth: Auth,
}

fn no_auth() -> Auth {
    Auth::None
}

fn sets_no_auth(auth: &Auth) -> bool {
    matches!(auth, Auth::None | Auth::Inherit)
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
    collections: &mut Vec<Arc<Collection>>,
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
            collections.push(Arc::new(collection));
            Some((collection_id, request_id))
        }
        Location::Folder {
            collection_id,
            folder_id,
        } => modify(collections, collection_id, |collection| {
            collection.add_item(folder_id, item)
        })
        .then_some((collection_id, request_id)),
    }
}

/// Runs `f` on the collection, copying it first if it's shared. Returns what
/// `f` returns, or `false` if there's no such collection.
pub fn modify(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    f: impl FnOnce(&mut Collection) -> bool,
) -> bool {
    collections
        .iter_mut()
        .find(|collection| collection.id == collection_id)
        .is_some_and(|collection| f(Arc::make_mut(collection)))
}

/// Replaces the collection's variables. Returns `false`, changing (and
/// copying) nothing, if there's no such collection or they're the same.
pub fn set_variables(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    variables: Vec<KeyValue>,
) -> bool {
    let unchanged = collections
        .iter()
        .find(|collection| collection.id == collection_id)
        .is_none_or(|collection| collection.variables == variables);
    !unchanged
        && modify(collections, collection_id, |collection| {
            collection.variables = variables;
            true
        })
}

/// Adds an environment named [`NEW_ENVIRONMENT_NAME`], numbered if that's
/// taken. Returns its id, or `None` if there's no such collection.
pub fn add_environment(collections: &mut [Arc<Collection>], collection_id: Uuid) -> Option<Uuid> {
    let mut id = None;
    modify(collections, collection_id, |collection| {
        let names = collection.environments.iter().map(|env| env.name.as_str());
        let environment = Environment::new(unique_name(NEW_ENVIRONMENT_NAME, names));
        id = Some(environment.id);
        collection.environments.push(environment);
        true
    });
    id
}

/// Returns `false`, changing (and copying) nothing, if there's no such
/// environment or the name is the same.
pub fn rename_environment(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
    name: impl Into<String>,
) -> bool {
    let name = name.into();
    find_environment(collections, collection_id, environment_id)
        .is_some_and(|environment| environment.name != name)
        && modify_environment(collections, collection_id, environment_id, |environment| {
            environment.name = name;
        })
}

/// Inserts a copy of the environment after it, in the same collection (see
/// [`copy_of`]). Returns the copy's id, or `None`, changing (and
/// copying) nothing, if there's no such environment.
pub fn duplicate_environment(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
) -> Option<Uuid> {
    find_environment(collections, collection_id, environment_id)?;
    let mut id = None;
    modify(collections, collection_id, |collection| {
        let environments = &mut collection.environments;
        let Some(index) = environments.iter().position(|env| env.id == environment_id) else {
            return false;
        };
        let names = environments.iter().map(|env| env.name.as_str());
        let copy = copy_of(&environments[index], names);
        id = Some(copy.id);
        environments.insert(index + 1, copy);
        true
    });
    id
}

/// Returns `false` if there's no such environment.
pub fn delete_environment(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
) -> bool {
    find_environment(collections, collection_id, environment_id).is_some()
        && modify(collections, collection_id, |collection| {
            collection
                .environments
                .retain(|env| env.id != environment_id);
            true
        })
}

/// Replaces the environment's variables. Returns `false`, changing (and
/// copying) nothing, if there's no such environment or they're the same.
pub fn set_environment_variables(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
    variables: Vec<KeyValue>,
) -> bool {
    find_environment(collections, collection_id, environment_id)
        .is_some_and(|environment| environment.variables != variables)
        && modify_environment(collections, collection_id, environment_id, |environment| {
            environment.variables = variables;
        })
}

fn find_environment(
    collections: &[Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
) -> Option<&Environment> {
    collections
        .iter()
        .find(|collection| collection.id == collection_id)?
        .environment(environment_id)
}

fn modify_environment(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
    f: impl FnOnce(&mut Environment),
) -> bool {
    modify(collections, collection_id, |collection| {
        collection
            .environments
            .iter_mut()
            .find(|env| env.id == environment_id)
            .map(f)
            .is_some()
    })
}

/// Bases the environment on shared environment `base`, or unlinks it for
/// `None`. Returns `false`, changing (and copying) nothing, if there's no such
/// environment or the base is the same.
pub fn set_environment_base(
    collections: &mut [Arc<Collection>],
    collection_id: Uuid,
    environment_id: Uuid,
    base: Option<Uuid>,
) -> bool {
    find_environment(collections, collection_id, environment_id)
        .is_some_and(|environment| environment.base != base)
        && modify_environment(collections, collection_id, environment_id, |environment| {
            environment.base = base;
        })
}

/// Unlinks every environment based on shared environment `base`, copying only
/// the collections that have one. Returns their ids.
pub fn unlink_base(collections: &mut [Arc<Collection>], base: Uuid) -> Vec<Uuid> {
    let is_based = |environment: &Environment| environment.base == Some(base);
    collections
        .iter_mut()
        .filter(|collection| collection.environments.iter().any(is_based))
        .map(|collection| {
            let collection = Arc::make_mut(collection);
            for environment in &mut collection.environments {
                if is_based(environment) {
                    environment.base = None;
                }
            }
            collection.id
        })
        .collect()
}

/// How many collection environments are based on each shared environment,
/// by its id.
pub fn base_counts(collections: &[Arc<Collection>]) -> HashMap<Uuid, usize> {
    let mut counts = HashMap::new();
    let bases = collections
        .iter()
        .flat_map(|collection| &collection.environments)
        .filter_map(|environment| environment.base);
    for base in bases {
        *counts.entry(base).or_insert(0) += 1;
    }
    counts
}

/// The name and location Save As starts with. A saved request (`linked`, as
/// collection and request ids) goes next to itself, named `typed_name` or
/// else its stored name; a draft goes to the root of `draft_collection` if
/// it exists, else of the first collection.
pub fn save_as_defaults(
    collections: &[Arc<Collection>],
    linked: Option<(Uuid, Uuid)>,
    typed_name: Option<String>,
    draft_collection: Option<Uuid>,
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
                .iter()
                .find(|collection| Some(collection.id) == draft_collection)
                .or(collections.first())
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
            variables: Vec::new(),
            environments: Vec::new(),
            auth: Auth::None,
        }
    }

    pub fn environment(&self, id: Uuid) -> Option<&Environment> {
        self.environments.iter().find(|env| env.id == id)
    }

    /// What a request in this collection resolves against with
    /// `environment_id` active; see [`Layers::collection`].
    pub fn layers<'a>(
        &'a self,
        environment_id: Option<Uuid>,
        shared: &'a [Arc<Environment>],
    ) -> Layers<'a> {
        Layers::collection(self, environment_id, shared)
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
    use crate::domain::{
        environment,
        request::{Body, HttpMethod, KeyValue},
    };

    #[test]
    fn new_requests_inherit_auth() {
        let mut collections = Vec::new();
        let (collection_id, request_id) = add_request_at(
            &mut collections,
            Location::NewCollection,
            NEW_REQUEST_NAME,
            Request::default(),
        )
        .unwrap();
        let collection = &collections[0];
        assert_eq!(collection.id, collection_id);
        assert_eq!(collection.auth, Auth::None);
        assert_eq!(
            collection.find_request(request_id).unwrap().request.auth,
            Auth::Inherit
        );
    }

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

    fn json(collections: &[Arc<Collection>]) -> Vec<serde_json::Value> {
        collections
            .iter()
            .map(|collection| serde_json::to_value(&**collection).unwrap())
            .collect()
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
    fn json_without_variables_loads_and_saving_writes_them() {
        let json = r#"{
            "id": "6f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f",
            "name": "Old",
            "items": [{"type": "request", "id": "7f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f",
                       "name": "R", "request": {"method": "Get", "url": "x", "params": [],
                       "headers": [], "body": "None", "auth": "None"}}]
        }"#;
        let mut loaded: Collection = serde_json::from_str(json).unwrap();
        assert!(loaded.variables.is_empty());
        assert_eq!(loaded.items.len(), 1);

        loaded.variables.push(KeyValue {
            enabled: true,
            key: "host".into(),
            value: "example.com".into(),
        });
        let value = serde_json::to_value(&loaded).unwrap();
        assert_eq!(value["variables"][0]["key"], "host");
        let reloaded: Collection = serde_json::from_value(value).unwrap();
        assert_eq!(reloaded.variables, loaded.variables);
    }

    #[test]
    fn json_without_auth_loads_as_none_and_saves_without_it() {
        let json = r#"{
            "id": "6f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f",
            "name": "Old",
            "items": [{"type": "request", "id": "7f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f",
                       "name": "R", "request": {"method": "Get", "url": "x", "params": [],
                       "headers": [], "body": "None", "auth": "None"}}],
            "variables": [],
            "environments": [{"id": "8f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f", "name": "Dev",
                              "variables": []}]
        }"#;
        let mut loaded: Collection = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.auth, Auth::None);
        assert_eq!(loaded.environments[0].auth, Auth::Inherit);
        let Item::Request(saved) = &loaded.items[0] else {
            panic!("expected a request");
        };
        assert_eq!(saved.request.auth, Auth::None);
        let expected: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), expected);

        loaded.auth = Auth::Inherit;
        assert_eq!(serde_json::to_value(&loaded).unwrap(), expected);

        loaded.auth = Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        let value = serde_json::to_value(&loaded).unwrap();
        let reloaded: Collection = serde_json::from_value(value).unwrap();
        assert_eq!(reloaded.auth, loaded.auth);
    }

    #[test]
    fn new_collection_writes_empty_variables() {
        let value = serde_json::to_value(Collection::new("API")).unwrap();
        assert_eq!(value["variables"], serde_json::json!([]));
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
        let mut collections = vec![Arc::new(f.collection)];
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
        let mut collections = vec![Arc::new(f.collection)];
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
        let mut collections = vec![Arc::new(fixture().collection)];

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
        let mut collections = vec![Arc::new(f.collection)];
        let before = json(&collections);

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

        assert_eq!(json(&collections), before);
    }

    #[test]
    fn save_as_defaults_put_a_nested_request_in_its_folder_under_its_stored_name() {
        let f = fixture();
        let collection_id = f.collection.id;
        let (name, location) = save_as_defaults(
            &[Arc::new(f.collection)],
            Some((collection_id, f.deep_request)),
            None,
            None,
        );
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
            &[Arc::new(f.collection)],
            Some((collection_id, f.root_request)),
            Some("Edited".into()),
            None,
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
    fn save_as_defaults_put_a_draft_at_the_draft_collection_root() {
        let first = Arc::new(Collection::new("First"));
        let second = Arc::new(Collection::new("Second"));
        let second_id = second.id;
        let (_, location) = save_as_defaults(&[first, second], None, None, Some(second_id));
        assert_eq!(
            location,
            Location::Folder {
                collection_id: second_id,
                folder_id: None,
            }
        );
    }

    #[test]
    fn save_as_defaults_put_a_draft_at_the_first_collection_root() {
        let first = fixture().collection;
        let second = Collection::new("Other");
        let first_id = first.id;
        let (name, location) = save_as_defaults(
            &[Arc::new(first), Arc::new(second)],
            None,
            Some("Ignored".into()),
            Some(Uuid::new_v4()),
        );
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
        let (name, location) = save_as_defaults(&[], None, None, Some(Uuid::new_v4()));
        assert_eq!(name, NEW_REQUEST_NAME);
        assert_eq!(location, Location::NewCollection);
    }

    #[test]
    fn modify_copies_a_shared_collection_and_keeps_the_others_shared() {
        let f = fixture();
        let changed_id = f.collection.id;
        let mut collections = vec![Arc::new(f.collection), Arc::new(Collection::new("Other"))];
        let held = collections.clone();

        assert!(modify(&mut collections, changed_id, |collection| {
            collection.rename(f.root_request, "Renamed")
        }));

        assert_eq!(
            collections[0].find_request(f.root_request).unwrap().name,
            "Renamed"
        );
        assert_eq!(held[0].find_request(f.root_request).unwrap().name, "Root");
        assert!(!Arc::ptr_eq(&collections[0], &held[0]));
        assert!(Arc::ptr_eq(&collections[1], &held[1]));
    }

    #[test]
    fn modify_an_unshared_collection_in_place() {
        let mut collections = vec![Arc::new(fixture().collection)];
        let id = collections[0].id;
        let before = Arc::as_ptr(&collections[0]);

        assert!(modify(&mut collections, id, |collection| {
            collection.name = "Renamed".into();
            true
        }));

        assert_eq!(Arc::as_ptr(&collections[0]), before);
        assert_eq!(collections[0].name, "Renamed");
    }

    #[test]
    fn modify_a_missing_collection_does_nothing() {
        let mut collections = vec![Arc::new(Collection::new("API"))];
        assert!(!modify(&mut collections, Uuid::new_v4(), |_| true));
    }

    fn variable(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn set_variables_replaces_them() {
        let mut collections = vec![Arc::new(Collection::new("API"))];
        let id = collections[0].id;
        let held = collections.clone();
        let variables = vec![variable("host", "example.com"), variable("token", "t")];

        assert!(set_variables(&mut collections, id, variables.clone()));

        assert_eq!(collections[0].variables, variables);
        assert!(held[0].variables.is_empty());
    }

    #[test]
    fn set_variables_to_the_same_ones_changes_nothing() {
        let mut collection = Collection::new("API");
        collection.variables = vec![variable("host", "example.com")];
        let mut collections = vec![Arc::new(collection)];
        let id = collections[0].id;
        let held = collections.clone();

        assert!(!set_variables(
            &mut collections,
            id,
            vec![variable("host", "example.com")]
        ));
        assert!(Arc::ptr_eq(&collections[0], &held[0]));
    }

    #[test]
    fn set_variables_of_a_missing_collection_does_nothing() {
        let mut collections = vec![Arc::new(Collection::new("API"))];
        assert!(!set_variables(
            &mut collections,
            Uuid::new_v4(),
            vec![variable("host", "example.com")]
        ));
        assert!(collections[0].variables.is_empty());
    }

    fn with_environment(variables: Vec<KeyValue>) -> (Vec<Arc<Collection>>, Uuid, Uuid) {
        let mut collections = vec![Arc::new(Collection::new("API"))];
        let collection_id = collections[0].id;
        let environment_id = add_environment(&mut collections, collection_id).unwrap();
        set_environment_variables(&mut collections, collection_id, environment_id, variables);
        (collections, collection_id, environment_id)
    }

    #[test]
    fn json_without_environments_loads_and_environments_round_trip() {
        let json = r#"{"id": "6f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f", "name": "Old", "items": []}"#;
        let loaded: Collection = serde_json::from_str(json).unwrap();
        assert!(loaded.environments.is_empty());

        let (collections, _, environment_id) =
            with_environment(vec![variable("host", "staging.example.com")]);
        let value = serde_json::to_value(&*collections[0]).unwrap();
        assert_eq!(value["environments"][0]["name"], NEW_ENVIRONMENT_NAME);
        let reloaded: Collection = serde_json::from_value(value).unwrap();
        assert_eq!(reloaded.environments, collections[0].environments);
        assert_eq!(reloaded.environments[0].id, environment_id);
    }

    #[test]
    fn variables_with_an_environment_put_its_rows_first() {
        let mut disabled = variable("token", "env-token");
        disabled.enabled = false;
        let (mut collections, collection_id, environment_id) =
            with_environment(vec![variable("host", "staging"), disabled]);
        assert!(set_variables(
            &mut collections,
            collection_id,
            vec![variable("host", "local"), variable("token", "t")]
        ));
        let collection = &collections[0];

        let variables = collection.layers(Some(environment_id), &[]).variables();
        assert_eq!(variables.get("host"), Some("staging"));
        assert_eq!(variables.get("token"), Some("t"));

        let variables = collection.layers(None, &[]).variables();
        assert_eq!(variables.get("host"), Some("local"));
        assert_eq!(
            collection.layers(Some(Uuid::new_v4()), &[]).variables(),
            collection.layers(None, &[]).variables()
        );
    }

    /// A shared environment with `host`, `token` and `region`.
    fn shared_base() -> Vec<Arc<Environment>> {
        let mut shared = Vec::new();
        let id = environment::add(&mut shared);
        environment::set_variables(
            &mut shared,
            id,
            vec![
                variable("host", "shared-host"),
                variable("token", "shared-token"),
                variable("region", "eu"),
            ],
        );
        shared
    }

    #[test]
    fn variables_with_a_based_environment_layer_environment_base_collection() {
        let shared = shared_base();
        let mut disabled = variable("token", "own-token");
        disabled.enabled = false;
        let (mut collections, collection_id, environment_id) =
            with_environment(vec![variable("host", "own-host"), disabled]);
        set_variables(
            &mut collections,
            collection_id,
            vec![
                variable("region", "us"),
                variable("token", "collection-token"),
                variable("user", "me"),
            ],
        );
        assert!(set_environment_base(
            &mut collections,
            collection_id,
            environment_id,
            Some(shared[0].id)
        ));

        let variables = collections[0]
            .layers(Some(environment_id), &shared)
            .variables();
        assert_eq!(variables.get("host"), Some("own-host"));
        assert_eq!(variables.get("token"), Some("shared-token"));
        assert_eq!(variables.get("region"), Some("eu"));
        assert_eq!(variables.get("user"), Some("me"));

        let without_environment = collections[0].layers(None, &shared).variables();
        assert_eq!(without_environment.get("region"), Some("us"));
    }

    #[test]
    fn variables_with_a_missing_base_ignore_it() {
        let (mut collections, collection_id, environment_id) =
            with_environment(vec![variable("host", "own-host")]);
        set_variables(
            &mut collections,
            collection_id,
            vec![variable("region", "us")],
        );
        set_environment_base(
            &mut collections,
            collection_id,
            environment_id,
            Some(Uuid::new_v4()),
        );

        let variables = collections[0]
            .layers(Some(environment_id), &shared_base())
            .variables();
        assert_eq!(variables.get("host"), Some("own-host"));
        assert_eq!(variables.get("region"), Some("us"));
        assert_eq!(variables.get("token"), None);
    }

    #[test]
    fn set_environment_base_only_when_it_changes() {
        let (mut collections, collection_id, environment_id) = with_environment(Vec::new());
        let base = Uuid::new_v4();
        let held = collections.clone();

        assert!(!set_environment_base(
            &mut collections,
            collection_id,
            environment_id,
            None
        ));
        assert!(!set_environment_base(
            &mut collections,
            collection_id,
            Uuid::new_v4(),
            Some(base)
        ));
        assert!(Arc::ptr_eq(&collections[0], &held[0]));

        assert!(set_environment_base(
            &mut collections,
            collection_id,
            environment_id,
            Some(base)
        ));
        assert_eq!(collections[0].environments[0].base, Some(base));
        assert_eq!(held[0].environments[0].base, None);
        assert!(set_environment_base(
            &mut collections,
            collection_id,
            environment_id,
            None
        ));
        assert_eq!(collections[0].environments[0].base, None);
    }

    #[test]
    fn unlink_base_touches_only_referencing_collections() {
        let deleted = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut collections = Vec::new();
        for base in [Some(deleted), Some(other), None] {
            let (mut one, collection_id, environment_id) = with_environment(Vec::new());
            set_environment_base(&mut one, collection_id, environment_id, base);
            collections.append(&mut one);
        }
        let second = collections[1].id;
        let also_based = add_environment(&mut collections, second).unwrap();
        set_environment_base(&mut collections, second, also_based, Some(deleted));
        let held = collections.clone();

        let changed = unlink_base(&mut collections, deleted);
        assert_eq!(changed, [collections[0].id, collections[1].id]);
        assert_eq!(collections[0].environments[0].base, None);
        assert_eq!(collections[1].environments[0].base, Some(other));
        assert_eq!(collections[1].environment(also_based).unwrap().base, None);
        assert!(Arc::ptr_eq(&collections[2], &held[2]));
        assert_eq!(held[0].environments[0].base, Some(deleted));

        assert!(unlink_base(&mut collections, deleted).is_empty());
    }

    #[test]
    fn base_counts_count_environments_per_base() {
        let (base, other) = (Uuid::new_v4(), Uuid::new_v4());
        let mut collections = Vec::new();
        for base in [Some(base), Some(other), None] {
            let (mut one, collection_id, environment_id) = with_environment(Vec::new());
            set_environment_base(&mut one, collection_id, environment_id, base);
            collections.append(&mut one);
        }
        let second = collections[1].id;
        let also_based = add_environment(&mut collections, second).unwrap();
        set_environment_base(&mut collections, second, also_based, Some(base));

        let counts = base_counts(&collections);
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[&base], 2);
        assert_eq!(counts[&other], 1);
    }

    #[test]
    fn json_with_unbased_environments_writes_no_base() {
        let (collections, _, _) = with_environment(Vec::new());
        let value = serde_json::to_value(&*collections[0]).unwrap();
        assert!(value["environments"][0].get("base").is_none());
    }

    #[test]
    fn add_environment_numbers_taken_default_names() {
        let mut collections = vec![Arc::new(Collection::new("API"))];
        let id = collections[0].id;
        let first = add_environment(&mut collections, id).unwrap();
        let second = add_environment(&mut collections, id).unwrap();
        assert!(rename_environment(&mut collections, id, first, "Local"));
        let third = add_environment(&mut collections, id).unwrap();
        add_environment(&mut collections, id).unwrap();

        let names: Vec<_> = collections[0]
            .environments
            .iter()
            .map(|env| env.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Local",
                "New environment 2",
                "New environment",
                "New environment 3"
            ]
        );
        assert_ne!(second, third);
        assert!(add_environment(&mut collections, Uuid::new_v4()).is_none());
    }

    #[test]
    fn rename_environment_only_when_it_changes() {
        let (mut collections, collection_id, environment_id) = with_environment(Vec::new());
        let held = collections.clone();

        assert!(!rename_environment(
            &mut collections,
            collection_id,
            environment_id,
            NEW_ENVIRONMENT_NAME
        ));
        assert!(Arc::ptr_eq(&collections[0], &held[0]));
        assert!(!rename_environment(
            &mut collections,
            collection_id,
            Uuid::new_v4(),
            "Prod"
        ));
        assert!(!rename_environment(
            &mut collections,
            Uuid::new_v4(),
            environment_id,
            "Prod"
        ));

        assert!(rename_environment(
            &mut collections,
            collection_id,
            environment_id,
            "Prod"
        ));
        assert_eq!(
            collections[0].environment(environment_id).unwrap().name,
            "Prod"
        );
        assert_eq!(held[0].environments[0].name, NEW_ENVIRONMENT_NAME);
    }

    #[test]
    fn duplicate_environment_inserts_a_copy_after_it_with_the_same_base() {
        let mut disabled = variable("token", "t");
        disabled.enabled = false;
        let rows = vec![variable("host", "staging"), disabled];
        let (mut collections, collection_id, original) = with_environment(rows.clone());
        let base = Uuid::new_v4();
        set_environment_base(&mut collections, collection_id, original, Some(base));
        let last = add_environment(&mut collections, collection_id).unwrap();
        let held = collections.clone();

        let copy = duplicate_environment(&mut collections, collection_id, original).unwrap();
        let environments = &collections[0].environments;
        let ids: Vec<_> = environments.iter().map(|env| env.id).collect();
        assert_eq!(ids, [original, copy, last]);
        let copied = &environments[1];
        assert_eq!(copied.name, "New environment copy");
        assert_eq!(copied.variables, rows);
        assert_eq!(copied.base, Some(base));
        assert_eq!(base_counts(&collections)[&base], 2);
        assert_eq!(held[0].environments.len(), 2);

        let again = duplicate_environment(&mut collections, collection_id, original).unwrap();
        assert_eq!(
            collections[0].environment(again).unwrap().name,
            "New environment copy 2"
        );

        let held = collections.clone();
        assert!(duplicate_environment(&mut collections, collection_id, Uuid::new_v4()).is_none());
        assert!(duplicate_environment(&mut collections, Uuid::new_v4(), original).is_none());
        assert!(Arc::ptr_eq(&collections[0], &held[0]));
    }

    #[test]
    fn delete_environment_removes_only_it() {
        let (mut collections, collection_id, deleted) = with_environment(Vec::new());
        let kept = add_environment(&mut collections, collection_id).unwrap();

        assert!(delete_environment(&mut collections, collection_id, deleted));
        assert!(collections[0].environment(deleted).is_none());
        assert!(collections[0].environment(kept).is_some());

        let held = collections.clone();
        assert!(!delete_environment(
            &mut collections,
            collection_id,
            deleted
        ));
        assert!(Arc::ptr_eq(&collections[0], &held[0]));
    }

    #[test]
    fn set_environment_variables_only_when_they_change() {
        let rows = vec![variable("host", "staging")];
        let (mut collections, collection_id, environment_id) = with_environment(rows.clone());
        assert_eq!(collections[0].environments[0].variables, rows);
        let held = collections.clone();

        assert!(!set_environment_variables(
            &mut collections,
            collection_id,
            environment_id,
            rows.clone()
        ));
        assert!(Arc::ptr_eq(&collections[0], &held[0]));
        assert!(!set_environment_variables(
            &mut collections,
            collection_id,
            Uuid::new_v4(),
            Vec::new()
        ));

        assert!(set_environment_variables(
            &mut collections,
            collection_id,
            environment_id,
            Vec::new()
        ));
        assert!(collections[0].environments[0].variables.is_empty());
        assert_eq!(held[0].environments[0].variables, rows);
        assert!(collections[0].variables.is_empty());
    }
}
