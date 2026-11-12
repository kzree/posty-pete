//! Environments: named variable sets. A collection's own environments live in
//! its file; shared environments belong to no collection and are stored one
//! per file.

use std::{collections::HashSet, sync::Arc};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    request::{Auth, KeyValue},
    variables::Variables,
};

pub const NEW_ENVIRONMENT_NAME: &str = "New environment";

/// A named set of variables layered in front of the collection's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    /// The shared environment a collection environment is based on. Always
    /// `None` on shared environments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Uuid>,
    pub variables: Vec<KeyValue>,
    /// Inherit means not set: the next layer's auth applies.
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
}

impl Environment {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            base: None,
            variables: Vec::new(),
            auth: Auth::Inherit,
        }
    }
}

pub fn find(shared: &[Arc<Environment>], id: Uuid) -> Option<&Environment> {
    shared.iter().find(|env| env.id == id).map(|env| &**env)
}

/// A base environment's variable as seen from an environment based on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InheritedVariable {
    pub name: String,
    pub value: String,
    /// The environment sets its own value for `name`.
    pub overridden: bool,
}

/// The base's enabled, named rows as they apply under `own`: once per
/// trimmed name (the first wins), in order.
pub fn inherited_variables(base: &[KeyValue], own: &[KeyValue]) -> Vec<InheritedVariable> {
    let own = Variables::layered([own]);
    let mut seen = HashSet::new();
    base.iter()
        .filter(|row| row.enabled)
        .filter_map(|row| {
            let name = row.key.trim();
            (!name.is_empty() && seen.insert(name)).then(|| InheritedVariable {
                name: name.to_string(),
                value: row.value.clone(),
                overridden: own.get(name).is_some(),
            })
        })
        .collect()
}

/// Adds a shared environment named [`NEW_ENVIRONMENT_NAME`], numbered if
/// that's taken, and returns its id.
pub fn add(shared: &mut Vec<Arc<Environment>>) -> Uuid {
    let names = shared.iter().map(|env| env.name.as_str());
    let environment = Environment::new(unique_name(NEW_ENVIRONMENT_NAME, names));
    let id = environment.id;
    shared.push(Arc::new(environment));
    id
}

/// Returns `false`, changing (and copying) nothing, if there's no such
/// environment or the name is the same.
pub fn rename(shared: &mut [Arc<Environment>], id: Uuid, name: impl Into<String>) -> bool {
    let name = name.into();
    find(shared, id).is_some_and(|env| env.name != name)
        && modify(shared, id, |env| env.name = name)
}

/// Inserts a copy of the environment after it (see [`copy_of`]) and returns
/// the copy's id, or `None` if there's no such environment.
pub fn duplicate(shared: &mut Vec<Arc<Environment>>, id: Uuid) -> Option<Uuid> {
    let index = shared.iter().position(|env| env.id == id)?;
    let names = shared.iter().map(|env| env.name.as_str());
    let copy = copy_of(&shared[index], names);
    let copy_id = copy.id;
    shared.insert(index + 1, Arc::new(copy));
    Some(copy_id)
}

/// Everything of `environment` under a new id, named "<name> copy", numbered
/// if that's in `taken`.
pub(super) fn copy_of<'a>(
    environment: &Environment,
    taken: impl Iterator<Item = &'a str> + Clone,
) -> Environment {
    Environment {
        id: Uuid::new_v4(),
        name: unique_name(&format!("{} copy", environment.name), taken),
        ..environment.clone()
    }
}

/// Returns `false` if there's no such environment.
pub fn delete(shared: &mut Vec<Arc<Environment>>, id: Uuid) -> bool {
    let before = shared.len();
    shared.retain(|env| env.id != id);
    shared.len() != before
}

/// Replaces the environment's variables. Returns `false`, changing (and
/// copying) nothing, if there's no such environment or they're the same.
pub fn set_variables(shared: &mut [Arc<Environment>], id: Uuid, variables: Vec<KeyValue>) -> bool {
    find(shared, id).is_some_and(|env| env.variables != variables)
        && modify(shared, id, |env| env.variables = variables)
}

/// Runs `f` on the environment, copying it first if it's shared.
fn modify(shared: &mut [Arc<Environment>], id: Uuid, f: impl FnOnce(&mut Environment)) -> bool {
    shared
        .iter_mut()
        .find(|env| env.id == id)
        .map(|env| f(Arc::make_mut(env)))
        .is_some()
}

/// `base`, or `base 2`, `base 3`, ... whichever is the first not in `taken`.
pub(super) fn unique_name<'a>(base: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let is_taken = |name: &str| taken.clone().any(|taken| taken == name);
    if !is_taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !is_taken(name))
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::layers::Layers;

    fn variable(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    fn names(shared: &[Arc<Environment>]) -> Vec<&str> {
        shared.iter().map(|env| env.name.as_str()).collect()
    }

    #[test]
    fn json_without_base_loads_and_saves_without_it() {
        let json =
            r#"{"id": "6f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f", "name": "Old", "variables": []}"#;
        let loaded: Environment = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.base, None);
        let value = serde_json::to_value(&loaded).unwrap();
        assert!(value.get("base").is_none());

        let based = Environment {
            base: Some(Uuid::new_v4()),
            ..loaded
        };
        let value = serde_json::to_value(&based).unwrap();
        let reloaded: Environment = serde_json::from_value(value).unwrap();
        assert_eq!(reloaded, based);
    }

    #[test]
    fn json_without_auth_inherits_and_saves_without_it() {
        let json =
            r#"{"id": "6f1c1f9e-4a5b-4c3d-9e8f-0a1b2c3d4e5f", "name": "Old", "variables": []}"#;
        let loaded: Environment = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.auth, Auth::Inherit);
        let expected: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), expected);

        for auth in [Auth::None, Auth::Bearer { token: "t".into() }] {
            let with_auth = Environment {
                auth,
                ..loaded.clone()
            };
            let value = serde_json::to_value(&with_auth).unwrap();
            let reloaded: Environment = serde_json::from_value(value).unwrap();
            assert_eq!(reloaded, with_auth);
        }
    }

    #[test]
    fn duplicate_copies_auth() {
        let mut shared = Vec::new();
        let id = add(&mut shared);
        Arc::make_mut(&mut shared[0]).auth = Auth::Bearer { token: "t".into() };
        let copy = duplicate(&mut shared, id).unwrap();
        assert_eq!(find(&shared, copy).unwrap().auth, shared[0].auth);
    }

    #[test]
    fn add_numbers_taken_default_names() {
        let mut shared = Vec::new();
        let first = add(&mut shared);
        add(&mut shared);
        assert!(rename(&mut shared, first, "Local"));
        add(&mut shared);
        assert_eq!(
            names(&shared),
            ["Local", "New environment 2", "New environment"]
        );
    }

    #[test]
    fn rename_only_when_it_changes() {
        let mut shared = Vec::new();
        let id = add(&mut shared);
        let held = shared.clone();

        assert!(!rename(&mut shared, id, NEW_ENVIRONMENT_NAME));
        assert!(!rename(&mut shared, Uuid::new_v4(), "Prod"));
        assert!(Arc::ptr_eq(&shared[0], &held[0]));

        assert!(rename(&mut shared, id, "Prod"));
        assert_eq!(names(&shared), ["Prod"]);
        assert_eq!(names(&held), [NEW_ENVIRONMENT_NAME]);
    }

    #[test]
    fn duplicate_inserts_a_numbered_copy_after_it() {
        let mut shared = Vec::new();
        let id = add(&mut shared);
        rename(&mut shared, id, "Staging");
        let mut disabled = variable("token", "t");
        disabled.enabled = false;
        let rows = vec![variable("host", "staging"), disabled];
        set_variables(&mut shared, id, rows.clone());
        let last = add(&mut shared);

        let first_copy = duplicate(&mut shared, id).unwrap();
        let second_copy = duplicate(&mut shared, id).unwrap();
        assert_eq!(
            names(&shared),
            [
                "Staging",
                "Staging copy 2",
                "Staging copy",
                NEW_ENVIRONMENT_NAME
            ]
        );
        assert_eq!(shared[3].id, last);
        assert_eq!(shared[2].id, first_copy);
        assert_eq!(shared[1].id, second_copy);
        assert_ne!(first_copy, id);
        assert_eq!(shared[2].variables, rows);
        assert_eq!(shared[0].variables, rows);

        assert!(set_variables(&mut shared, first_copy, Vec::new()));
        assert_eq!(shared[0].variables, rows);
        assert_eq!(duplicate(&mut shared, Uuid::new_v4()), None);
        assert_eq!(shared.len(), 4);
    }

    #[test]
    fn copy_of_keeps_the_base() {
        let mut original = Environment::new("Local");
        original.base = Some(Uuid::new_v4());
        original.variables = vec![variable("host", "localhost")];
        let copy = copy_of(&original, ["Local"].into_iter());
        assert_eq!(
            copy,
            Environment {
                id: copy.id,
                name: "Local copy".into(),
                ..original.clone()
            }
        );
        assert_ne!(copy.id, original.id);
    }

    #[test]
    fn delete_removes_only_it() {
        let mut shared = Vec::new();
        let deleted = add(&mut shared);
        let kept = add(&mut shared);

        assert!(delete(&mut shared, deleted));
        assert!(!delete(&mut shared, deleted));
        assert!(find(&shared, deleted).is_none());
        assert!(find(&shared, kept).is_some());
    }

    #[test]
    fn set_variables_only_when_they_change() {
        let mut shared = Vec::new();
        let id = add(&mut shared);
        let rows = vec![variable("host", "staging")];

        assert!(set_variables(&mut shared, id, rows.clone()));
        let held = shared.clone();
        assert!(!set_variables(&mut shared, id, rows.clone()));
        assert!(!set_variables(&mut shared, Uuid::new_v4(), Vec::new()));
        assert!(Arc::ptr_eq(&shared[0], &held[0]));
        assert_eq!(shared[0].variables, rows);
    }

    #[test]
    fn shared_variables_are_the_environments_own() {
        let mut shared = Vec::new();
        let id = add(&mut shared);
        let mut disabled = variable("token", "t");
        disabled.enabled = false;
        set_variables(&mut shared, id, vec![variable("host", "staging"), disabled]);

        let variables = Layers::Shared(find(&shared, id).unwrap()).variables();
        assert_eq!(variables.get("host"), Some("staging"));
        assert_eq!(variables.get("token"), None);
    }

    #[test]
    fn inherited_variables_mark_the_names_set_own() {
        let mut disabled = variable("token", "base token");
        disabled.enabled = false;
        let base = [
            variable(" host ", "staging"),
            variable("port", "443"),
            disabled.clone(),
            variable("", "unnamed"),
            variable("host", "duplicate"),
        ];
        let mut own_disabled = variable("port", "80");
        own_disabled.enabled = false;
        let own = [variable("host ", "local"), own_disabled];

        let inherited = inherited_variables(&base, &own);
        let expected = [("host", "staging", true), ("port", "443", false)];
        assert_eq!(inherited.len(), expected.len());
        for (row, (name, value, overridden)) in inherited.iter().zip(expected) {
            assert_eq!(
                (row.name.as_str(), row.value.as_str(), row.overridden),
                (name, value, overridden)
            );
        }
    }

    #[test]
    fn inherited_variables_of_an_empty_base_are_empty() {
        assert!(inherited_variables(&[], &[variable("host", "local")]).is_empty());
    }
}
