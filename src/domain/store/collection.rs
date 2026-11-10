use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use uuid::Uuid;

use crate::domain::collection::Collection;

/// One `<uuid>.json` file per collection in `dir`.
pub struct CollectionStore {
    dir: PathBuf,
}

impl CollectionStore {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating collections directory {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Sorted by name, case-insensitively. Unreadable files are skipped with a warning.
    pub fn load_all(&self) -> anyhow::Result<Vec<Collection>> {
        let entries = fs::read_dir(&self.dir)
            .with_context(|| format!("reading collections directory {}", self.dir.display()))?;

        let mut collections = Vec::new();
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading collections directory {}", self.dir.display()))?
                .path();
            if !path.is_file() || path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            match read_collection(&path) {
                Ok(collection) => collections.push(collection),
                Err(e) => tracing::warn!("skipping collection file {}: {e:#}", path.display()),
            }
        }

        collections.sort_by_cached_key(|collection| collection.name.to_lowercase());
        Ok(collections)
    }

    pub fn save(&self, collection: &Collection) -> anyhow::Result<()> {
        let path = self.path(collection.id);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(collection)?;

        fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))
    }

    pub fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        let path = self.path(id);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(e).with_context(|| format!("deleting {}", path.display()))
            }
            _ => Ok(()),
        }
    }

    fn path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

fn read_collection(path: &Path) -> anyhow::Result<Collection> {
    let json = fs::read(path)?;
    Ok(serde_json::from_slice(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        collection::{Folder, Item, SavedRequest},
        request::{Auth, Body, HttpMethod, Request},
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let name = format!("posty-pete-test-{}-{}", std::process::id(), Uuid::new_v4());
            Self(std::env::temp_dir().join(name))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn setup() -> (TempDir, CollectionStore) {
        let dir = TempDir::new();
        let store = CollectionStore::new(dir.0.clone()).unwrap();
        (dir, store)
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
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
        (collection, request_id)
    }

    #[test]
    fn new_creates_missing_directory() {
        let dir = TempDir::new();
        let nested = dir.0.join("a").join("collections");
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

        assert_eq!(file_names(&dir.0), [format!("{}.json", collection.id)]);
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "New");
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let (dir, store) = setup();
        store.save(&Collection::new("API")).unwrap();
        assert!(
            file_names(&dir.0)
                .iter()
                .all(|name| !name.ends_with(".tmp"))
        );
    }

    #[test]
    fn load_all_skips_invalid_and_non_json_files() {
        let (dir, store) = setup();
        let valid = Collection::new("Valid");
        store.save(&valid).unwrap();
        let broken = dir.0.join(format!("{}.json", Uuid::new_v4()));
        fs::write(&broken, "not json").unwrap();
        fs::write(dir.0.join("leftover.json.tmp"), "{}").unwrap();
        fs::write(dir.0.join("notes.txt"), "hello").unwrap();

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
        assert!(file_names(&dir.0).is_empty());
        assert!(store.load_all().unwrap().is_empty());
        store.delete(Uuid::new_v4()).unwrap();
    }
}
