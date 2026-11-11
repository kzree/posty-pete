use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::request::Request;

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
}

/// The back/forward history and the unsaved draft, kept across restarts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedNavigation {
    pub entries: Vec<NavEntry>,
    /// Index of the current entry in `entries`.
    pub index: usize,
    #[serde(default)]
    pub draft: Option<Request>,
    /// Each collection's active environment, by collection id.
    #[serde(default)]
    pub active_environments: HashMap<Uuid, Uuid>,
    #[serde(default)]
    pub draft_collection: Option<Uuid>,
}

impl SavedNavigation {
    pub fn is_valid(&self) -> bool {
        self.index < self.entries.len()
    }
}
