//! Updating an imported collection from a fresh import of its spec, keeping
//! the user's edits. Requests and folders match by their origin key, and the
//! hash stored with it tells whether the user or the spec changed one since.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{
    collection::{
        Collection, CollectionSource, Folder, Item, Origin, SavedRequest, auth_hash, value_hash,
    },
    request::{HttpMethod, KeyValue},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestChange {
    /// The request's id, or for an added one the id it's added with.
    pub id: Uuid,
    pub name: String,
    pub method: HttpMethod,
    /// Where an updated request moves, like "Moves to “pets”".
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    pub id: Uuid,
    pub name: String,
    pub method: HttpMethod,
    /// Changed by the user since the import, so kept by default.
    pub edited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthChange {
    Unchanged,
    /// The user hadn't changed it, so it takes the spec's.
    Updated,
    /// The spec changed it, but so did the user, whose auth stays.
    KeptYours,
}

/// What updating a collection from its spec does, for the preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePlan {
    pub added: Vec<RequestChange>,
    /// Unedited requests that take the spec's new version or move to its tag.
    pub updated: Vec<RequestChange>,
    /// Changed by both; the user's version stays unless chosen otherwise.
    pub conflicts: Vec<RequestChange>,
    /// Operations gone from the spec.
    pub removed: Vec<Removal>,
    /// Edited by the user while the spec didn't change them.
    pub kept: Vec<RequestChange>,
    pub variables_added: Vec<String>,
    /// Import-added variables the user hadn't changed, taking the spec's value.
    pub variables_updated: Vec<String>,
    /// Import-added variables the spec no longer needs. Kept by default.
    pub variables_unused: Vec<String>,
    pub auth: AuthChange,
    /// The spec's version before and after, when it changed.
    pub version: Option<(String, String)>,
    /// The new source's label, when the spec comes from somewhere else now.
    pub source: Option<String>,
}

/// The preview's choices. Start from [`UpdatePlan::default_choices`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    /// Conflicts that take the spec's version.
    pub use_spec: HashSet<Uuid>,
    /// Removed operations whose requests are deleted. The rest stay as the
    /// user's own requests.
    pub remove: HashSet<Uuid>,
    /// Unused variables that are deleted.
    pub remove_variables: HashSet<String>,
    /// Removed requests that were edited when previewed. One edited since
    /// isn't deleted unless it was edited then too and checked anyway.
    pub edited: HashSet<Uuid>,
}

impl UpdatePlan {
    /// Whether updating changes anything. Requests that keep the user's
    /// edits of an unchanged operation don't count.
    pub fn has_changes(&self) -> bool {
        !(self.added.is_empty()
            && self.updated.is_empty()
            && self.conflicts.is_empty()
            && self.removed.is_empty()
            && self.variables_added.is_empty()
            && self.variables_updated.is_empty()
            && self.variables_unused.is_empty()
            && self.auth == AuthChange::Unchanged
            && self.version.is_none()
            && self.source.is_none())
    }

    /// Unedited removed requests are deleted; everything else keeps the
    /// user's version.
    pub fn default_choices(&self) -> Choices {
        Choices {
            remove: self
                .removed
                .iter()
                .filter(|removal| !removal.edited)
                .map(|removal| removal.id)
                .collect(),
            edited: self
                .removed
                .iter()
                .filter(|removal| removal.edited)
                .map(|removal| removal.id)
                .collect(),
            ..Choices::default()
        }
    }
}

/// Compares `current` with `fresh`, a new import of its spec.
pub fn plan(current: &Collection, fresh: &Collection) -> UpdatePlan {
    let analysis = Analysis::new(current, fresh);
    let change = |saved: &SavedRequest, note: Option<String>| RequestChange {
        id: saved.id,
        name: saved.name.clone(),
        method: saved.request.method,
        note,
    };
    let mut plan = UpdatePlan {
        added: Vec::new(),
        updated: Vec::new(),
        conflicts: Vec::new(),
        removed: Vec::new(),
        kept: Vec::new(),
        variables_added: analysis
            .variables
            .added
            .iter()
            .map(|row| row.key.clone())
            .collect(),
        variables_updated: analysis
            .variables
            .updated
            .iter()
            .map(|(key, _)| key.clone())
            .collect(),
        variables_unused: analysis.variables.unused.clone(),
        auth: analysis.auth,
        version: analysis.version.clone(),
        source: analysis.source.clone(),
    };
    for entry in &analysis.entries {
        let matched = match entry {
            Entry::Matched(matched) => matched,
            Entry::Added { fresh, .. } => {
                plan.added.push(change(fresh, None));
                continue;
            }
        };
        let note = matched.moves.then(|| moves_to(matched.fresh_tag, fresh));
        match matched.kind {
            Kind::Updated => plan.updated.push(RequestChange {
                id: matched.current.id,
                ..change(matched.fresh, note)
            }),
            Kind::Unchanged if matched.moves => plan.updated.push(change(matched.current, note)),
            Kind::Conflict => plan.conflicts.push(change(matched.current, None)),
            Kind::Kept => plan.kept.push(change(matched.current, None)),
            Kind::Unchanged | Kind::Converged => {}
        }
    }
    plan.removed = analysis
        .removed
        .iter()
        .map(|(saved, edited)| Removal {
            id: saved.id,
            name: saved.name.clone(),
            method: saved.request.method,
            edited: *edited,
        })
        .collect();
    plan
}

/// Applies the update with `choices` and returns the updated collection.
/// Matched requests and folders keep their ids. Planning again with the same
/// `fresh` afterwards finds no changes.
pub fn apply(current: &Collection, fresh: &Collection, choices: &Choices) -> Collection {
    let analysis = Analysis::new(current, fresh);
    let mut updated = current.clone();
    let orders = sibling_orders(fresh);
    let filled_import_folders = import_folders(&current.items, |folder| !folder.items.is_empty());

    for (saved, edited) in &analysis.removed {
        let checked = choices.remove.contains(&saved.id);
        if checked && (!edited || choices.edited.contains(&saved.id)) {
            updated.remove(saved.id);
        } else if let Some(kept) = updated.find_request_mut(saved.id) {
            kept.origin = None;
        }
    }

    for entry in &analysis.entries {
        match entry {
            Entry::Added { fresh: saved, tag } => {
                let key = ItemKey::Request(origin_key(saved).to_owned());
                let item = Item::Request((*saved).clone());
                insert(&mut updated, item, &key, *tag, fresh, &orders);
            }
            Entry::Matched(matched) => {
                let id = matched.current.id;
                let take_spec = match matched.kind {
                    Kind::Updated => true,
                    Kind::Conflict => choices.use_spec.contains(&id),
                    Kind::Unchanged | Kind::Kept | Kind::Converged => false,
                };
                let Some(saved) = updated.find_request_mut(id) else {
                    continue;
                };
                if take_spec {
                    saved.name = matched.fresh.name.clone();
                    saved.request = matched.fresh.request.clone();
                }
                saved.origin = matched.fresh.origin.clone();
                if matched.moves
                    && let Some(item) = updated.remove(id)
                {
                    let key = ItemKey::Request(origin_key(matched.fresh).to_owned());
                    insert(&mut updated, item, &key, matched.fresh_tag, fresh, &orders);
                }
            }
        }
    }

    let fresh_tags: HashSet<&str> = fresh_folder_tags(&fresh.items);
    let emptied = import_folders(&updated.items, |folder| {
        folder.items.is_empty()
            && filled_import_folders.contains(&folder.id)
            && folder.origin.as_ref().is_some_and(|origin| {
                !fresh_tags.contains(origin.key.as_str()) && origin.hash == folder.content_hash()
            })
    });
    for id in emptied {
        updated.remove(id);
    }

    let variables = &analysis.variables;
    for (key, value) in &variables.updated {
        if let Some(row) = updated.variables.iter_mut().find(|row| &row.key == key) {
            row.value = value.clone();
        }
    }
    updated.variables.extend(variables.added.iter().cloned());
    updated.variables.retain(|row| {
        !(choices.remove_variables.contains(&row.key) && variables.unused.contains(&row.key))
    });

    if analysis.auth == AuthChange::Updated {
        updated.auth = fresh.auth.clone();
    }

    updated.source = fresh
        .source
        .clone()
        .or_else(|| current.source.clone())
        .map(|source| CollectionSource {
            variables: variables.owned.clone(),
            auth_hash: auth_hash(&fresh.auth),
            ..source
        });
    updated
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Neither side changed it.
    Unchanged,
    /// Only the spec changed it.
    Updated,
    /// Only the user changed it.
    Kept,
    Conflict,
    /// Both changed it the same way.
    Converged,
}

struct Matched<'a> {
    current: &'a SavedRequest,
    fresh: &'a SavedRequest,
    fresh_tag: Option<&'a str>,
    kind: Kind,
    /// Unedited, in an import-created place, and the spec gives it another tag.
    moves: bool,
}

enum Entry<'a> {
    Matched(Matched<'a>),
    Added {
        fresh: &'a SavedRequest,
        tag: Option<&'a str>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement<'a> {
    Root,
    ImportFolder(&'a str),
    UserFolder,
}

#[derive(Default)]
struct VariableChanges {
    added: Vec<KeyValue>,
    updated: Vec<(String, String)>,
    unused: Vec<String>,
    /// What the import owns afterwards.
    owned: Vec<Origin>,
}

struct Analysis<'a> {
    /// In the spec's order.
    entries: Vec<Entry<'a>>,
    removed: Vec<(&'a SavedRequest, bool)>,
    variables: VariableChanges,
    auth: AuthChange,
    version: Option<(String, String)>,
    source: Option<String>,
}

impl<'a> Analysis<'a> {
    fn new(current: &'a Collection, fresh: &'a Collection) -> Self {
        let mut placed = Vec::new();
        collect_current(&current.items, Placement::Root, &mut placed);
        let mut by_key: HashMap<&str, (&SavedRequest, Placement)> = HashMap::new();
        for (saved, placement) in placed.iter().copied() {
            by_key
                .entry(origin_key(saved))
                .or_insert((saved, placement));
        }

        let mut fresh_requests = Vec::new();
        collect_fresh(&fresh.items, None, &mut fresh_requests);
        let fresh_keys: HashSet<&str> = fresh_requests
            .iter()
            .map(|(saved, _)| origin_key(saved))
            .collect();

        let entries = fresh_requests
            .iter()
            .map(|&(fresh_saved, fresh_tag)| {
                let Some(&(current_saved, placement)) = by_key.get(origin_key(fresh_saved)) else {
                    return Entry::Added {
                        fresh: fresh_saved,
                        tag: fresh_tag,
                    };
                };
                let stored = stored_hash(current_saved);
                let now = current_saved.content_hash();
                let spec = stored_hash(fresh_saved);
                let edited = now != stored;
                let kind = match (edited, spec != stored) {
                    (false, false) => Kind::Unchanged,
                    (false, true) => Kind::Updated,
                    (true, false) => Kind::Kept,
                    (true, true) if now == spec => Kind::Converged,
                    (true, true) => Kind::Conflict,
                };
                let untagged = current_saved
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.untagged);
                let current_tag = match placement {
                    Placement::Root if untagged => Some(None),
                    Placement::Root => None,
                    Placement::ImportFolder(tag) => Some(Some(tag)),
                    Placement::UserFolder => None,
                };
                let moves = !edited && current_tag.is_some_and(|tag| tag != fresh_tag);
                Entry::Matched(Matched {
                    current: current_saved,
                    fresh: fresh_saved,
                    fresh_tag,
                    kind,
                    moves,
                })
            })
            .collect();

        let mut seen = HashSet::new();
        let removed = placed
            .iter()
            .map(|(saved, _)| *saved)
            .filter(|saved| seen.insert(origin_key(saved)))
            .filter(|saved| !fresh_keys.contains(origin_key(saved)))
            .map(|saved| (saved, saved.content_hash() != stored_hash(saved)))
            .collect();

        let (old_source, new_source) = (current.source.as_ref(), fresh.source.as_ref());
        let stored_auth = old_source.map(|source| source.auth_hash.as_str());
        let new_auth = auth_hash(&fresh.auth);
        let now_auth = auth_hash(&current.auth);
        let auth = if Some(new_auth.as_str()) == stored_auth || now_auth == new_auth {
            AuthChange::Unchanged
        } else if Some(now_auth.as_str()) == stored_auth {
            AuthChange::Updated
        } else {
            AuthChange::KeptYours
        };

        let version = match (old_source, new_source) {
            (Some(old), Some(new)) if old.version != new.version => {
                Some((old.version.clone(), new.version.clone()))
            }
            _ => None,
        };
        let source = match (old_source, new_source) {
            (Some(old), Some(new)) if (old.kind, &old.location) != (new.kind, &new.location) => {
                Some(new.location.label())
            }
            (None, Some(new)) => Some(new.location.label()),
            _ => None,
        };

        Self {
            entries,
            removed,
            variables: variable_changes(current, fresh),
            auth,
            version,
            source,
        }
    }
}

fn variable_changes(current: &Collection, fresh: &Collection) -> VariableChanges {
    let old_owned: HashMap<&str, &str> = current
        .source
        .iter()
        .flat_map(|source| &source.variables)
        .map(|origin| (origin.key.as_str(), origin.hash.as_str()))
        .collect();
    let spec_needs: HashSet<&str> = fresh
        .source
        .iter()
        .flat_map(|source| &source.variables)
        .map(|origin| origin.key.as_str())
        .collect();
    let mut changes = VariableChanges::default();
    let mut handled = HashSet::new();
    for row in &fresh.variables {
        if !spec_needs.contains(row.key.as_str()) || !handled.insert(row.key.as_str()) {
            continue;
        }
        let owned = Origin {
            key: row.key.clone(),
            hash: value_hash(&row.value),
            untagged: false,
        };
        match current.variables.iter().find(|have| have.key == row.key) {
            None => {
                changes.added.push(row.clone());
                changes.owned.push(owned);
            }
            Some(have) => {
                let Some(&stored) = old_owned.get(row.key.as_str()) else {
                    continue;
                };
                if value_hash(&have.value) == stored && have.value != row.value {
                    changes.updated.push((row.key.clone(), row.value.clone()));
                }
                changes.owned.push(owned);
            }
        }
    }
    let mut unused_seen = HashSet::new();
    changes.unused = current
        .source
        .iter()
        .flat_map(|source| &source.variables)
        .map(|origin| origin.key.as_str())
        .filter(|key| !spec_needs.contains(key) && unused_seen.insert(*key))
        .filter(|key| current.variables.iter().any(|row| row.key == *key))
        .map(str::to_owned)
        .collect();
    changes
}

fn origin_key(saved: &SavedRequest) -> &str {
    saved
        .origin
        .as_ref()
        .map_or("", |origin| origin.key.as_str())
}

fn stored_hash(saved: &SavedRequest) -> &str {
    saved
        .origin
        .as_ref()
        .map_or("", |origin| origin.hash.as_str())
}

/// The imported requests, each with where it sits.
fn collect_current<'a>(
    items: &'a [Item],
    placement: Placement<'a>,
    out: &mut Vec<(&'a SavedRequest, Placement<'a>)>,
) {
    for item in items {
        match item {
            Item::Request(saved) if saved.origin.is_some() => out.push((saved, placement)),
            Item::Request(_) => {}
            Item::Folder(folder) => {
                let inner = match &folder.origin {
                    Some(origin) => Placement::ImportFolder(&origin.key),
                    None => Placement::UserFolder,
                };
                collect_current(&folder.items, inner, out);
            }
        }
    }
}

/// The spec's requests, each with its tag.
fn collect_fresh<'a>(
    items: &'a [Item],
    tag: Option<&'a str>,
    out: &mut Vec<(&'a SavedRequest, Option<&'a str>)>,
) {
    for item in items {
        match item {
            Item::Request(saved) if saved.origin.is_some() => out.push((saved, tag)),
            Item::Request(_) => {}
            Item::Folder(folder) => {
                let inner = folder.origin.as_ref().map(|origin| origin.key.as_str());
                collect_fresh(&folder.items, inner.or(tag), out);
            }
        }
    }
}

fn fresh_folder_tags(items: &[Item]) -> HashSet<&str> {
    let mut tags = HashSet::new();
    for item in items {
        if let Item::Folder(folder) = item {
            if let Some(origin) = &folder.origin {
                tags.insert(origin.key.as_str());
            }
            tags.extend(fresh_folder_tags(&folder.items));
        }
    }
    tags
}

fn import_folders(items: &[Item], keep: impl Fn(&Folder) -> bool) -> Vec<Uuid> {
    fn walk(items: &[Item], keep: &dyn Fn(&Folder) -> bool, out: &mut Vec<Uuid>) {
        for item in items {
            if let Item::Folder(folder) = item {
                if folder.origin.is_some() && keep(folder) {
                    out.push(folder.id);
                }
                walk(&folder.items, keep, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(items, &keep, &mut out);
    out
}

fn moves_to(tag: Option<&str>, fresh: &Collection) -> String {
    let name = tag.map(|tag| {
        find_import_folder(&fresh.items, tag)
            .map_or(tag, |folder| folder.name.as_str())
            .to_owned()
    });
    match name {
        Some(name) => format!("Moves to “{name}”"),
        None => "Moves to the top level".to_owned(),
    }
}

/// An imported request or folder, by what it came from in the spec.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ItemKey {
    Request(String),
    Folder(String),
}

fn item_key(item: &Item) -> Option<ItemKey> {
    match item {
        Item::Request(saved) => saved
            .origin
            .as_ref()
            .map(|origin| ItemKey::Request(origin.key.clone())),
        Item::Folder(folder) => folder
            .origin
            .as_ref()
            .map(|origin| ItemKey::Folder(origin.key.clone())),
    }
}

/// The order of each container's imported items in the spec: the top level
/// under `None`, a tag folder under its tag.
fn sibling_orders(fresh: &Collection) -> HashMap<Option<String>, Vec<ItemKey>> {
    fn walk(
        items: &[Item],
        container: Option<String>,
        orders: &mut HashMap<Option<String>, Vec<ItemKey>>,
    ) {
        orders.insert(
            container.clone(),
            items.iter().filter_map(item_key).collect(),
        );
        for item in items {
            if let Item::Folder(folder) = item {
                let inner = folder.origin.as_ref().map(|origin| origin.key.clone());
                walk(&folder.items, inner.or(container.clone()), orders);
            }
        }
    }
    let mut orders = HashMap::new();
    walk(&fresh.items, None, &mut orders);
    orders
}

fn find_import_folder<'a>(items: &'a [Item], tag: &str) -> Option<&'a Folder> {
    items.iter().find_map(|item| match item {
        Item::Folder(folder)
            if folder
                .origin
                .as_ref()
                .is_some_and(|origin| origin.key == tag) =>
        {
            Some(folder)
        }
        Item::Folder(folder) => find_import_folder(&folder.items, tag),
        Item::Request(_) => None,
    })
}

/// Puts `item` in its tag's folder, creating the folder if there's none,
/// next to its neighbours in the spec.
fn insert(
    collection: &mut Collection,
    item: Item,
    key: &ItemKey,
    tag: Option<&str>,
    fresh: &Collection,
    orders: &HashMap<Option<String>, Vec<ItemKey>>,
) {
    let no_order = Vec::new();
    let order = |container: Option<&str>| {
        orders
            .get(&container.map(str::to_owned))
            .unwrap_or(&no_order)
    };
    let Some(tag) = tag else {
        insert_ordered(&mut collection.items, item, key, order(None));
        return;
    };
    let folder_id = match find_import_folder(&collection.items, tag) {
        Some(folder) => folder.id,
        None => {
            let Some(template) = find_import_folder(&fresh.items, tag) else {
                insert_ordered(&mut collection.items, item, key, order(None));
                return;
            };
            let mut folder = template.clone();
            folder.items.clear();
            let id = folder.id;
            let folder_key = ItemKey::Folder(tag.to_owned());
            insert_ordered(
                &mut collection.items,
                Item::Folder(folder),
                &folder_key,
                order(None),
            );
            id
        }
    };
    match collection.find_folder_mut(folder_id) {
        Some(folder) => insert_ordered(&mut folder.items, item, key, order(Some(tag))),
        None => collection.items.push(item),
    }
}

/// Inserts after the nearest earlier spec sibling present, else before the
/// nearest later one, else at the end.
fn insert_ordered(items: &mut Vec<Item>, item: Item, key: &ItemKey, order: &[ItemKey]) {
    let index_of = |wanted: &ItemKey| {
        items
            .iter()
            .position(|item| item_key(item).as_ref() == Some(wanted))
    };
    let position = order
        .iter()
        .position(|sibling| sibling == key)
        .and_then(|at| {
            order[..at]
                .iter()
                .rev()
                .find_map(|earlier| index_of(earlier).map(|index| index + 1))
                .or_else(|| order[at + 1..].iter().find_map(index_of))
        })
        .unwrap_or(items.len());
    items.insert(position, item);
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
