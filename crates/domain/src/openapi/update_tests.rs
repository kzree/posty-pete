use serde_json::{Value, json};

use super::*;
use crate::{
    collection::{Folder, SourceLocation},
    openapi::import,
    request::Auth,
};

fn location() -> SourceLocation {
    SourceLocation::File("/specs/pets.json".into())
}

fn spec(paths: Value) -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {"title": "Pets", "version": "1"},
        "servers": [{"url": "https://pets.example.com"}],
        "tags": [{"name": "pets"}, {"name": "stores"}],
        "paths": paths,
    })
}

fn base_paths() -> Value {
    json!({
        "/pets": {
            "get": {"operationId": "listPets", "summary": "List pets", "tags": ["pets"]},
            "post": {"operationId": "addPet", "summary": "Add pet", "tags": ["pets"]},
        },
        "/stores": {
            "get": {"operationId": "listStores", "summary": "List stores", "tags": ["stores"]},
        },
        "/health": {
            "get": {"operationId": "health", "summary": "Health"},
        },
    })
}

fn convert(spec: &Value) -> Collection {
    import(&spec.to_string(), location()).unwrap().collection
}

fn base() -> Collection {
    convert(&spec(base_paths()))
}

fn by_key<'a>(collection: &'a Collection, key: &str) -> &'a SavedRequest {
    find_by_key(&collection.items, key).unwrap_or_else(|| panic!("no request {key}"))
}

fn find_by_key<'a>(items: &'a [Item], key: &str) -> Option<&'a SavedRequest> {
    items.iter().find_map(|item| match item {
        Item::Request(saved) if saved.origin.as_ref().is_some_and(|o| o.key == key) => Some(saved),
        Item::Request(_) => None,
        Item::Folder(folder) => find_by_key(&folder.items, key),
    })
}

fn by_key_mut<'a>(collection: &'a mut Collection, key: &str) -> &'a mut SavedRequest {
    let id = by_key(collection, key).id;
    collection.find_request_mut(id).unwrap()
}

fn folder_named<'a>(collection: &'a Collection, name: &str) -> &'a Folder {
    collection
        .items
        .iter()
        .find_map(|item| match item {
            Item::Folder(folder) if folder.name == name => Some(folder),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no folder {name}"))
}

/// The names of `items`, folders as "name/".
fn names(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Folder(folder) => format!("{}/", folder.name),
            Item::Request(saved) => saved.name.clone(),
        })
        .collect()
}

fn ids(changes: &[RequestChange]) -> Vec<Uuid> {
    changes.iter().map(|change| change.id).collect()
}

fn update(current: &Collection, fresh: &Collection) -> (UpdatePlan, Collection) {
    let plan = plan(current, fresh);
    let updated = apply(current, fresh, &plan.default_choices());
    (plan, updated)
}

fn assert_up_to_date(current: &Collection, spec: &Value) {
    let again = plan(current, &convert(spec));
    assert!(!again.has_changes(), "{again:#?}");
}

fn with_summary(paths: &mut Value, path: &str, method: &str, summary: &str) {
    paths[path][method]["summary"] = json!(summary);
}

#[test]
fn the_same_spec_is_already_up_to_date() {
    let current = base();
    let plan = plan(&current, &base());
    assert!(!plan.has_changes(), "{plan:#?}");
    assert_eq!(plan.auth, AuthChange::Unchanged);
    assert!(plan.kept.is_empty());

    let applied = apply(&current, &base(), &plan.default_choices());
    let strip = |collection: &Collection| {
        let mut value = serde_json::to_value(collection).unwrap();
        value["source"]["imported_at"] = Value::Null;
        value
    };
    assert_eq!(strip(&applied), strip(&current));
}

#[test]
fn unedited_requests_take_the_spec_changes_and_keep_their_ids() {
    let current = base();
    let mut paths = base_paths();
    with_summary(&mut paths, "/pets", "get", "List all pets");
    let new_spec = spec(paths);
    let (plan, updated) = update(&current, &convert(&new_spec));

    let old = by_key(&current, "listPets");
    assert_eq!(ids(&plan.updated), [old.id]);
    assert_eq!(plan.updated[0].name, "List all pets");
    assert_eq!(plan.updated[0].note, None);
    assert!(plan.added.is_empty() && plan.removed.is_empty() && plan.conflicts.is_empty());

    let new = by_key(&updated, "listPets");
    assert_eq!(new.id, old.id);
    assert_eq!(new.name, "List all pets");
    assert_eq!(new.origin.as_ref().unwrap().hash, new.content_hash());
    assert_eq!(
        folder_named(&updated, "pets").id,
        folder_named(&current, "pets").id
    );
    assert_eq!(updated.id, current.id);
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn user_edits_are_kept_while_the_spec_doesnt_change_them() {
    let mut current = base();
    by_key_mut(&mut current, "addPet")
        .request
        .url
        .push_str("?mine=1");
    let edited = by_key(&current, "addPet").clone();

    let plan = plan(&current, &base());
    assert!(!plan.has_changes());
    assert_eq!(ids(&plan.kept), [edited.id]);

    let mut paths = base_paths();
    with_summary(&mut paths, "/health", "get", "Health check");
    let (plan, updated) = update(&current, &convert(&spec(paths)));
    assert!(plan.has_changes());
    assert_eq!(ids(&plan.kept), [edited.id]);
    let kept = by_key(&updated, "addPet");
    assert_eq!(kept.request, edited.request);
    assert_eq!(kept.origin, edited.origin);
}

#[test]
fn conflicts_keep_the_users_version_by_default() {
    let mut current = base();
    by_key_mut(&mut current, "listPets")
        .request
        .url
        .push_str("?mine=1");
    let edited = by_key(&current, "listPets").clone();
    let mut paths = base_paths();
    with_summary(&mut paths, "/pets", "get", "List all pets");
    let new_spec = spec(paths);

    let (plan, updated) = update(&current, &convert(&new_spec));
    assert_eq!(ids(&plan.conflicts), [edited.id]);
    assert!(plan.updated.is_empty());
    let kept = by_key(&updated, "listPets");
    assert_eq!((&kept.name, &kept.request), (&edited.name, &edited.request));
    assert_ne!(kept.origin, edited.origin);

    // Kept on purpose: the next run with the same spec doesn't ask again.
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn conflicts_can_take_the_specs_version() {
    let mut current = base();
    by_key_mut(&mut current, "listPets")
        .request
        .url
        .push_str("?mine=1");
    let id = by_key(&current, "listPets").id;
    let mut paths = base_paths();
    with_summary(&mut paths, "/pets", "get", "List all pets");
    let new_spec = spec(paths);
    let fresh = convert(&new_spec);

    let plan = plan(&current, &fresh);
    let mut choices = plan.default_choices();
    choices.use_spec.insert(id);
    let updated = apply(&current, &fresh, &choices);

    let taken = by_key(&updated, "listPets");
    assert_eq!(taken.id, id);
    assert_eq!(taken.name, "List all pets");
    assert_eq!(taken.request, by_key(&fresh, "listPets").request);
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn the_same_change_on_both_sides_is_no_conflict() {
    let mut current = base();
    by_key_mut(&mut current, "listPets").name = "List all pets".into();
    let mut paths = base_paths();
    with_summary(&mut paths, "/pets", "get", "List all pets");
    let plan = plan(&current, &convert(&spec(paths)));
    assert!(!plan.has_changes(), "{plan:#?}");
}

#[test]
fn added_operations_go_into_their_tag_folder_at_the_spec_position() {
    let current = base();
    let mut paths = base_paths();
    paths["/pets/{id}"] = json!({
        "get": {"operationId": "getPet", "summary": "Get pet", "tags": ["pets"]},
    });
    paths["/pets"] = json!({
        "get": {"operationId": "listPets", "summary": "List pets", "tags": ["pets"]},
        "put": {"operationId": "replacePets", "summary": "Replace pets", "tags": ["pets"]},
        "post": {"operationId": "addPet", "summary": "Add pet", "tags": ["pets"]},
    });
    paths["/owners"] = json!({
        "get": {"operationId": "listOwners", "summary": "List owners", "tags": ["owners"]},
    });
    paths["/version"] = json!({"get": {"operationId": "version", "summary": "Version"}});
    let mut new_spec = spec(paths);
    new_spec["tags"] = json!([{"name": "pets"}, {"name": "owners"}, {"name": "stores"}]);
    let fresh = convert(&new_spec);

    let (plan, updated) = update(&current, &fresh);
    let added: Vec<_> = plan
        .added
        .iter()
        .map(|change| change.name.as_str())
        .collect();
    assert_eq!(added, ["Replace pets", "Get pet", "List owners", "Version"]);
    assert_eq!(by_key(&updated, "getPet").id, plan.added[1].id);

    assert_eq!(
        names(&updated.items),
        ["pets/", "owners/", "stores/", "Health", "Version"]
    );
    assert_eq!(
        names(&folder_named(&updated, "pets").items),
        ["List pets", "Replace pets", "Add pet", "Get pet"]
    );
    let owners = folder_named(&updated, "owners");
    assert_eq!(owners.origin.as_ref().unwrap().key, "owners");
    assert_eq!(names(&owners.items), ["List owners"]);
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn added_operations_find_their_folder_after_the_user_renamed_it() {
    let mut current = base();
    let pets = folder_named(&current, "pets").id;
    current.rename(pets, "Animals");
    let mut paths = base_paths();
    paths["/pets/{id}"] = json!({
        "get": {"operationId": "getPet", "summary": "Get pet", "tags": ["pets"]},
    });
    let (_, updated) = update(&current, &convert(&spec(paths)));
    assert_eq!(names(&updated.items), ["Animals/", "stores/", "Health"]);
    assert_eq!(
        names(&folder_named(&updated, "Animals").items),
        ["List pets", "Add pet", "Get pet"]
    );
}

#[test]
fn removed_operations_are_deleted_unless_edited() {
    let mut current = base();
    by_key_mut(&mut current, "addPet")
        .request
        .url
        .push_str("?mine=1");
    let (add_pet, health) = (by_key(&current, "addPet").id, by_key(&current, "health").id);
    let mut paths = base_paths();
    paths["/pets"].as_object_mut().unwrap().remove("post");
    paths.as_object_mut().unwrap().remove("/health");
    let new_spec = spec(paths);

    let (plan, updated) = update(&current, &convert(&new_spec));
    let removed: Vec<_> = plan
        .removed
        .iter()
        .map(|removal| (removal.id, removal.edited))
        .collect();
    assert_eq!(removed, [(add_pet, true), (health, false)]);
    assert_eq!(plan.default_choices().remove, HashSet::from([health]));

    assert!(updated.find_request(health).is_none());
    let kept = updated.find_request(add_pet).unwrap();
    assert!(
        kept.origin.is_none(),
        "a kept request becomes the user's own"
    );
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn removed_operations_follow_the_choices() {
    let mut current = base();
    by_key_mut(&mut current, "addPet")
        .request
        .url
        .push_str("?mine=1");
    let (add_pet, health) = (by_key(&current, "addPet").id, by_key(&current, "health").id);
    let mut paths = base_paths();
    paths["/pets"].as_object_mut().unwrap().remove("post");
    paths.as_object_mut().unwrap().remove("/health");
    let fresh = convert(&spec(paths));

    let mut choices = plan(&current, &fresh).default_choices();
    choices.remove = HashSet::from([add_pet]);
    let updated = apply(&current, &fresh, &choices);
    assert!(updated.find_request(add_pet).is_none());
    assert!(updated.find_request(health).unwrap().origin.is_none());
}

#[test]
fn a_request_edited_after_the_preview_isnt_removed() {
    let mut current = base();
    let health = by_key(&current, "health").id;
    let mut paths = base_paths();
    paths.as_object_mut().unwrap().remove("/health");
    let fresh = convert(&spec(paths));
    let choices = plan(&current, &fresh).default_choices();
    assert!(choices.remove.contains(&health));

    by_key_mut(&mut current, "health")
        .request
        .url
        .push_str("?mine=1");
    let updated = apply(&current, &fresh, &choices);
    let kept = updated.find_request(health).unwrap();
    assert!(kept.request.url.ends_with("?mine=1"));
    assert!(kept.origin.is_none());
}

#[test]
fn an_emptied_import_folder_goes_when_its_tag_does() {
    let current = base();
    let mut paths = base_paths();
    paths.as_object_mut().unwrap().remove("/stores");
    let mut new_spec = spec(paths);
    new_spec["tags"] = json!([{"name": "pets"}]);
    let (_, updated) = update(&current, &convert(&new_spec));
    assert_eq!(names(&updated.items), ["pets/", "Health"]);
}

#[test]
fn user_requests_and_folders_are_untouched() {
    let mut current = base();
    let mine = Folder::new("Mine");
    let mine_id = mine.id;
    current.add_item(None, Item::Folder(mine));
    let scratch = SavedRequest::new("Scratch", crate::request::Request::default());
    let scratch_id = scratch.id;
    current.add_item(Some(mine_id), Item::Request(scratch));
    let pets = folder_named(&current, "pets").id;
    let own = SavedRequest::new("My pet query", crate::request::Request::default());
    let own_id = own.id;
    current.add_item(Some(pets), Item::Request(own));
    let empty = Folder::new("Empty");
    let empty_id = empty.id;
    current.add_item(None, Item::Folder(empty));

    let mut paths = json!({});
    paths["/pets"] = json!({
        "get": {"operationId": "listPets", "summary": "List all pets", "tags": ["pets"]},
    });
    let mut new_spec = spec(paths);
    new_spec["tags"] = json!([{"name": "pets"}]);
    let (plan, updated) = update(&current, &convert(&new_spec));

    for id in [own_id, scratch_id] {
        assert!(!plan.removed.iter().any(|removal| removal.id == id));
        let (before, after) = (
            current.find_request(id).unwrap(),
            updated.find_request(id).unwrap(),
        );
        assert_eq!(
            (&after.name, &after.request),
            (&before.name, &before.request)
        );
    }
    assert_eq!(
        names(&folder_named(&updated, "pets").items),
        ["List all pets", "My pet query"]
    );
    assert!(updated.find_folder(mine_id).is_some());
    assert!(updated.find_folder(empty_id).is_some());
    assert_eq!(names(&updated.items), ["pets/", "Mine/", "Empty/"]);
}

#[test]
fn requests_the_user_moved_into_their_own_folder_stay_there() {
    let mut current = base();
    let mine = Folder::new("Mine");
    let mine_id = mine.id;
    current.add_item(None, Item::Folder(mine));
    let health = by_key(&current, "health").id;
    let item = current.remove(health).unwrap();
    current.add_item(Some(mine_id), item);

    let mut paths = base_paths();
    paths["/health"]["get"]["tags"] = json!(["stores"]);
    paths["/health"]["get"]["summary"] = json!("Health check");
    let (plan, updated) = update(&current, &convert(&spec(paths)));
    assert_eq!(ids(&plan.updated), [health]);
    assert_eq!(plan.updated[0].note, None);
    assert_eq!(updated.ancestors(health), Some(vec![mine_id]));
    assert_eq!(updated.find_request(health).unwrap().name, "Health check");
}

#[test]
fn unedited_requests_follow_a_tag_move() {
    let mut current = base();
    by_key_mut(&mut current, "addPet")
        .request
        .url
        .push_str("?mine=1");
    let (list_pets, add_pet, health) = (
        by_key(&current, "listPets").id,
        by_key(&current, "addPet").id,
        by_key(&current, "health").id,
    );
    let stores = folder_named(&current, "stores").id;
    let pets = folder_named(&current, "pets").id;

    let mut paths = base_paths();
    paths["/pets"]["get"]["tags"] = json!(["stores"]);
    paths["/pets"]["post"]["tags"] = json!(["stores"]);
    paths["/health"]["get"]["tags"] = json!(["pets"]);
    let new_spec = spec(paths);
    let (plan, updated) = update(&current, &convert(&new_spec));

    let notes: Vec<_> = plan
        .updated
        .iter()
        .map(|change| (change.id, change.note.as_deref()))
        .collect();
    assert_eq!(
        notes,
        [
            (health, Some("Moves to “pets”")),
            (list_pets, Some("Moves to “stores”")),
        ]
    );
    assert_eq!(updated.ancestors(list_pets), Some(vec![stores]));
    assert_eq!(updated.ancestors(add_pet), Some(vec![pets]), "edited stays");
    assert_eq!(updated.ancestors(health), Some(vec![pets]));
    assert_eq!(
        names(&folder_named(&updated, "stores").items),
        ["List pets", "List stores"]
    );
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn untagging_moves_an_unedited_request_to_the_top_level() {
    let current = base();
    let list_pets = by_key(&current, "listPets").id;
    let mut paths = base_paths();
    paths["/pets"]["get"]
        .as_object_mut()
        .unwrap()
        .remove("tags");
    let new_spec = spec(paths);
    let (plan, updated) = update(&current, &convert(&new_spec));
    assert_eq!(
        plan.updated[0].note.as_deref(),
        Some("Moves to the top level")
    );
    assert_eq!(updated.ancestors(list_pets), Some(vec![]));
    assert_eq!(
        names(&updated.items),
        ["pets/", "stores/", "List pets", "Health"]
    );
    assert_up_to_date(&updated, &new_spec);
}

fn secured(mut spec: Value, scheme: Value) -> Value {
    spec["components"] = json!({"securitySchemes": {"auth": scheme}});
    spec["security"] = json!([{"auth": []}]);
    spec
}

fn variable<'a>(collection: &'a Collection, key: &str) -> Option<&'a str> {
    collection
        .variables
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.value.as_str())
}

fn set_variable(collection: &mut Collection, key: &str, value: &str) {
    collection
        .variables
        .iter_mut()
        .find(|row| row.key == key)
        .unwrap()
        .value = value.into();
}

#[test]
fn variables_are_added_and_never_overwritten() {
    let bearer = json!({"type": "http", "scheme": "bearer"});
    let mut current = convert(&secured(spec(base_paths()), bearer.clone()));
    set_variable(&mut current, "token", "secret");
    set_variable(&mut current, "baseUrl", "http://localhost:8080");
    current.variables.push(KeyValue {
        enabled: true,
        key: "apiKey".into(),
        value: "mine".into(),
    });

    let mut new_spec = secured(
        spec(base_paths()),
        json!({"type": "apiKey", "in": "header", "name": "X-Key"}),
    );
    new_spec["servers"] = json!([{"url": "https://v2.pets.example.com"}]);
    new_spec["paths"]["/pets/{petId}"] = json!({
        "get": {"operationId": "getPet", "tags": ["pets"], "parameters": [
            {"name": "petId", "in": "path", "required": true, "example": 7}
        ]},
    });
    let (plan, updated) = update(&current, &convert(&new_spec));

    assert_eq!(plan.variables_added, ["petId"]);
    assert!(plan.variables_updated.is_empty());
    assert_eq!(plan.variables_unused, ["token"]);
    assert_eq!(
        variable(&updated, "token"),
        Some("secret"),
        "kept by default"
    );
    assert_eq!(variable(&updated, "baseUrl"), Some("http://localhost:8080"));
    assert_eq!(variable(&updated, "apiKey"), Some("mine"));
    assert_eq!(variable(&updated, "petId"), Some("7"));

    let owned: Vec<_> = updated
        .source
        .as_ref()
        .unwrap()
        .variables
        .iter()
        .map(|origin| origin.key.as_str())
        .collect();
    assert_eq!(owned, ["baseUrl", "petId"], "a user's apiKey isn't adopted");
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn unedited_variables_take_the_specs_value_and_unused_ones_can_go() {
    let bearer = json!({"type": "http", "scheme": "bearer"});
    let current = convert(&secured(spec(base_paths()), bearer));
    let mut new_spec = spec(base_paths());
    new_spec["servers"] = json!([{"url": "https://v2.pets.example.com"}]);
    let fresh = convert(&new_spec);

    let plan = plan(&current, &fresh);
    assert_eq!(plan.variables_updated, ["baseUrl"]);
    assert_eq!(plan.variables_unused, ["token"]);
    let mut choices = plan.default_choices();
    choices.remove_variables.insert("token".into());
    choices.remove_variables.insert("baseUrl".into());
    let updated = apply(&current, &fresh, &choices);
    assert_eq!(
        variable(&updated, "baseUrl"),
        Some("https://v2.pets.example.com")
    );
    assert_eq!(variable(&updated, "token"), None);
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn collection_auth_updates_only_while_unchanged() {
    let current = base();
    let new_spec = secured(
        spec(base_paths()),
        json!({"type": "http", "scheme": "bearer"}),
    );
    let (plan, updated) = update(&current, &convert(&new_spec));
    assert_eq!(plan.auth, AuthChange::Updated);
    assert_eq!(
        updated.auth,
        Auth::Bearer {
            token: "{{token}}".into()
        }
    );
    assert_up_to_date(&updated, &new_spec);

    let mut edited = base();
    let mine = Auth::Basic {
        username: "me".into(),
        password: "pw".into(),
    };
    edited.auth = mine.clone();
    let (plan, updated) = update(&edited, &convert(&new_spec));
    assert_eq!(plan.auth, AuthChange::KeptYours);
    assert!(plan.has_changes());
    assert_eq!(updated.auth, mine);
    assert_up_to_date(&updated, &new_spec);

    let plan = super::plan(&edited, &base());
    assert_eq!(plan.auth, AuthChange::Unchanged);
}

#[test]
fn source_metadata_is_refreshed() {
    let current = base();
    let mut new_spec = spec(base_paths());
    new_spec["info"]["version"] = json!("2");
    let fresh = convert(&new_spec);
    let (plan, updated) = update(&current, &fresh);
    assert_eq!(plan.version, Some(("1".into(), "2".into())));
    assert!(plan.has_changes());
    let source = updated.source.unwrap();
    assert_eq!(source.version, "2");
    assert_eq!(
        source.imported_at,
        fresh.source.as_ref().unwrap().imported_at
    );
    assert_eq!(updated.name, current.name);
}

#[test]
fn a_new_source_location_is_stored() {
    let current = base();
    let url = SourceLocation::Url("https://pets.example.com/openapi.json".into());
    let fresh = import(&spec(base_paths()).to_string(), url.clone())
        .unwrap()
        .collection;
    let (plan, updated) = update(&current, &fresh);
    assert!(plan.has_changes());
    assert_eq!(
        plan.source.as_deref(),
        Some("https://pets.example.com/openapi.json")
    );
    assert!(plan.added.is_empty() && plan.updated.is_empty());
    assert_eq!(updated.source.as_ref().unwrap().location, url);
    assert!(!super::plan(&updated, &fresh).has_changes());
    assert_eq!(super::plan(&current, &base()).source, None);
}

#[test]
fn a_request_the_user_moved_to_the_top_level_stays_there() {
    let mut current = base();
    let list_pets = by_key(&current, "listPets").id;
    let item = current.remove(list_pets).unwrap();
    current.add_item(None, item);
    let mut paths = base_paths();
    with_summary(&mut paths, "/pets", "get", "List all pets");
    let (plan, updated) = update(&current, &convert(&spec(paths)));
    assert_eq!(ids(&plan.updated), [list_pets]);
    assert_eq!(plan.updated[0].note, None);
    assert_eq!(updated.ancestors(list_pets), Some(vec![]));
    assert_eq!(
        updated.find_request(list_pets).unwrap().name,
        "List all pets"
    );
}

#[test]
fn a_newly_tagged_operation_moves_from_the_top_level() {
    let current = base();
    let health = by_key(&current, "health").id;
    assert!(by_key(&current, "health").origin.as_ref().unwrap().untagged);
    let mut paths = base_paths();
    paths["/health"]["get"]["tags"] = json!(["stores"]);
    let new_spec = spec(paths);
    let (plan, updated) = update(&current, &convert(&new_spec));
    assert_eq!(plan.updated[0].note.as_deref(), Some("Moves to “stores”"));
    assert_eq!(
        updated.ancestors(health),
        Some(vec![folder_named(&current, "stores").id])
    );
    assert!(!by_key(&updated, "health").origin.as_ref().unwrap().untagged);
    assert_up_to_date(&updated, &new_spec);
}

#[test]
fn origins_without_the_untagged_flag_dont_move_from_the_top_level() {
    let mut current = base();
    by_key_mut(&mut current, "health")
        .origin
        .as_mut()
        .unwrap()
        .untagged = false;
    let health = by_key(&current, "health").id;
    let mut paths = base_paths();
    paths["/health"]["get"]["tags"] = json!(["stores"]);
    let (_, updated) = update(&current, &convert(&spec(paths)));
    assert_eq!(updated.ancestors(health), Some(vec![]));
}
