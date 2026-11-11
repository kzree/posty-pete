use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{environment_selection::DraftSource, request::Request};

/// A back/forward history entry as stored on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavEntry {
    Draft,
    SavedRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
    Settings,
    Collection {
        id: Uuid,
    },
    SharedEnvironments,
}

/// The back/forward history and the unsaved draft, kept across restarts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredNavigation")]
pub struct SavedNavigation {
    pub entries: Vec<NavEntry>,
    /// Index of the current entry in `entries`.
    pub index: usize,
    #[serde(default)]
    pub draft: Option<Request>,
    /// Each collection's active environment, by collection id.
    #[serde(default)]
    pub active_environments: HashMap<Uuid, Uuid>,
    #[serde(default, skip_serializing_if = "DraftSource::is_none")]
    pub draft_source: DraftSource,
}

/// [`SavedNavigation`] as read, also accepting the `draft_collection` that
/// older versions wrote instead of `draft_source`.
#[derive(Deserialize)]
struct StoredNavigation {
    entries: Vec<NavEntry>,
    index: usize,
    #[serde(default)]
    draft: Option<Request>,
    #[serde(default)]
    active_environments: HashMap<Uuid, Uuid>,
    #[serde(default)]
    draft_source: DraftSource,
    #[serde(default)]
    draft_collection: Option<Uuid>,
}

impl From<StoredNavigation> for SavedNavigation {
    fn from(stored: StoredNavigation) -> Self {
        let draft_source = match (stored.draft_source, stored.draft_collection) {
            (DraftSource::None, Some(id)) => DraftSource::Collection(id),
            (source, _) => source,
        };
        Self {
            entries: stored.entries,
            index: stored.index,
            draft: stored.draft,
            active_environments: stored.active_environments,
            draft_source,
        }
    }
}

impl SavedNavigation {
    pub fn is_valid(&self) -> bool {
        self.index < self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(json: &str) -> SavedNavigation {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn old_draft_collection_loads_as_a_collection_source() {
        let id = Uuid::new_v4();
        let saved = load(&format!(
            r#"{{ "entries": [{{ "kind": "draft" }}], "index": 0, "draft_collection": "{id}" }}"#
        ));
        assert_eq!(saved.draft_source, DraftSource::Collection(id));
    }

    #[test]
    fn draft_source_round_trips_without_the_old_field() {
        let saved = SavedNavigation {
            entries: vec![NavEntry::Draft],
            index: 0,
            draft: None,
            active_environments: HashMap::new(),
            draft_source: DraftSource::Shared(Uuid::new_v4()),
        };
        let value = serde_json::to_value(&saved).unwrap();
        assert!(value.get("draft_collection").is_none());
        assert_eq!(
            serde_json::from_value::<SavedNavigation>(value).unwrap(),
            saved
        );
    }

    #[test]
    fn shared_environments_entry_round_trips() {
        let saved = load(
            r#"{ "entries": [{ "kind": "draft" }, { "kind": "shared_environments" }], "index": 1 }"#,
        );
        assert_eq!(saved.entries[1], NavEntry::SharedEnvironments);
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value["entries"][1]["kind"], "shared_environments");
    }

    #[test]
    fn no_draft_source_is_not_written() {
        let saved = load(r#"{ "entries": [{ "kind": "draft" }], "index": 0 }"#);
        assert_eq!(saved.draft_source, DraftSource::None);
        let value = serde_json::to_value(&saved).unwrap();
        assert!(value.get("draft_source").is_none());
    }
}
