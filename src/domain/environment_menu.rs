use std::sync::Arc;

use uuid::Uuid;

use super::{
    collection::Collection,
    environment::{self, Environment},
    environment_selection::EnvironmentSelection,
};

pub const NO_ENVIRONMENT: &str = "No environment";
pub const SHARED_HEADING: &str = "Shared";

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
    /// A shared environment, for the draft.
    Shared(Uuid),
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
    /// Opens the shared environments, with this one selected if given.
    ManageShared(Option<Uuid>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentMenu {
    pub scope: MenuScope,
    /// The active environment's name, or "No environment".
    pub label: String,
    /// "Collection › Environment" when there's a collection, "Shared ›
    /// Environment" for a draft using a shared environment.
    pub tooltip: String,
    pub items: Vec<EnvironmentMenuItem>,
}

impl EnvironmentMenu {
    /// `None` when `scope`'s collection doesn't exist.
    pub fn new(
        scope: MenuScope,
        selection: &EnvironmentSelection,
        collections: &[Arc<Collection>],
        shared: &[Arc<Environment>],
    ) -> Option<Self> {
        let collection = match scope {
            MenuScope::Collection(id) => Some(collections.iter().find(|c| c.id == id)?),
            MenuScope::Draft => selection.draft_collection(collections),
        };
        let draft_shared = match scope {
            MenuScope::Collection(_) => None,
            MenuScope::Draft => selection.draft_shared_environment(shared),
        };
        let active = match draft_shared {
            Some(environment) => Some(environment.name.as_str()),
            None => collection.and_then(|c| active_name(c, selection)),
        };
        let label = active.unwrap_or(NO_ENVIRONMENT).to_string();
        let tooltip = match (collection, draft_shared) {
            (Some(collection), _) => format!("{} › {label}", collection.name),
            (None, Some(_)) => format!("Shared › {label}"),
            (None, None) => "Environment".to_string(),
        };
        let mut items = match (scope, collection) {
            (MenuScope::Collection(_), Some(collection)) => collection_items(collection, selection),
            _ => draft_items(collection, draft_shared, selection, collections, shared),
        };
        items.push(EnvironmentMenuItem::ManageShared(manage_shared_selection(
            scope,
            collection,
            draft_shared,
            selection,
            shared,
        )));
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

/// The shared environment "Manage shared environments…" selects: the draft's,
/// or the base of the collection's active environment.
fn manage_shared_selection(
    scope: MenuScope,
    collection: Option<&Arc<Collection>>,
    draft_shared: Option<&Environment>,
    selection: &EnvironmentSelection,
    shared: &[Arc<Environment>],
) -> Option<Uuid> {
    match scope {
        MenuScope::Draft => draft_shared.map(|environment| environment.id),
        MenuScope::Collection(_) => {
            let collection = collection?;
            let active = collection.environment(selection.active_environment(collection)?)?;
            let base = environment::find(shared, active.base?)?;
            Some(base.id)
        }
    }
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

/// The shared environments under "Shared", `active` checked; nothing when
/// there are none.
fn shared_group(
    shared: &[Arc<Environment>],
    active: Option<Uuid>,
) -> impl Iterator<Item = EnvironmentMenuItem> + '_ {
    let heading = (!shared.is_empty()).then(|| {
        [
            EnvironmentMenuItem::Separator,
            EnvironmentMenuItem::Heading(SHARED_HEADING.to_string()),
        ]
    });
    let choices = shared
        .iter()
        .map(move |environment| EnvironmentMenuItem::Choice {
            choice: EnvironmentChoice::Shared(environment.id),
            name: environment.name.clone(),
            checked: active == Some(environment.id),
        });
    heading.into_iter().flatten().chain(choices)
}

/// "Shared", then every collection with environments under its name. Only
/// the draft source is checked: its shared environment, or the draft
/// collection's active environment.
fn draft_items(
    draft: Option<&Arc<Collection>>,
    draft_shared: Option<&Environment>,
    selection: &EnvironmentSelection,
    collections: &[Arc<Collection>],
    shared: &[Arc<Environment>],
) -> Vec<EnvironmentMenuItem> {
    let draft_active = draft.and_then(|c| selection.active_environment(c));
    let shared_active = draft_shared.map(|environment| environment.id);
    let mut items = vec![no_environment(
        draft_active.is_none() && shared_active.is_none(),
    )];
    items.extend(shared_group(shared, shared_active));
    for collection in collections.iter().filter(|c| !c.environments.is_empty()) {
        let is_draft = draft.is_some_and(|draft| draft.id == collection.id);
        items.push(EnvironmentMenuItem::Separator);
        items.push(EnvironmentMenuItem::Heading(collection.name.clone()));
        items.extend(choices(collection, draft_active.filter(|_| is_draft)));
    }
    items.push(EnvironmentMenuItem::Separator);
    if let Some(draft) = draft {
        items.push(EnvironmentMenuItem::Manage(draft.id));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment::Environment;

    fn environment(name: &str) -> Environment {
        Environment::new(name)
    }

    fn collection(name: &str, environments: &[&str]) -> Arc<Collection> {
        let mut collection = Collection::new(name);
        collection.environments = environments.iter().map(|name| environment(name)).collect();
        Arc::new(collection)
    }

    fn shared_env(name: &str) -> Arc<Environment> {
        Arc::new(environment(name))
    }

    fn shared_choice(environment: &Environment, checked: bool) -> EnvironmentMenuItem {
        EnvironmentMenuItem::Choice {
            choice: EnvironmentChoice::Shared(environment.id),
            name: environment.name.clone(),
            checked,
        }
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
            EnvironmentMenu::new(MenuScope::Collection(api.id), &selection, &collections, &[])
                .unwrap();

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
                EnvironmentMenuItem::ManageShared(None),
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
            &[],
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
                EnvironmentMenuItem::ManageShared(None),
            ]
        );
    }

    #[test]
    fn a_missing_collection_has_no_menu() {
        let menu = EnvironmentMenu::new(
            MenuScope::Collection(Uuid::new_v4()),
            &EnvironmentSelection::default(),
            &[],
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

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections, &[]).unwrap();

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
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::ManageShared(None),
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

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections, &[]).unwrap();

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
                EnvironmentMenuItem::ManageShared(None),
            ]
        );
    }

    #[test]
    fn draft_collection_without_environments_still_offers_manage() {
        let empty = collection("Empty", &[]);
        let collections = [empty.clone()];
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_collection(Some(empty.id), None);

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &collections, &[]).unwrap();

        assert_eq!(menu.tooltip, "Empty › No environment");
        assert_eq!(
            menu.items,
            vec![
                no_environment(true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(empty.id),
                EnvironmentMenuItem::ManageShared(None),
            ]
        );
    }

    #[test]
    fn draft_using_a_shared_environment_checks_only_it() {
        let api = collection("API", &["Local"]);
        let collections = [api.clone()];
        let shared = [shared_env("Dev"), shared_env("Prod")];
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_collection(Some(api.id), Some(env_id(&api, 0)));
        selection.set_draft_shared_environment(shared[1].id);

        let menu =
            EnvironmentMenu::new(MenuScope::Draft, &selection, &collections, &shared).unwrap();

        assert_eq!(menu.label, "Prod");
        assert_eq!(menu.tooltip, "Shared › Prod");
        assert_eq!(
            menu.items,
            vec![
                no_environment(false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading(SHARED_HEADING.into()),
                shared_choice(&shared[0], false),
                shared_choice(&shared[1], true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("API".into()),
                choice(&api, 0, false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::ManageShared(Some(shared[1].id)),
            ]
        );
    }

    #[test]
    fn draft_on_a_collection_lists_shared_environments_unchecked() {
        let api = collection("API", &["Local"]);
        let collections = [api.clone()];
        let shared = [shared_env("Dev")];
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_collection(Some(api.id), Some(env_id(&api, 0)));

        let menu =
            EnvironmentMenu::new(MenuScope::Draft, &selection, &collections, &shared).unwrap();

        assert_eq!(
            menu.items,
            vec![
                no_environment(false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading(SHARED_HEADING.into()),
                shared_choice(&shared[0], false),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Heading("API".into()),
                choice(&api, 0, true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(api.id),
                EnvironmentMenuItem::ManageShared(None),
            ]
        );
    }

    #[test]
    fn collection_scope_has_no_shared_group_and_manages_the_active_base() {
        let dev = shared_env("Dev");
        let mut api = Collection::new("API");
        let mut local = environment("Local");
        local.base = Some(dev.id);
        api.environments = vec![local];
        let api = Arc::new(api);
        let collections = [api.clone()];
        let mut selection = EnvironmentSelection::default();
        selection.set_active_environment(api.id, Some(env_id(&api, 0)));

        let menu = EnvironmentMenu::new(
            MenuScope::Collection(api.id),
            &selection,
            &collections,
            std::slice::from_ref(&dev),
        )
        .unwrap();

        assert_eq!(menu.tooltip, "API › Local");
        assert_eq!(
            menu.items,
            vec![
                no_environment(false),
                choice(&api, 0, true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::Manage(api.id),
                EnvironmentMenuItem::ManageShared(Some(dev.id)),
            ]
        );
    }

    #[test]
    fn collection_scope_ignores_a_deleted_base() {
        let mut api = Collection::new("API");
        let mut local = environment("Local");
        local.base = Some(Uuid::new_v4());
        api.environments = vec![local];
        let api = Arc::new(api);
        let collections = [api.clone()];
        let mut selection = EnvironmentSelection::default();
        selection.set_active_environment(api.id, Some(env_id(&api, 0)));

        let menu =
            EnvironmentMenu::new(MenuScope::Collection(api.id), &selection, &collections, &[])
                .unwrap();

        assert_eq!(
            menu.items.last(),
            Some(&EnvironmentMenuItem::ManageShared(None))
        );
    }

    #[test]
    fn draft_using_a_deleted_shared_environment_has_no_environment() {
        let mut selection = EnvironmentSelection::default();
        selection.set_draft_shared_environment(Uuid::new_v4());

        let menu = EnvironmentMenu::new(MenuScope::Draft, &selection, &[], &[]).unwrap();

        assert_eq!(menu.label, NO_ENVIRONMENT);
        assert_eq!(menu.tooltip, "Environment");
        assert_eq!(
            menu.items,
            vec![
                no_environment(true),
                EnvironmentMenuItem::Separator,
                EnvironmentMenuItem::ManageShared(None),
            ]
        );
    }
}
