//! Which variables an auth value set on a collection or environment can
//! use. A layer is used in several combinations, so a name counts as defined
//! if any of them defines it.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use uuid::Uuid;

use super::{
    collection::Collection,
    environment::{self, Environment},
    request::{Auth, KeyValue},
    variables::{collect_undefined, defined_key},
};

/// The variable names defined somewhere a layer's auth resolves. Names
/// only: values aren't needed to tell what's defined.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefinedNames(HashSet<String>);

impl DefinedNames {
    fn from_sources<'a>(sources: impl IntoIterator<Item = &'a [KeyValue]>) -> Self {
        let mut names = HashSet::new();
        for key in sources.into_iter().flatten().filter_map(defined_key) {
            if !names.contains(key) {
                names.insert(key.to_string());
            }
        }
        Self(names)
    }

    /// The collection, or any of its environments and their bases.
    pub fn collection(collection: &Collection, shared: &[Arc<Environment>]) -> Self {
        let environments = collection
            .environments
            .iter()
            .flat_map(|env| with_base(env, shared));
        Self::from_sources(
            [collection.variables.as_slice()]
                .into_iter()
                .chain(environments.map(|env| env.variables.as_slice())),
        )
    }

    /// The environment, its base, and its collection.
    pub fn collection_environment(
        collection: &Collection,
        environment: &Environment,
        shared: &[Arc<Environment>],
    ) -> Self {
        Self::from_sources(
            with_base(environment, shared)
                .map(|env| env.variables.as_slice())
                .chain([collection.variables.as_slice()]),
        )
    }

    /// The shared environment, the collection environments based on it, and
    /// their collections.
    pub fn shared_environment(environment: &Environment, collections: &[Arc<Collection>]) -> Self {
        let linked = collections.iter().flat_map(|collection| {
            let based: Vec<_> = collection
                .environments
                .iter()
                .filter(|env| env.base == Some(environment.id))
                .map(|env| env.variables.as_slice())
                .collect();
            let collection_variables =
                (!based.is_empty()).then_some(collection.variables.as_slice());
            based.into_iter().chain(collection_variables)
        });
        Self::from_sources([environment.variables.as_slice()].into_iter().chain(linked))
    }

    /// [`Self::collection_environment`] for each of `collection`'s environments.
    pub fn collection_environments(
        collection: &Collection,
        shared: &[Arc<Environment>],
    ) -> HashMap<Uuid, Self> {
        collection
            .environments
            .iter()
            .map(|env| {
                (
                    env.id,
                    Self::collection_environment(collection, env, shared),
                )
            })
            .collect()
    }

    /// [`Self::shared_environment`] for each shared environment.
    pub fn shared_environments(
        shared: &[Arc<Environment>],
        collections: &[Arc<Collection>],
    ) -> HashMap<Uuid, Self> {
        shared
            .iter()
            .map(|env| (env.id, Self::shared_environment(env, collections)))
            .collect()
    }

    /// The names referenced in `auth` that aren't defined, in first-seen order.
    pub fn unresolved(&self, auth: &Auth) -> Vec<String> {
        let mut unresolved = Vec::new();
        for text in auth.texts() {
            collect_undefined(text, &mut unresolved, |name| self.0.contains(name));
        }
        unresolved
    }
}

/// `environment`, then its base if it exists.
fn with_base<'a>(
    environment: &'a Environment,
    shared: &'a [Arc<Environment>],
) -> impl Iterator<Item = &'a Environment> {
    let base = environment
        .base
        .and_then(|id| environment::find(shared, id));
    [environment].into_iter().chain(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::ApiKeyLocation;

    fn variable(key: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: "v".into(),
        }
    }

    fn environment(name: &str, keys: &[&str]) -> Environment {
        Environment {
            variables: keys.iter().map(|key| variable(key)).collect(),
            ..Environment::new(name)
        }
    }

    fn based_on(base: &Environment, name: &str, keys: &[&str]) -> Environment {
        Environment {
            base: Some(base.id),
            ..environment(name, keys)
        }
    }

    fn collection(keys: &[&str], environments: Vec<Environment>) -> Collection {
        Collection {
            variables: keys.iter().map(|key| variable(key)).collect(),
            environments,
            ..Collection::new("Pets")
        }
    }

    fn bearer(token: &str) -> Auth {
        Auth::Bearer {
            token: token.into(),
        }
    }

    fn basic(username: &str, password: &str) -> Auth {
        Auth::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    #[test]
    fn collection_auth_uses_its_variables_and_any_environment_and_base() {
        let base = Arc::new(environment("Shared", &["shared"]));
        let staging = based_on(&base, "Staging", &["staging"]);
        let prod = environment("Prod", &["prod"]);
        let collection = collection(&["own"], vec![staging, prod]);
        let defined = DefinedNames::collection(&collection, &[base]);

        assert!(defined.unresolved(&bearer("{{own}}")).is_empty());
        assert!(defined.unresolved(&bearer("{{staging}}")).is_empty());
        assert!(defined.unresolved(&bearer("{{prod}}")).is_empty());
        assert!(defined.unresolved(&bearer("{{shared}}")).is_empty());
        assert_eq!(defined.unresolved(&bearer("{{missing}}")), ["missing"]);
    }

    #[test]
    fn collection_auth_ignores_unrelated_shared_environments() {
        let unrelated = Arc::new(environment("Other", &["other"]));
        let collection = collection(&[], vec![environment("Staging", &[])]);
        let defined = DefinedNames::collection(&collection, &[unrelated]);
        assert_eq!(defined.unresolved(&bearer("{{other}}")), ["other"]);
    }

    #[test]
    fn environment_auth_uses_itself_its_base_and_the_collection() {
        let base = Arc::new(environment("Shared", &["shared"]));
        let staging = based_on(&base, "Staging", &["staging"]);
        let prod = environment("Prod", &["prod"]);
        let collection = collection(&["own"], vec![staging.clone(), prod]);
        let defined = DefinedNames::collection_environment(&collection, &staging, &[base]);

        let auth = basic("{{staging}} {{shared}}", "{{own}} {{prod}}");
        assert_eq!(defined.unresolved(&auth), ["prod"]);
    }

    #[test]
    fn environment_auth_with_a_missing_base_ignores_it() {
        let gone = environment("Gone", &["gone"]);
        let staging = based_on(&gone, "Staging", &[]);
        let collection = collection(&[], vec![staging.clone()]);
        let defined = DefinedNames::collection_environment(&collection, &staging, &[]);
        assert_eq!(defined.unresolved(&bearer("{{gone}}")), ["gone"]);
    }

    #[test]
    fn shared_auth_uses_itself_and_environments_based_on_it_with_their_collections() {
        let shared = environment("Shared", &["shared"]);
        let other = environment("Other", &[]);
        let linked = collection(
            &["linked_collection"],
            vec![
                based_on(&shared, "Staging", &["staging"]),
                based_on(&other, "Prod", &["prod"]),
            ],
        );
        let unlinked = collection(&["unlinked_collection"], vec![environment("Dev", &["dev"])]);
        let collections = [Arc::new(linked), Arc::new(unlinked)];
        let defined = DefinedNames::shared_environment(&shared, &collections);

        let auth = Auth::ApiKey {
            key: "{{shared}}{{staging}}{{linked_collection}}".into(),
            value: "{{prod}} {{unlinked_collection}} {{dev}}".into(),
            location: ApiKeyLocation::Header,
        };
        assert_eq!(
            defined.unresolved(&auth),
            ["prod", "unlinked_collection", "dev"]
        );
    }

    #[test]
    fn per_environment_maps_cover_every_environment() {
        let base = environment("Shared", &["shared"]);
        let staging = based_on(&base, "Staging", &["staging"]);
        let shared = [Arc::new(base)];
        let collections = [Arc::new(collection(&["own"], vec![staging.clone()]))];

        let environments = DefinedNames::collection_environments(&collections[0], &shared);
        assert_eq!(
            environments[&staging.id],
            DefinedNames::collection_environment(&collections[0], &staging, &shared)
        );
        let shared_map = DefinedNames::shared_environments(&shared, &collections);
        assert!(
            shared_map[&shared[0].id]
                .unresolved(&bearer("{{staging}}{{own}}"))
                .is_empty()
        );
    }

    #[test]
    fn disabled_rows_and_blank_keys_are_not_defined() {
        let mut collection = collection(&[" token "], vec![]);
        collection.variables.push(KeyValue {
            enabled: false,
            key: "off".into(),
            value: "v".into(),
        });
        let defined = DefinedNames::collection(&collection, &[]);
        assert_eq!(
            defined.unresolved(&bearer("{{token}} {{off}} {{off}}")),
            ["off"]
        );
    }

    #[test]
    fn auth_without_values_has_nothing_unresolved() {
        let defined = DefinedNames::default();
        assert!(defined.unresolved(&Auth::None).is_empty());
        assert!(defined.unresolved(&Auth::Inherit).is_empty());
        assert_eq!(defined.unresolved(&basic("{{a}}", "{{b}}")), ["a", "b"]);
    }

    #[test]
    fn a_name_disabled_in_one_layer_is_defined_by_another() {
        let mut staging = environment("Staging", &[" region"]);
        staging.variables.push(KeyValue {
            enabled: false,
            key: "token".into(),
            value: "v".into(),
        });
        let collection = collection(&["token "], vec![staging.clone()]);
        let defined = DefinedNames::collection_environment(&collection, &staging, &[]);
        assert!(
            defined
                .unresolved(&bearer("{{token}}{{ region }}"))
                .is_empty()
        );
        assert_eq!(
            defined.unresolved(&basic("{{b}} {{ region}} {{a}}", "{{a}} {{c}} {{b}}")),
            ["b", "a", "c"]
        );
    }
}
