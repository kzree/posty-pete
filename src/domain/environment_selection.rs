use std::{collections::HashMap, sync::Arc};

use uuid::Uuid;

use super::{collection::Collection, variables::Variables};

/// Each collection's active environment, and the collection drafts resolve
/// against. Ids of deleted collections or environments count as none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentSelection {
    active: HashMap<Uuid, Uuid>,
    draft_collection: Option<Uuid>,
}

impl EnvironmentSelection {
    pub fn new(active: HashMap<Uuid, Uuid>, draft_collection: Option<Uuid>) -> Self {
        Self {
            active,
            draft_collection,
        }
    }

    /// `collection`'s active environment, if it still has it.
    pub fn active_environment(&self, collection: &Collection) -> Option<Uuid> {
        self.active
            .get(&collection.id)
            .copied()
            .filter(|id| collection.environment(*id).is_some())
    }

    /// `None` clears it.
    pub fn set_active_environment(&mut self, collection_id: Uuid, environment_id: Option<Uuid>) {
        match environment_id {
            Some(id) => self.active.insert(collection_id, id),
            None => self.active.remove(&collection_id),
        };
    }

    /// The draft collection, if it still exists.
    pub fn draft_collection<'a>(
        &self,
        collections: &'a [Arc<Collection>],
    ) -> Option<&'a Arc<Collection>> {
        let id = self.draft_collection?;
        collections.iter().find(|collection| collection.id == id)
    }

    /// Resolves drafts against `collection_id` (`None` = no variables) and
    /// makes `environment_id` its active environment.
    pub fn set_draft_collection(
        &mut self,
        collection_id: Option<Uuid>,
        environment_id: Option<Uuid>,
    ) {
        self.draft_collection = collection_id;
        if let Some(collection_id) = collection_id {
            self.set_active_environment(collection_id, environment_id);
        }
    }

    /// `collection`'s variables with its active environment in front.
    pub fn variables(&self, collection: &Collection) -> Variables {
        collection.variables_with(self.active_environment(collection))
    }

    /// This selection without the ids `collections` no longer have, as
    /// (active environments, draft collection) for saving.
    pub fn pruned(&self, collections: &[Arc<Collection>]) -> (HashMap<Uuid, Uuid>, Option<Uuid>) {
        let active = collections
            .iter()
            .filter_map(|collection| {
                let environment = self.active_environment(collection)?;
                Some((collection.id, environment))
            })
            .collect();
        let draft = self.draft_collection(collections).map(|c| c.id);
        (active, draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        collection::{Environment, add_environment},
        request::KeyValue,
    };

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
    fn variables_use_the_active_environment() {
        let (collection, staging) = collection();
        let mut selection = EnvironmentSelection::default();
        assert_eq!(selection.variables(&collection).get("host"), Some("local"));

        selection.set_active_environment(collection.id, Some(staging));
        assert_eq!(selection.active_environment(&collection), Some(staging));
        assert_eq!(
            selection.variables(&collection).get("host"),
            Some("staging")
        );

        selection.set_active_environment(collection.id, None);
        assert_eq!(selection.active_environment(&collection), None);
        assert_eq!(selection.variables(&collection).get("host"), Some("local"));
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
        assert_eq!(selection.variables(&collection).get("host"), Some("local"));
        assert!(selection.draft_collection(&collections).is_none());
        assert_eq!(selection.pruned(&collections), (HashMap::new(), None));

        selection.set_draft_collection(Some(collection.id), Some(staging));
        let expected = HashMap::from([(collection.id, staging)]);
        assert_eq!(
            selection.pruned(&collections),
            (expected, Some(collection.id))
        );
    }
}
