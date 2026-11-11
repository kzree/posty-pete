use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    collection::Collection,
    environment::{self, Environment},
    variables::Variables,
};

/// What drafts resolve against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum DraftSource {
    /// No variables.
    #[default]
    None,
    /// A collection, with its active environment.
    Collection(Uuid),
    /// A shared environment's variables alone.
    Shared(Uuid),
}

impl DraftSource {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }
}

/// Each collection's active environment, and what drafts resolve against.
/// Ids of deleted collections or environments count as none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentSelection {
    active: HashMap<Uuid, Uuid>,
    draft: DraftSource,
}

impl EnvironmentSelection {
    pub fn new(active: HashMap<Uuid, Uuid>, draft: DraftSource) -> Self {
        Self { active, draft }
    }

    /// `collection`'s active environment, if it still has it.
    pub fn active_environment(&self, collection: &Collection) -> Option<Uuid> {
        self.active
            .get(&collection.id)
            .copied()
            .filter(|id| collection.environment(*id).is_some())
    }

    /// The stored id, whether or not the environment still exists.
    pub fn active_environment_id(&self, collection_id: Uuid) -> Option<Uuid> {
        self.active.get(&collection_id).copied()
    }

    /// `None` clears it.
    pub fn set_active_environment(&mut self, collection_id: Uuid, environment_id: Option<Uuid>) {
        match environment_id {
            Some(id) => self.active.insert(collection_id, id),
            None => self.active.remove(&collection_id),
        };
    }

    /// The draft collection, if drafts use one and it still exists.
    pub fn draft_collection<'a>(
        &self,
        collections: &'a [Arc<Collection>],
    ) -> Option<&'a Arc<Collection>> {
        let DraftSource::Collection(id) = self.draft else {
            return None;
        };
        collections.iter().find(|collection| collection.id == id)
    }

    /// The shared environment drafts use, if they use one and it still exists.
    pub fn draft_shared_environment<'a>(
        &self,
        shared: &'a [Arc<Environment>],
    ) -> Option<&'a Environment> {
        let DraftSource::Shared(id) = self.draft else {
            return None;
        };
        environment::find(shared, id)
    }

    /// Resolves drafts against `collection_id` (`None` = no variables) and
    /// makes `environment_id` its active environment.
    pub fn set_draft_collection(
        &mut self,
        collection_id: Option<Uuid>,
        environment_id: Option<Uuid>,
    ) {
        self.draft = collection_id.map_or(DraftSource::None, DraftSource::Collection);
        if let Some(collection_id) = collection_id {
            self.set_active_environment(collection_id, environment_id);
        }
    }

    /// Resolves drafts against shared environment `id` alone.
    pub fn set_draft_shared_environment(&mut self, id: Uuid) {
        self.draft = DraftSource::Shared(id);
    }

    /// The draft's variables; `None` when it has nothing to resolve against.
    pub fn draft_variables(
        &self,
        collections: &[Arc<Collection>],
        shared: &[Arc<Environment>],
    ) -> Option<Variables> {
        match self.draft {
            DraftSource::None => None,
            DraftSource::Collection(_) => self
                .draft_collection(collections)
                .map(|collection| self.variables(collection, shared)),
            DraftSource::Shared(id) => environment::shared_variables(shared, id),
        }
    }

    /// `collection`'s variables with its active environment, and the shared
    /// environment that's based on, in front.
    pub fn variables(&self, collection: &Collection, shared: &[Arc<Environment>]) -> Variables {
        collection.variables_with(self.active_environment(collection), shared)
    }

    /// This selection without the ids that no longer exist, as (active
    /// environments, draft source) for saving. `shared` is `None` while the
    /// shared environments aren't loaded, which keeps a shared draft source.
    pub fn pruned(
        &self,
        collections: &[Arc<Collection>],
        shared: Option<&[Arc<Environment>]>,
    ) -> (HashMap<Uuid, Uuid>, DraftSource) {
        let active = collections
            .iter()
            .filter_map(|collection| {
                let environment = self.active_environment(collection)?;
                Some((collection.id, environment))
            })
            .collect();
        let exists = match (self.draft, shared) {
            (DraftSource::None, _) => false,
            (DraftSource::Collection(_), _) => self.draft_collection(collections).is_some(),
            (DraftSource::Shared(_), Some(shared)) => {
                self.draft_shared_environment(shared).is_some()
            }
            (DraftSource::Shared(_), None) => true,
        };
        let draft = if exists {
            self.draft
        } else {
            DraftSource::None
        };
        (active, draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{collection::add_environment, request::KeyValue};

    fn variable(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    /// A collection with `host = local` and a Staging environment with `host = staging`.
    fn collection() -> (Arc<Collection>, Uuid) {
        let mut collection = Collection::new("API");
        collection.variables = vec![variable("host", "local")];
        let mut collections = [Arc::new(collection)];
        let id = collections[0].id;
        let environment_id = add_environment(&mut collections, id).unwrap();
        let [mut collection] = collections;
        let environment: &mut Environment = Arc::make_mut(&mut collection)
            .environments
            .iter_mut()
            .find(|env| env.id == environment_id)
            .unwrap();
        environment.variables = vec![variable("host", "staging")];
        (collection, environment_id)
    }

    #[test]
    fn active_environment_id_keeps_stale_ids() {
        let (collection, staging) = collection();
        let mut selection = EnvironmentSelection::default();
        assert_eq!(selection.active_environment_id(collection.id), None);
        let gone = Uuid::new_v4();
        selection.set_active_environment(collection.id, Some(gone));
        assert_eq!(selection.active_environment_id(collection.id), Some(gone));
        selection.set_active_environment(collection.id, Some(staging));
        assert_eq!(
            selection.active_environment_id(collection.id),
            Some(staging)
        );
    }

    #[test]
    fn variables_use_the_active_environment() {
        let (collection, staging) = collection();
        let mut selection = EnvironmentSelection::default();
        assert_eq!(
            selection.variables(&collection, &[]).get("host"),
            Some("local")
        );

        selection.set_active_environment(collection.id, Some(staging));
        assert_eq!(selection.active_environment(&collection), Some(staging));
        assert_eq!(
            selection.variables(&collection, &[]).get("host"),
            Some("staging")
        );

        selection.set_active_environment(collection.id, None);
        assert_eq!(selection.active_environment(&collection), None);
        assert_eq!(
            selection.variables(&collection, &[]).get("host"),
            Some("local")
        );
    }

    #[test]
    fn setting_the_draft_collection_activates_its_environment() {
        let (collection, staging) = collection();
        let collections = [collection.clone()];
        let mut selection = EnvironmentSelection::default();
        assert!(selection.draft_collection(&collections).is_none());

        selection.set_draft_collection(Some(collection.id), Some(staging));
        assert_eq!(
            selection.draft_collection(&collections).map(|c| c.id),
            Some(collection.id)
        );
        assert_eq!(selection.active_environment(&collection), Some(staging));

        selection.set_draft_collection(None, None);
        assert!(selection.draft_collection(&collections).is_none());
        assert_eq!(selection.active_environment(&collection), Some(staging));
    }

    #[test]
    fn stale_ids_count_as_none_and_are_pruned() {
        let (collection, staging) = collection();
        let mut selection = EnvironmentSelection::default();
        selection.set_active_environment(collection.id, Some(Uuid::new_v4()));
        selection.set_active_environment(Uuid::new_v4(), Some(staging));
        selection.set_draft_collection(Some(Uuid::new_v4()), None);
        let collections = [collection.clone()];

        assert_eq!(selection.active_environment(&collection), None);
        assert_eq!(
            selection.variables(&collection, &[]).get("host"),
            Some("local")
        );
        assert!(selection.draft_collection(&collections).is_none());
        assert_eq!(
            selection.pruned(&collections, Some(&[])),
            (HashMap::new(), DraftSource::None)
        );

        selection.set_draft_collection(Some(collection.id), Some(staging));
        let expected = HashMap::from([(collection.id, staging)]);
        assert_eq!(
            selection.pruned(&collections, Some(&[])),
            (expected, DraftSource::Collection(collection.id))
        );
    }

    fn shared(name: &str, value: &str) -> Arc<Environment> {
        let mut environment = Environment::new(name);
        environment.variables = vec![variable("host", value)];
        Arc::new(environment)
    }

    #[test]
    fn draft_variables_follow_the_draft_source() {
        let (collection, staging) = collection();
        let collections = [collection.clone()];
        let dev = shared("Dev", "dev");
        let shared = [dev.clone()];
        let mut selection = EnvironmentSelection::default();
        assert_eq!(selection.draft_variables(&collections, &shared), None);

        selection.set_draft_collection(Some(collection.id), Some(staging));
        let variables = selection.draft_variables(&collections, &shared).unwrap();
        assert_eq!(variables.get("host"), Some("staging"));

        selection.set_draft_shared_environment(dev.id);
        assert!(selection.draft_collection(&collections).is_none());
        assert_eq!(
            selection.draft_shared_environment(&shared).map(|e| e.id),
            Some(dev.id)
        );
        let variables = selection.draft_variables(&collections, &shared).unwrap();
        assert_eq!(variables.get("host"), Some("dev"));
        // The collection keeps its active environment.
        assert_eq!(selection.active_environment(&collection), Some(staging));

        selection.set_draft_collection(None, None);
        assert_eq!(selection.draft_variables(&collections, &shared), None);
    }

    #[test]
    fn stale_shared_draft_source_counts_as_none_and_is_pruned_once_loaded() {
        let dev = shared("Dev", "dev");
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_shared_environment(dev.id);

        assert_eq!(selection.draft_variables(&[], &[]), None);
        assert!(selection.draft_shared_environment(&[]).is_none());
        assert_eq!(selection.pruned(&[], Some(&[])).1, DraftSource::None);
        assert_eq!(selection.pruned(&[], None).1, DraftSource::Shared(dev.id));
        assert_eq!(
            selection.pruned(&[], Some(std::slice::from_ref(&dev))).1,
            DraftSource::Shared(dev.id)
        );
    }

    #[test]
    fn draft_source_json_layout() {
        let id = Uuid::new_v4();
        let cases = [
            (DraftSource::None, serde_json::json!({ "kind": "none" })),
            (
                DraftSource::Collection(id),
                serde_json::json!({ "kind": "collection", "id": id }),
            ),
            (
                DraftSource::Shared(id),
                serde_json::json!({ "kind": "shared", "id": id }),
            ),
        ];
        for (source, json) in cases {
            assert_eq!(serde_json::to_value(source).unwrap(), json);
            assert_eq!(serde_json::from_value::<DraftSource>(json).unwrap(), source);
        }
    }
}
