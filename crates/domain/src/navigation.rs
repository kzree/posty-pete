use std::{collections::HashMap, mem, sync::Arc};

use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

use crate::{
    collection::MovedRequest, environment_selection::DraftSource, layout::Layout, recents::Recents,
    request::Request,
};

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

impl NavEntry {
    /// The entry pointing at the moved request's new collection.
    pub fn with_move(self, moved: MovedRequest) -> Self {
        match self {
            Self::SavedRequest {
                collection_id,
                request_id,
            } => Self::SavedRequest {
                collection_id: moved.collection_of(collection_id, request_id),
                request_id,
            },
            entry => entry,
        }
    }
}

/// The back/forward history and the unsaved draft, kept across restarts.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(from = "StoredNavigation")]
pub struct SavedNavigation {
    pub entries: Vec<NavEntry>,
    /// Index of the current entry in `entries`.
    pub index: usize,
    pub draft: Option<Request>,
    /// Each collection's active environment, by collection id.
    pub active_environments: HashMap<Uuid, Uuid>,
    pub draft_source: DraftSource,
    /// What the palette lists as recent.
    pub recents: Recents,
    pub layout: Layout,
}

/// How [`SavedNavigation`] is written, borrowing the session part from
/// wherever it is kept.
#[derive(Serialize)]
pub struct NavigationFile<'a> {
    entries: &'a [NavEntry],
    index: usize,
    draft: Option<&'a Request>,
    active_environments: &'a HashMap<Uuid, Uuid>,
    #[serde(skip_serializing_if = "DraftSource::is_none")]
    draft_source: &'a DraftSource,
    #[serde(skip_serializing_if = "Recents::is_empty")]
    recents: &'a Recents,
    layout: &'a Layout,
}

impl<'a> NavigationFile<'a> {
    fn new(preferences: &'a SavedNavigation, session: &'a Session) -> Self {
        Self {
            entries: &session.entries,
            index: session.index,
            draft: session.draft.as_ref(),
            ..Self::from(preferences)
        }
    }
}

impl<'a> From<&'a SavedNavigation> for NavigationFile<'a> {
    fn from(saved: &'a SavedNavigation) -> Self {
        Self {
            entries: &saved.entries,
            index: saved.index,
            draft: saved.draft.as_ref(),
            active_environments: &saved.active_environments,
            draft_source: &saved.draft_source,
            recents: &saved.recents,
            layout: &saved.layout,
        }
    }
}

impl Serialize for SavedNavigation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        NavigationFile::from(self).serialize(serializer)
    }
}

/// The route, back/forward history and draft of a [`SavedNavigation`].
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    entries: Vec<NavEntry>,
    index: usize,
    draft: Option<Request>,
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
    #[serde(default)]
    layout: Layout,
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
            layout: stored.layout,
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

    /// `self` with a blank session, and the session it had.
    fn split_session(mut self) -> (Self, Session) {
        let session = Session {
            entries: mem::replace(&mut self.entries, vec![NavEntry::Draft]),
            index: mem::take(&mut self.index),
            draft: self.draft.take(),
        };
        (self, session)
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
            Some(stored) if !restore_session && !stored.has_blank_session() => {
                let (restore, session) = stored.split_session();
                Self {
                    restore: Some(restore),
                    kept: KeptSession(Some(Arc::new(session))),
                }
            }
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
pub struct KeptSession(Option<Arc<Session>>);

impl KeptSession {
    /// What to write instead of `current`.
    pub fn apply(&mut self, current: SavedNavigation) -> NavigationToSave {
        let kept = match &self.0 {
            Some(kept) if current.has_blank_session() => Some(kept.clone()),
            _ => {
                self.0 = None;
                None
            }
        };
        NavigationToSave { current, kept }
    }

    /// Points the kept session's entries at the moved request's new collection.
    pub fn apply_move(&mut self, moved: MovedRequest) {
        let Some(session) = &mut self.0 else {
            return;
        };
        if session
            .entries
            .iter()
            .any(|entry| entry.with_move(moved) != *entry)
        {
            let session = Arc::make_mut(session);
            for entry in &mut session.entries {
                *entry = entry.with_move(moved);
            }
        }
    }
}

/// `current`, or its preferences with a kept session that it shares.
pub struct NavigationToSave {
    current: SavedNavigation,
    kept: Option<Arc<Session>>,
}

impl<'a> From<&'a NavigationToSave> for NavigationFile<'a> {
    fn from(to_save: &'a NavigationToSave) -> Self {
        match &to_save.kept {
            Some(kept) => NavigationFile::new(&to_save.current, kept),
            None => NavigationFile::from(&to_save.current),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recents::Recent;

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
            layout: Layout::default(),
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

    fn kept(stored: &SavedNavigation) -> KeptSession {
        KeptSession(Some(Arc::new(stored.clone().split_session().1)))
    }

    /// What `to_save` writes, as read back.
    fn read_back(to_save: &NavigationToSave) -> SavedNavigation {
        serde_json::from_value(serde_json::to_value(NavigationFile::from(to_save)).unwrap())
            .unwrap()
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
        assert_eq!(startup.kept, kept(&stored));
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
        let written = read_back(&kept.apply(current.clone()));
        assert_eq!(written.entries, stored.entries);
        assert_eq!(written.index, stored.index);
        assert_eq!(written.draft, stored.draft);
        assert_eq!(written.active_environments, current.active_environments);
        assert_eq!(written.recents, current.recents);

        let moved_on = session(
            vec![NavEntry::Draft, NavEntry::Collection { id: Uuid::new_v4() }],
            None,
        );
        assert_eq!(read_back(&kept.apply(moved_on.clone())), moved_on);
        assert_eq!(read_back(&kept.apply(blank())), blank());
    }

    #[test]
    fn saves_share_the_kept_session() {
        let stored = session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        );
        let mut kept = Startup::new(Some(stored), false).kept;
        let first = kept.apply(blank());
        let second = kept.apply(with_preferences(blank()));
        let shared = kept.0.as_ref().unwrap();
        assert!(Arc::ptr_eq(first.kept.as_ref().unwrap(), shared));
        assert!(Arc::ptr_eq(second.kept.as_ref().unwrap(), shared));
    }

    #[test]
    fn moves_repoint_saved_request_entries_and_the_kept_session() {
        let (from, to, request_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let saved = |collection_id, request_id| NavEntry::SavedRequest {
            collection_id,
            request_id,
        };
        let moved = MovedRequest {
            request_id,
            from,
            to,
        };
        let other = saved(from, Uuid::new_v4());
        assert_eq!(
            saved(from, request_id).with_move(moved),
            saved(to, request_id)
        );
        assert_eq!(other.with_move(moved), other);
        assert_eq!(
            NavEntry::Collection { id: from }.with_move(moved),
            NavEntry::Collection { id: from }
        );

        let stored = session(vec![saved(from, request_id), other], None);
        let mut kept = kept(&stored);
        let untouched = kept.0.clone().unwrap();
        kept.apply_move(MovedRequest {
            request_id: Uuid::new_v4(),
            ..moved
        });
        assert!(Arc::ptr_eq(kept.0.as_ref().unwrap(), &untouched));
        kept.apply_move(moved);
        let written = read_back(&kept.apply(blank()));
        assert_eq!(written.entries, [saved(to, request_id), other]);
        assert_eq!(untouched.entries, stored.entries);
    }

    #[test]
    fn kept_session_is_written_like_the_whole_navigation() {
        let stored = with_preferences(session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        ));
        let mut kept = kept(&stored);
        let mut current = blank();
        current.active_environments = stored.active_environments.clone();
        current.draft_source = stored.draft_source;
        current.recents = stored.recents.clone();
        assert_eq!(
            serde_json::to_string(&NavigationFile::from(&kept.apply(current))).unwrap(),
            serde_json::to_string(&stored).unwrap()
        );
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
            layout: Layout::default(),
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
    fn files_without_a_layout_load_with_the_default() {
        let saved = load(r#"{ "entries": [{ "kind": "draft" }], "index": 0 }"#);
        assert_eq!(saved.layout, Layout::default());
    }

    #[test]
    fn layout_round_trips() {
        let mut saved = blank();
        saved.layout = Layout {
            sidebar_width: 333.,
            sidebar_collapsed: true,
            request_split: Some(0.6),
        };
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(
            serde_json::from_value::<SavedNavigation>(value).unwrap(),
            saved
        );
    }

    #[test]
    fn layout_is_restored_and_written_without_restoring_the_session() {
        let mut stored = session(
            vec![NavEntry::Draft, NavEntry::Settings],
            Some(edited_draft()),
        );
        stored.layout.sidebar_width = 333.;
        let startup = Startup::new(Some(stored.clone()), false);
        assert_eq!(startup.restore.unwrap().layout, stored.layout);

        let mut kept = startup.kept;
        let mut current = blank();
        current.layout.request_split = Some(0.3);
        assert_eq!(
            read_back(&kept.apply(current.clone())).layout,
            current.layout
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
