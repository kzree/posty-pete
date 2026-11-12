use std::{fs, io, path::PathBuf};

use anyhow::{Context as _, bail};

use crate::navigation::SavedNavigation;

/// The navigation state in a single JSON file.
pub struct NavigationStore {
    path: PathBuf,
}

impl NavigationStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `Ok(None)` if there is no file yet.
    pub fn load(&self) -> anyhow::Result<Option<SavedNavigation>> {
        let json = match fs::read(&self.path) {
            Ok(json) => json,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        let saved: SavedNavigation = serde_json::from_slice(&json)
            .with_context(|| format!("parsing {}", self.path.display()))?;
        if !saved.is_valid() {
            bail!("{}: current entry is out of range", self.path.display());
        }
        Ok(Some(saved))
    }

    pub fn save(&self, saved: &SavedNavigation) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        super::write_json_atomically(&self.path, saved)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use uuid::Uuid;

    use super::*;
    use crate::store::test_support::TempDir;
    use crate::{
        environment_selection::DraftSource,
        layout::Layout,
        navigation::NavEntry,
        recents::{Recent, Recents},
        request::{HttpMethod, Request},
    };

    fn setup() -> (TempDir, NavigationStore) {
        let dir = TempDir::new();
        let store = NavigationStore::new(dir.path().join("navigation.json"));
        (dir, store)
    }

    fn sample() -> SavedNavigation {
        SavedNavigation {
            entries: vec![
                NavEntry::Draft,
                NavEntry::SavedRequest {
                    collection_id: Uuid::new_v4(),
                    request_id: Uuid::new_v4(),
                },
                NavEntry::Collection { id: Uuid::new_v4() },
                NavEntry::Settings,
            ],
            index: 2,
            draft: Some(Request {
                method: HttpMethod::Post,
                url: "example.com/draft".into(),
                ..Request::default()
            }),
            active_environments: HashMap::from([(Uuid::new_v4(), Uuid::new_v4())]),
            draft_source: DraftSource::Collection(Uuid::new_v4()),
            recents: Recents::new([Recent::Collection { id: Uuid::new_v4() }]),
            layout: Layout {
                sidebar_width: 320.,
                sidebar_collapsed: true,
                request_split: Some(0.4),
            },
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let (_dir, store) = setup();
        let saved = sample();
        store.save(&saved).unwrap();
        assert_eq!(store.load().unwrap(), Some(saved));
    }

    #[test]
    fn save_overwrites_and_leaves_no_tmp_file() {
        let (dir, store) = setup();
        store.save(&sample()).unwrap();
        let mut newer = sample();
        newer.draft = None;
        store.save(&newer).unwrap();

        assert_eq!(store.load().unwrap(), Some(newer));
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["navigation.json"]);
    }

    #[test]
    fn missing_file_loads_as_none() {
        let (_dir, store) = setup();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let (dir, store) = setup();
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(dir.path().join("navigation.json"), "{ not json").unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn out_of_range_index_is_an_error() {
        let (_dir, store) = setup();
        let mut saved = sample();
        saved.index = saved.entries.len();
        store.save(&saved).unwrap();
        assert!(store.load().is_err());

        store
            .save(&SavedNavigation {
                entries: Vec::new(),
                index: 0,
                ..sample()
            })
            .unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn missing_optional_fields_load_as_empty() {
        let (dir, store) = setup();
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.path().join("navigation.json"),
            r#"{ "entries": [{ "kind": "draft" }, { "kind": "settings" }], "index": 1 }"#,
        )
        .unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.entries, [NavEntry::Draft, NavEntry::Settings]);
        assert_eq!(loaded.draft, None);
        assert!(loaded.active_environments.is_empty());
        assert_eq!(loaded.draft_source, DraftSource::None);
        assert!(loaded.recents.is_empty());
    }
}
