use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{environment_selection::DraftSource, recents::Recents, request::Request};

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
    /// What the palette lists as recent.
    #[serde(default, skip_serializing_if = "Recents::is_empty")]
    pub recents: Recents,
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
    #[serde(default)]
    recents: Recents,
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
            recents: stored.recents,
        }
    }
}

impl SavedNavigation {
    pub fn is_valid(&self) -> bool {
        self.index < self.entries.len()
    }

    /// Whether the session part (route, back/forward history and draft) is
    /// what a fresh start has: the draft page with an empty draft.
    pub fn has_blank_session(&self) -> bool {
        self.entries == [NavEntry::Draft]
            && self.index == 0
            && self
                .draft
                .as_ref()
                .is_none_or(|draft| *draft == Request::default())
    }

    /// `self` with the route, back/forward history and draft of `session`.
    fn with_session_of(self, session: &SavedNavigation) -> Self {
        Self {
            entries: session.entries.clone(),
            index: session.index,
            draft: session.draft.clone(),
            ..self
        }
    }

    fn without_session(self) -> Self {
        Self {
            entries: vec![NavEntry::Draft],
            index: 0,
            draft: None,
            ..self
        }
    }
}

/// What startup does with the stored navigation.
#[derive(Debug, PartialEq)]
pub struct Startup {
    /// What to restore. The environment selection and recents are kept as
    /// preferences; the session part is blank when it isn't restored.
    pub restore: Option<SavedNavigation>,
    pub kept: KeptSession,
}

impl Startup {
    pub fn new(stored: Option<SavedNavigation>, restore_session: bool) -> Self {
        match stored {
            Some(stored) if !restore_session && !stored.has_blank_session() => Self {
                restore: Some(stored.clone().without_session()),
                kept: KeptSession(Some(Box::new(stored))),
            },
            restore => Self {
                restore,
                kept: KeptSession(None),
            },
        }
    }
}

/// A stored session that wasn't restored. It stays on disk until the user
/// leaves the blank start, so turning restoring back on still finds it.
#[derive(Debug, Default, PartialEq)]
pub struct KeptSession(Option<Box<SavedNavigation>>);

impl KeptSession {
    /// What to write instead of `current`.
    pub fn apply(&mut self, current: SavedNavigation) -> SavedNavigation {
        match &self.0 {
            Some(kept) if current.has_blank_session() => current.with_session_of(kept),
            _ => {
                self.0 = None;
                current
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::recents::Recent;

    fn load(json: &str) -> SavedNavigation {
        serde_json::from_str(json).unwrap()
    }

    fn session(entries: Vec<NavEntry>, draft: Option<Request>) -> SavedNavigation {
        SavedNavigation {
            index: entries.len() - 1,
            entries,
            draft,
            active_environments: HashMap::new(),
            draft_source: DraftSource::None,
            recents: Recents::default(),
        }
    }

    fn blank() -> SavedNavigation {
        session(vec![NavEntry::Draft], Some(Request::default()))
    }

    fn edited_draft() -> Request {
        Request {
            url: "https://example.com".into(),
            ..Request::default()
        }
    }

    fn with_preferences(mut saved: SavedNavigation) -> SavedNavigation {
        saved
            .active_environments
            .insert(Uuid::new_v4(), Uuid::new_v4());
        saved.draft_source = DraftSource::Shared(Uuid::new_v4());
        saved
            .recents
            .record(Recent::Collection { id: Uuid::new_v4() });
        saved
    }

    #[test]
    fn blank_session_ignores_preferences() {
        assert!(blank().has_blank_session());
        assert!(session(vec![NavEntry::Draft], None).has_blank_session());
        assert!(with_preferences(blank()).has_blank_session());
        assert!(!session(vec![NavEntry::Draft], Some(edited_draft())).has_blank_session());
        assert!(!session(vec![NavEntry::Draft, NavEntry::Settings], None).has_blank_session());
        let mut back = session(vec![NavEntry::Draft, NavEntry::Draft], None);
        back.index = 0;
        assert!(!back.has_blank_session());
    }

    #[test]
    fn startup_restores_everything_when_restoring_sessions() {
        let stored = with_preferences(session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        ));
        let startup = Startup::new(Some(stored.clone()), true);
        assert_eq!(startup.restore, Some(stored));
        assert_eq!(startup.kept, KeptSession(None));
    }

    #[test]
    fn startup_without_restoring_keeps_only_the_preferences() {
        let stored = with_preferences(session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        ));
        let startup = Startup::new(Some(stored.clone()), false);
        let restore = startup.restore.unwrap();
        assert!(restore.has_blank_session());
        assert_eq!(restore.draft, None);
        assert_eq!(restore.active_environments, stored.active_environments);
        assert_eq!(restore.draft_source, stored.draft_source);
        assert_eq!(restore.recents, stored.recents);
        assert_eq!(startup.kept, KeptSession(Some(Box::new(stored))));
    }

    #[test]
    fn startup_without_a_stored_session_keeps_nothing() {
        assert_eq!(
            Startup::new(None, false),
            Startup {
                restore: None,
                kept: KeptSession(None),
            }
        );
        let stored = with_preferences(blank());
        let startup = Startup::new(Some(stored.clone()), false);
        assert_eq!(startup.restore, Some(stored));
        assert_eq!(startup.kept, KeptSession(None));
    }

    #[test]
    fn kept_session_is_written_until_the_user_leaves_the_blank_start() {
        let stored = session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        );
        let mut kept = Startup::new(Some(stored.clone()), false).kept;

        let current = with_preferences(blank());
        let written = kept.apply(current.clone());
        assert_eq!(written.entries, stored.entries);
        assert_eq!(written.index, stored.index);
        assert_eq!(written.draft, stored.draft);
        assert_eq!(written.active_environments, current.active_environments);
        assert_eq!(written.recents, current.recents);

        let moved_on = session(
            vec![NavEntry::Draft, NavEntry::Collection { id: Uuid::new_v4() }],
            None,
        );
        assert_eq!(kept.apply(moved_on.clone()), moved_on);
        assert_eq!(kept.apply(blank()), blank());
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
            recents: Recents::default(),
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
    fn files_without_recents_load_with_none() {
        let saved = load(r#"{ "entries": [{ "kind": "draft" }], "index": 0 }"#);
        assert!(saved.recents.is_empty());
        let value = serde_json::to_value(&saved).unwrap();
        assert!(value.get("recents").is_none());
    }

    #[test]
    fn malformed_recents_keep_the_rest_of_the_file() {
        let id = Uuid::new_v4();
        for recents in ["null", r#""oops""#] {
            let saved = load(&format!(
                r#"{{ "entries": [{{ "kind": "draft" }}], "index": 0,
                     "draft_source": {{ "kind": "collection", "id": "{id}" }},
                     "recents": {recents} }}"#
            ));
            assert!(saved.recents.is_empty());
            assert_eq!(saved.draft_source, DraftSource::Collection(id));
        }
    }

    #[test]
    fn recents_round_trip() {
        let id = Uuid::new_v4();
        let saved = load(&format!(
            r#"{{ "entries": [{{ "kind": "draft" }}], "index": 0,
                 "recents": [{{ "kind": "collection", "id": "{id}" }}] }}"#
        ));
        assert_eq!(saved.recents.all(), [Recent::Collection { id }]);
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(
            serde_json::from_value::<SavedNavigation>(value).unwrap(),
            saved
        );
    }

    #[test]
    fn no_draft_source_is_not_written() {
        let saved = load(r#"{ "entries": [{ "kind": "draft" }], "index": 0 }"#);
        assert_eq!(saved.draft_source, DraftSource::None);
        let value = serde_json::to_value(&saved).unwrap();
        assert!(value.get("draft_source").is_none());
    }
}
