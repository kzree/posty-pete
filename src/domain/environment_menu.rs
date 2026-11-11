use std::sync::Arc;

use uuid::Uuid;

use super::{collection::Collection, environment_selection::EnvironmentSelection};

pub const NO_ENVIRONMENT: &str = "No environment";

/// Whose environments the picker offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScope {
    /// One collection's, for its saved requests and its page.
    Collection(Uuid),
    /// Every collection's, for the draft.
    Draft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentChoice {
    None,
    Environment {
        collection_id: Uuid,
        environment_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentMenuItem {
    /// A collection's name above its environments.
    Heading(String),
    Choice {
        choice: EnvironmentChoice,
        name: String,
        checked: bool,
    },
    Separator,
    /// Opens collection `Uuid`'s environments.
    Manage(Uuid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentMenu {
    pub scope: MenuScope,
    /// The active environment's name, or "No environment".
    pub label: String,
    /// "Collection › Environment" when there's a collection.
    pub tooltip: String,
    pub items: Vec<EnvironmentMenuItem>,
}

impl EnvironmentMenu {
    /// `None` when `scope`'s collection doesn't exist.
    pub fn new(
        scope: MenuScope,
        selection: &EnvironmentSelection,
        collections: &[Arc<Collection>],
    ) -> Option<Self> {
        let collection = match scope {
            MenuScope::Collection(id) => Some(collections.iter().find(|c| c.id == id)?),
            MenuScope::Draft => selection.draft_collection(collections),
        };
        let active = collection.and_then(|c| active_name(c, selection));
        let label = active.unwrap_or(NO_ENVIRONMENT).to_string();
        let tooltip = match collection {
            Some(collection) => format!("{} › {label}", collection.name),
            None => "Environment".to_string(),
        };
        let items = match (scope, collection) {
            (MenuScope::Collection(_), Some(collection)) => collection_items(collection, selection),
            _ => draft_items(collection, selection, collections),
        };
        Some(Self {
            scope,
            label,
            tooltip,
            items,
        })
    }
}

fn active_name<'a>(
    collection: &'a Collection,
    selection: &EnvironmentSelection,
) -> Option<&'a str> {
    let id = selection.active_environment(collection)?;
    Some(collection.environment(id)?.name.as_str())
}

fn no_environment(checked: bool) -> EnvironmentMenuItem {
    EnvironmentMenuItem::Choice {
        choice: EnvironmentChoice::None,
        name: NO_ENVIRONMENT.to_string(),
        checked,
    }
}

/// `collection`'s environments, the active one checked.
fn choices(
    collection: &Collection,
    active: Option<Uuid>,
) -> impl Iterator<Item = EnvironmentMenuItem> + '_ {
    collection
        .environments
        .iter()
        .map(move |environment| EnvironmentMenuItem::Choice {
            choice: EnvironmentChoice::Environment {
                collection_id: collection.id,
                environment_id: environment.id,
            },
            name: environment.name.clone(),
            checked: active == Some(environment.id),
        })
}

fn collection_items(
    collection: &Collection,
    selection: &EnvironmentSelection,
) -> Vec<EnvironmentMenuItem> {
    let active = selection.active_environment(collection);
    let mut items = vec![no_environment(active.is_none())];
    items.extend(choices(collection, active));
    items.push(EnvironmentMenuItem::Separator);
    items.push(EnvironmentMenuItem::Manage(collection.id));
    items
}

/// Every collection with environments, under its name; only the draft
/// collection's active environment is checked.
fn draft_items(
    draft: Option<&Arc<Collection>>,
    selection: &EnvironmentSelection,
    collections: &[Arc<Collection>],
) -> Vec<EnvironmentMenuItem> {
    let draft_active = draft.and_then(|c| selection.active_environment(c));
    let mut items = vec![no_environment(draft_active.is_none())];
    for collection in collections.iter().filter(|c| !c.environments.is_empty()) {
        let is_draft = draft.is_some_and(|draft| draft.id == collection.id);
        items.push(EnvironmentMenuItem::Separator);
        items.push(EnvironmentMenuItem::Heading(collection.name.clone()));
        items.extend(choices(collection, draft_active.filter(|_| is_draft)));
    }
    if let Some(draft) = draft {
        items.push(EnvironmentMenuItem::Separator);
        items.push(EnvironmentMenuItem::Manage(draft.id));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::collection::Environment;

    fn environment(name: &str) -> Environment {
        Environment {
            id: Uuid::new_v4(),
            name: name.into(),
            variables: Vec::new(),
        }
    }

    fn collection(name: &str, environments: &[&str]) -> Arc<Collection> {
        let mut collection = Collection::new(name);
        collection.environments = environments.iter().map(|name| environment(name)).collect();
        Arc::new(collection)
    }

    fn env_id(collection: &Collection, ix: usize) -> Uuid {
        collection.environments[ix].id
    }

    fn choice(collection: &Collection, ix: usize, checked: bool) -> EnvironmentMenuItem {
        let environment = &collection.environments[ix];
        EnvironmentMenuItem::Choice {
            choice: EnvironmentChoice::Environment {
                collection_id: collection.id,
                environment_id: environment.id,
            },
            name: environment.name.clone(),
            checked,
        }
    }

    #[test]
    fn collection_scope_lists_its_environments_and_manage() {
        let api = collection("API", &["Local", "Staging"]);
        let collections = [api.clone(), collection("Other", &["Prod"])];
        let mut selection = EnvironmentSelection::default();
        selection.set_active_environment(api.id, Some(env_id(&api, 1)));

        let menu =
            EnvironmentMenu::new(MenuScope::Collection(api.id), &selection, &collections).unwrap();

        assert_eq!(menu.label, "Staging");
        assert_eq!(menu.tooltip, "API › Staging");
        assert_eq!(
            menu.items,
            vec![
                no_environment(false),
                choice(&api, 0, false),
                choice(&api, 1, true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(api.id),
            ]
        );
    }

    #[test]
    fn collection_scope_without_an_active_environment() {
        let api = collection("API", &[]);
        let collections = [api.clone()];
        let menu = EnvironmentMenu::new(
            MenuScope::Collection(api.id),
            &EnvironmentSelection::default(),
            &collections,
        )
        .unwrap();

        assert_eq!(menu.label, NO_ENVIRONMENT);
        assert_eq!(menu.tooltip, "API › No environment");
        assert_eq!(
            menu.items,
            vec![
                no_environment(true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(api.id),
            ]
        );
    }

    #[test]
    fn a_missing_collection_has_no_menu() {
        let menu = EnvironmentMenu::new(
            MenuScope::Collection(Uuid::new_v4()),
            &EnvironmentSelection::default(),
            &[],
        );
        assert_eq!(menu, None);
    }

    #[test]
    fn draft_without_a_draft_collection_groups_collections_with_environments() {
        let api = collection("API", &["Local"]);
        let empty = collection("Empty", &[]);
        let payments = collection("Payments", &["Staging", "Prod"]);
        let collections = [api.clone(), empty, payments.clone()];
        let mut selection = EnvironmentSelection::default();
        // Active, but not the draft collection's, so not checked.
        selection.set_active_environment(api.id, Some(env_id(&api, 0)));

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections).unwrap();

        assert_eq!(menu.label, NO_ENVIRONMENT);
        assert_eq!(menu.tooltip, "Environment");
        assert_eq!(
            menu.items,
            vec![
                no_environment(true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("API".into()),
                choice(&api, 0, false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("Payments".into()),
                choice(&payments, 0, false),
                choice(&payments, 1, false),
            ]
        );
    }

    #[test]
    fn draft_checks_the_draft_collections_environment_and_manages_it() {
        let api = collection("API", &["Local"]);
        let payments = collection("Payments", &["Staging", "Prod"]);
        let collections = [api.clone(), payments.clone()];
        let mut selection = EnvironmentSelection::default();
        selection.set_active_environment(api.id, Some(env_id(&api, 0)));
        selection.set_draft_collection(Some(payments.id), Some(env_id(&payments, 1)));

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections).unwrap();

        assert_eq!(menu.label, "Prod");
        assert_eq!(menu.tooltip, "Payments › Prod");
        assert_eq!(
            menu.items,
            vec![
                no_environment(false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("API".into()),
                choice(&api, 0, false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("Payments".into()),
                choice(&payments, 0, false),
                choice(&payments, 1, true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(payments.id),
            ]
        );
    }

    #[test]
    fn draft_collection_without_environments_still_offers_manage() {
        let empty = collection("Empty", &[]);
        let collections = [empty.clone()];
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_collection(Some(empty.id), None);

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections).unwrap();

        assert_eq!(menu.tooltip, "Empty › No environment");
        assert_eq!(
            menu.items,
            vec![
                no_environment(true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(empty.id),
            ]
        );
    }
}
