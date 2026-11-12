use std::path::PathBuf;

use uuid::Uuid;

use super::json_dir::JsonDir;
use crate::collection::Collection;

/// One `<uuid>.json` file per collection in `dir`.
pub struct CollectionStore {
    dir: JsonDir,
}

impl CollectionStore {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            dir: JsonDir::new(dir)?,
        })
    }

    /// Sorted by name, case-insensitively. Unreadable files are skipped with a warning.
    pub fn load_all(&self) -> anyhow::Result<Vec<Collection>> {
        let mut collections: Vec<Collection> = self.dir.load_all()?;
        collections.sort_by_cached_key(|collection| collection.name.to_lowercase());
        Ok(collections)
    }

    pub fn save(&self, collection: &Collection) -> anyhow::Result<()> {
        self.dir.save(collection.id, collection)
    }

    pub fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        self.dir.delete(id)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::store::test_support::{TempDir, file_names};
    use crate::{
        collection::{Folder, Item, SavedRequest},
        environment::Environment,
        request::{Auth, Body, HttpMethod, KeyValue, Request},
    };

    fn setup() -> (TempDir, CollectionStore) {
        let dir = TempDir::new();
        let store = CollectionStore::new(dir.path().to_path_buf()).unwrap();
        (dir, store)
    }

    fn nested_collection(name: &str) -> (Collection, Uuid) {
        let mut collection = Collection::new(name);
        let folder = Folder::new("Folder");
        let folder_id = folder.id;
        let request = SavedRequest::new(
            "Deep",
            Request {
                method: HttpMethod::Put,
                url: "example.com/deep".into(),
                params: Vec::new(),
                headers: Vec::new(),
                body: Body::None,
                auth: Auth::None,
            },
        );
        let request_id = request.id;
        assert!(collection.add_item(None, Item::Folder(folder)));
        assert!(collection.add_item(Some(folder_id), Item::Request(request)));
        let mut environment = Environment::new("Staging");
        environment.variables.push(KeyValue {
            enabled: true,
            key: "host".into(),
            value: "staging.example.com".into(),
        });
        collection.environments.push(environment);
        (collection, request_id)
    }

    #[test]
    fn new_creates_missing_directory() {
        let dir = TempDir::new();
        let nested = dir.path().join("a").join("collections");
        CollectionStore::new(nested.clone()).unwrap();
        assert!(nested.is_dir());
    }

    #[test]
    fn save_then_load_keeps_nested_items() {
        let (_dir, store) = setup();
        let (collection, request_id) = nested_collection("API");
        store.save(&collection).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, collection.id);
        assert_eq!(loaded[0].name, "API");
        let deep = loaded[0].find_request(request_id).unwrap();
        assert_eq!(deep.request.url, "example.com/deep");
        assert_eq!(loaded[0].environments, collection.environments);
        assert_eq!(
            serde_json::to_value(&loaded[0]).unwrap(),
            serde_json::to_value(&collection).unwrap()
        );
    }

    #[test]
    fn save_twice_overwrites_one_file() {
        let (dir, store) = setup();
        let mut collection = Collection::new("Old");
        store.save(&collection).unwrap();
        collection.name = "New".into();
        store.save(&collection).unwrap();

        assert_eq!(file_names(dir.path()), [format!("{}.json", collection.id)]);
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "New");
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let (dir, store) = setup();
        store.save(&Collection::new("API")).unwrap();
        assert!(
            file_names(dir.path())
                .iter()
                .all(|name| !name.ends_with(".tmp"))
        );
    }

    #[test]
    fn load_all_skips_invalid_and_non_json_files() {
        let (dir, store) = setup();
        let valid = Collection::new("Valid");
        store.save(&valid).unwrap();
        let broken = dir.path().join(format!("{}.json", Uuid::new_v4()));
        fs::write(&broken, "not json").unwrap();
        fs::write(dir.path().join("leftover.json.tmp"), "{}").unwrap();
        fs::write(dir.path().join("notes.txt"), "hello").unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, valid.id);
        assert!(broken.exists());
    }

    #[test]
    fn load_all_sorts_by_name_case_insensitively() {
        let (_dir, store) = setup();
        for name in ["charlie", "Bravo", "alpha"] {
            store.save(&Collection::new(name)).unwrap();
        }
        let names: Vec<_> = store
            .load_all()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["alpha", "Bravo", "charlie"]);
    }

    #[test]
    fn delete_removes_file_and_ignores_unknown_id() {
        let (dir, store) = setup();
        let collection = Collection::new("API");
        store.save(&collection).unwrap();

        store.delete(collection.id).unwrap();
        assert!(file_names(dir.path()).is_empty());
        assert!(store.load_all().unwrap().is_empty());
        store.delete(Uuid::new_v4()).unwrap();
    }
}
