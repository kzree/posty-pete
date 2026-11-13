//! Recently opened items and recently run commands, newest first.

use std::{collections::HashSet, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use super::{
    collection::{Collection, MovedRequest},
    command::Command,
    environment::Environment,
    settings_index::SettingsSection,
};

/// How many items, and separately how many commands, are kept.
pub const CAP: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recent {
    Request {
        collection_id: Uuid,
        request_id: Uuid,
    },
    Collection {
        id: Uuid,
    },
    Folder {
        collection_id: Uuid,
        folder_id: Uuid,
    },
    SharedEnvironment {
        id: Uuid,
    },
    CollectionEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
    },
    SettingsPage {
        section: SettingsSection,
    },
    Command {
        command: Command,
    },
}

impl Recent {
    pub fn is_command(self) -> bool {
        matches!(self, Self::Command { .. })
    }

    /// Whether it's still there. Shared environments count as there while
    /// `shared` isn't loaded.
    fn exists(self, collections: &[Arc<Collection>], shared: Option<&[Arc<Environment>]>) -> bool {
        let collection = |id: Uuid| collections.iter().find(|collection| collection.id == id);
        match self {
            Self::Request {
                collection_id,
                request_id,
            } => collection(collection_id)
                .is_some_and(|collection| collection.find_request(request_id).is_some()),
            Self::Collection { id } => collection(id).is_some(),
            Self::Folder {
                collection_id,
                folder_id,
            } => collection(collection_id)
                .is_some_and(|collection| collection.find_folder(folder_id).is_some()),
            Self::CollectionEnvironment {
                collection_id,
                environment_id,
            } => collection(collection_id).is_some_and(|collection| {
                collection
                    .environments
                    .iter()
                    .any(|environment| environment.id == environment_id)
            }),
            Self::SharedEnvironment { id } => {
                shared.is_none_or(|shared| shared.iter().any(|environment| environment.id == id))
            }
            Self::SettingsPage { .. } | Self::Command { .. } => true,
        }
    }
}

/// Newest first, without repeats, at most [`CAP`] items and [`CAP`] commands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Recents(Vec<Recent>);

impl Recents {
    /// From `recents` newest first; repeats keep the newest, past the caps is dropped.
    pub fn new(recents: impl IntoIterator<Item = Recent>) -> Self {
        let mut seen = HashSet::new();
        let (mut items, mut commands) = (0, 0);
        let kept = recents
            .into_iter()
            .filter(|recent| seen.insert(*recent))
            .filter(|recent| {
                let count = if recent.is_command() {
                    &mut commands
                } else {
                    &mut items
                };
                *count += 1;
                *count <= CAP
            })
            .collect();
        Self(kept)
    }

    pub fn all(&self) -> &[Recent] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Makes `recent` the newest. Returns whether anything changed.
    pub fn record(&mut self, recent: Recent) -> bool {
        if self.0.first() == Some(&recent) {
            return false;
        }
        let older = std::mem::take(&mut self.0);
        *self = Self::new(std::iter::once(recent).chain(older));
        true
    }

    /// Adds `older` after the ones already here.
    pub fn merge_older(&mut self, older: Recents) {
        let newer = std::mem::take(&mut self.0);
        *self = Self::new(newer.into_iter().chain(older.0));
    }

    /// Points the moved request's entry at its new collection. Returns whether
    /// anything changed.
    pub fn apply_move(&mut self, moved: MovedRequest) -> bool {
        let mut changed = false;
        for recent in &mut self.0 {
            if let Recent::Request {
                collection_id,
                request_id,
            } = recent
            {
                let now = moved.collection_of(*collection_id, *request_id);
                changed |= now != *collection_id;
                *collection_id = now;
            }
        }
        changed
    }

    /// Drops what no longer exists. Returns whether anything was dropped.
    pub fn prune(
        &mut self,
        collections: &[Arc<Collection>],
        shared: Option<&[Arc<Environment>]>,
    ) -> bool {
        let before = self.0.len();
        self.0.retain(|recent| recent.exists(collections, shared));
        self.0.len() != before
    }
}

/// Entries this version doesn't know, like a removed command, are skipped,
/// and anything but a list reads as empty, rather than failing the whole file.
impl<'de> Deserialize<'de> for Recents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let serde_json::Value::Array(values) = serde_json::Value::deserialize(deserializer)? else {
            return Ok(Self::default());
        };
        Ok(Self::new(
            values
                .into_iter()
                .filter_map(|value| serde_json::from_value(value).ok()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collection::{Folder, Item, SavedRequest},
        request::Request,
    };

    fn collection() -> Recent {
        Recent::Collection { id: Uuid::new_v4() }
    }

    fn command(command: Command) -> Recent {
        Recent::Command { command }
    }

    #[test]
    fn record_puts_the_newest_first() {
        let (a, b) = (collection(), collection());
        let mut recents = Recents::default();
        assert!(recents.record(a));
        assert!(recents.record(b));
        assert_eq!(recents.all(), [b, a]);
    }

    #[test]
    fn recording_again_moves_to_the_front_without_a_repeat() {
        let (a, b, c) = (collection(), collection(), collection());
        let mut recents = Recents::new([c, b, a]);
        assert!(recents.record(a));
        assert_eq!(recents.all(), [a, c, b]);
        assert!(!recents.record(a));
        assert_eq!(recents.all(), [a, c, b]);
    }

    #[test]
    fn items_and_commands_are_capped_separately() {
        let mut recents = Recents::default();
        recents.record(command(Command::Save));
        let items: Vec<Recent> = (0..CAP + 5).map(|_| collection()).collect();
        for item in &items {
            recents.record(*item);
        }
        let kept: Vec<Recent> = items.iter().rev().take(CAP).copied().collect();
        let mut expected = kept.clone();
        expected.push(command(Command::Save));
        assert_eq!(recents.all(), expected);

        for command in Command::ALL {
            recents.record(Recent::Command { command });
        }
        let commands = recents.all().iter().filter(|r| r.is_command()).count();
        assert_eq!(commands, CAP.min(Command::ALL.len()));
        assert_eq!(recents.all().len() - commands, CAP);
    }

    #[test]
    fn apply_move_repoints_only_the_moved_request() {
        let (from, to, request_id, other_request) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let request = |collection_id, request_id| Recent::Request {
            collection_id,
            request_id,
        };
        let folder = Recent::Folder {
            collection_id: from,
            folder_id: request_id,
        };
        let mut recents = Recents::new([
            request(from, other_request),
            request(from, request_id),
            folder,
            Recent::Collection { id: from },
        ]);
        let moved = MovedRequest {
            request_id,
            from,
            to,
        };

        assert!(recents.apply_move(moved));
        assert_eq!(
            recents.all(),
            [
                request(from, other_request),
                request(to, request_id),
                folder,
                Recent::Collection { id: from },
            ]
        );
        assert!(!recents.apply_move(moved));
    }

    #[test]
    fn new_keeps_the_first_of_repeats() {
        let (a, b) = (collection(), collection());
        assert_eq!(Recents::new([a, b, a, b]).all(), [a, b]);
    }

    #[test]
    fn merge_older_goes_after_the_newer_ones() {
        let (a, b, c) = (collection(), collection(), collection());
        let mut recents = Recents::new([a, b]);
        recents.merge_older(Recents::new([b, c]));
        assert_eq!(recents.all(), [a, b, c]);
    }

    #[test]
    fn prune_drops_deleted_items_and_keeps_the_rest() {
        let mut folder = Folder::new("Users");
        let kept_request = SavedRequest::new("Get", Request::default());
        folder.items = vec![Item::Request(kept_request.clone())];
        let mut api = Collection::new("API");
        api.items = vec![Item::Folder(folder.clone())];
        api.environments = vec![Environment::new("Staging")];
        let shared = vec![Arc::new(Environment::new("Local"))];
        let collections = vec![Arc::new(api.clone())];

        let existing = [
            Recent::Collection { id: api.id },
            Recent::Folder {
                collection_id: api.id,
                folder_id: folder.id,
            },
            Recent::Request {
                collection_id: api.id,
                request_id: kept_request.id,
            },
            Recent::CollectionEnvironment {
                collection_id: api.id,
                environment_id: api.environments[0].id,
            },
            Recent::SharedEnvironment { id: shared[0].id },
            Recent::SettingsPage {
                section: SettingsSection::Appearance,
            },
            command(Command::NewRequest),
        ];
        let deleted = [
            collection(),
            Recent::Request {
                collection_id: api.id,
                request_id: Uuid::new_v4(),
            },
            Recent::Folder {
                collection_id: api.id,
                folder_id: Uuid::new_v4(),
            },
            Recent::CollectionEnvironment {
                collection_id: api.id,
                environment_id: Uuid::new_v4(),
            },
            Recent::SharedEnvironment { id: Uuid::new_v4() },
        ];
        let mut recents = Recents::new(deleted.into_iter().chain(existing));
        assert!(recents.prune(&collections, Some(&shared)));
        assert_eq!(recents.all(), existing);
        assert!(!recents.prune(&collections, Some(&shared)));
    }

    #[test]
    fn shared_environments_are_kept_until_they_load() {
        let recent = Recent::SharedEnvironment { id: Uuid::new_v4() };
        let mut recents = Recents::new([recent]);
        assert!(!recents.prune(&[], None));
        assert!(recents.prune(&[], Some(&[])));
        assert!(recents.is_empty());
    }

    #[test]
    fn round_trips_through_json() {
        let recents = Recents::new([
            Recent::Request {
                collection_id: Uuid::new_v4(),
                request_id: Uuid::new_v4(),
            },
            Recent::SettingsPage {
                section: SettingsSection::HistoryAndData,
            },
            command(Command::SaveAs),
        ]);
        let value = serde_json::to_value(&recents).unwrap();
        assert_eq!(value[1]["kind"], "settings_page");
        assert_eq!(value[1]["section"], "history_and_data");
        assert_eq!(value[2]["command"], "save_as");
        assert_eq!(serde_json::from_value::<Recents>(value).unwrap(), recents);
    }

    #[test]
    fn unknown_entries_are_skipped() {
        let id = Uuid::new_v4();
        let recents: Recents = serde_json::from_str(&format!(
            r#"[
                {{ "kind": "command", "command": "teleport" }},
                {{ "kind": "something_new" }},
                {{ "kind": "collection", "id": "{id}" }}
            ]"#
        ))
        .unwrap();
        assert_eq!(recents.all(), [Recent::Collection { id }]);
    }

    #[test]
    fn anything_but_a_list_loads_as_empty() {
        for json in ["null", r#""recent""#, "{}", "3"] {
            let recents: Recents = serde_json::from_str(json).unwrap();
            assert!(recents.is_empty(), "{json}");
        }
    }

    #[test]
    fn loading_dedupes_and_caps() {
        let ids: Vec<Uuid> = (0..CAP + 3).map(|_| Uuid::new_v4()).collect();
        let mut json: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| serde_json::json!({ "kind": "collection", "id": id }))
            .collect();
        json.insert(1, json[0].clone());
        let recents: Recents = serde_json::from_value(json.into()).unwrap();
        let expected: Vec<Recent> = ids[..CAP]
            .iter()
            .map(|&id| Recent::Collection { id })
            .collect();
        assert_eq!(recents.all(), expected);
    }
}
