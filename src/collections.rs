use std::sync::Arc;

use anyhow::anyhow;
use gpui::{AppContext, Context, EventEmitter};
use uuid::Uuid;

use domain::{
    collection::{self, Collection, Destination, Folder, Item, Location, MovedItems, SavedRequest},
    openapi::update::{self, Choices},
    request::{Auth, KeyValue, Request},
    store::{
        self,
        collection::CollectionStore,
        jobs::{self, SaveQueue},
    },
};

use crate::save_queue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Loaded,
    Failed,
}

pub enum CollectionsEvent {
    Changed,
    /// Sent once, after the initial load finished or failed.
    Loaded,
    /// `title` says what didn't happen, e.g. "Couldn't save collection".
    Failed {
        title: &'static str,
        error: anyhow::Error,
    },
}

/// Owns the saved collections: loads them off the UI thread and saves every
/// change in the background.
///
/// Store jobs run one at a time, starting with the initial load, and only the
/// newest pending save or delete of a collection is kept. So saves of a
/// collection can't land out of order.
pub struct Collections {
    /// Sorted by name, case-insensitively.
    collections: Vec<Arc<Collection>>,
    load_state: LoadState,
    jobs: SaveQueue<CollectionStore>,
}

impl EventEmitter<CollectionsEvent> for Collections {}

impl Collections {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (jobs, queued) = jobs::save_queue();
        cx.spawn(async move |this, cx| {
            let opened = cx.background_spawn(async { open_and_load() }).await;
            let Ok(store) = this.update(cx, |this, cx| this.finish_loading(opened, cx)) else {
                return;
            };
            save_queue::run(this, cx, store, queued, |failure, cx| {
                cx.emit(CollectionsEvent::Failed {
                    title: failure.title,
                    error: failure.error,
                })
            })
            .await;
        })
        .detach();

        Self {
            collections: Vec::new(),
            load_state: LoadState::Loading,
            jobs,
        }
    }

    pub fn all(&self) -> &[Arc<Collection>] {
        &self.collections
    }

    pub fn load_state(&self) -> LoadState {
        self.load_state
    }

    pub fn get(&self, id: Uuid) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id).map(|c| &**c)
    }

    pub fn shared(&self, id: Uuid) -> Option<Arc<Collection>> {
        self.collections.iter().find(|c| c.id == id).cloned()
    }

    pub fn create(&mut self, name: impl Into<String>, cx: &mut Context<Self>) -> Uuid {
        self.add(Collection::new(name), cx)
    }

    /// Adds a whole collection, such as an imported one, and saves it.
    pub fn add(&mut self, collection: Collection, cx: &mut Context<Self>) -> Uuid {
        let id = collection.id;
        self.collections.push(Arc::new(collection));
        self.sort();
        self.changed(id, cx);
        id
    }

    /// Renames the collection itself when `id == collection_id`.
    pub fn rename(
        &mut self,
        collection_id: Uuid,
        id: Uuid,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let name = name.into();
        let renamed = self.modify(collection_id, |collection| {
            if id == collection_id {
                collection.name = name;
                true
            } else {
                collection.rename(id, name)
            }
        });
        if renamed {
            self.sort();
            self.changed(collection_id, cx);
        }
    }

    pub fn add_folder(
        &mut self,
        collection_id: Uuid,
        parent: Option<Uuid>,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let folder = Folder::new(name);
        let id = folder.id;
        self.add_item(collection_id, parent, Item::Folder(folder), cx)
            .then_some(id)
    }

    pub fn add_request(
        &mut self,
        collection_id: Uuid,
        parent: Option<Uuid>,
        name: impl Into<String>,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let saved = SavedRequest::new(name, request);
        let id = saved.id;
        self.add_item(collection_id, parent, Item::Request(saved), cx)
            .then_some(id)
    }

    /// Adds a request at `location` with one save, creating the collection
    /// first for `Location::NewCollection`. Returns the collection and request ids.
    pub fn add_request_at(
        &mut self,
        location: Location,
        name: impl Into<String>,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Option<(Uuid, Uuid)> {
        let (collection_id, request_id) =
            collection::add_request_at(&mut self.collections, location, name, request)?;
        self.sort();
        self.changed(collection_id, cx);
        Some((collection_id, request_id))
    }

    /// Replaces the saved request, and renames it too if `name` is given.
    /// Returns `false` if the request doesn't exist.
    pub fn update_request(
        &mut self,
        collection_id: Uuid,
        request_id: Uuid,
        request: Request,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let updated = self.modify(collection_id, |collection| {
            let Some(saved) = collection.find_request_mut(request_id) else {
                return false;
            };
            saved.request = request;
            if let Some(name) = name {
                saved.name = name;
            }
            true
        });
        if updated {
            self.changed(collection_id, cx);
        }
        updated
    }

    /// Saves only if `variables` differ from the stored ones.
    pub fn set_variables(
        &mut self,
        collection_id: Uuid,
        variables: Vec<KeyValue>,
        cx: &mut Context<Self>,
    ) {
        if collection::set_variables(&mut self.collections, collection_id, variables) {
            self.changed(collection_id, cx);
        }
    }

    pub fn set_auth(&mut self, collection_id: Uuid, auth: Auth, cx: &mut Context<Self>) {
        if collection::set_auth(&mut self.collections, collection_id, auth) {
            self.changed(collection_id, cx);
        }
    }

    pub fn add_environment(&mut self, collection_id: Uuid, cx: &mut Context<Self>) -> Option<Uuid> {
        let id = collection::add_environment(&mut self.collections, collection_id)?;
        self.changed(collection_id, cx);
        Some(id)
    }

    pub fn rename_environment(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let collections = &mut self.collections;
        if collection::rename_environment(collections, collection_id, environment_id, name) {
            self.changed(collection_id, cx);
        }
    }

    pub fn duplicate_environment(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let collections = &mut self.collections;
        let id = collection::duplicate_environment(collections, collection_id, environment_id)?;
        self.changed(collection_id, cx);
        Some(id)
    }

    pub fn delete_environment(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if collection::delete_environment(&mut self.collections, collection_id, environment_id) {
            self.changed(collection_id, cx);
        }
    }

    /// Saves only if `variables` differ from the stored ones.
    pub fn set_environment_variables(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        variables: Vec<KeyValue>,
        cx: &mut Context<Self>,
    ) {
        let collections = &mut self.collections;
        if collection::set_environment_variables(
            collections,
            collection_id,
            environment_id,
            variables,
        ) {
            self.changed(collection_id, cx);
        }
    }

    /// Saves only if `auth` differs from the stored one.
    pub fn set_environment_auth(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        auth: Auth,
        cx: &mut Context<Self>,
    ) {
        let collections = &mut self.collections;
        if collection::set_environment_auth(collections, collection_id, environment_id, auth) {
            self.changed(collection_id, cx);
        }
    }

    /// `base: None` unlinks the environment.
    pub fn set_environment_base(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        base: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        let collections = &mut self.collections;
        if collection::set_environment_base(collections, collection_id, environment_id, base) {
            self.changed(collection_id, cx);
        }
    }

    /// Unlinks every environment based on shared environment `base`.
    pub fn unlink_base(&mut self, base: Uuid, cx: &mut Context<Self>) {
        let changed = collection::unlink_base(&mut self.collections, base);
        if changed.is_empty() {
            return;
        }
        cx.emit(CollectionsEvent::Changed);
        cx.notify();
        for id in changed {
            self.enqueue_save(id, cx);
        }
    }

    /// Updates an imported collection from `fresh`, a new import of its
    /// spec, with one save. Returns `false` if there's no such collection.
    pub fn apply_spec_update(
        &mut self,
        collection_id: Uuid,
        fresh: &Collection,
        choices: &Choices,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(slot) = self.collections.iter_mut().find(|c| c.id == collection_id) else {
            return false;
        };
        *slot = Arc::new(update::apply(slot, fresh, choices));
        self.sort();
        self.changed(collection_id, cx);
        true
    }

    /// Clears the collection's source and origins, so it's no longer updated from a spec.
    pub fn stop_syncing(&mut self, collection_id: Uuid, cx: &mut Context<Self>) {
        if self.modify(collection_id, Collection::stop_syncing) {
            self.changed(collection_id, cx);
        }
    }

    pub fn duplicate_request(
        &mut self,
        collection_id: Uuid,
        request_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let id = collection::duplicate_request(&mut self.collections, collection_id, request_id)?;
        self.changed(collection_id, cx);
        Some(id)
    }

    /// Moves the request to `to`, saving each collection that changed.
    /// Returns `false` if nothing moved.
    pub fn move_request(
        &mut self,
        collection_id: Uuid,
        request_id: Uuid,
        to: Destination,
        cx: &mut Context<Self>,
    ) -> bool {
        if !collection::move_request(&mut self.collections, collection_id, request_id, to) {
            return false;
        }
        self.changed(collection_id, cx);
        if to.collection_id != collection_id {
            self.enqueue_save(to.collection_id, cx);
        }
        true
    }

    /// Moves the folder with everything in it to `to`, saving each collection
    /// that changed. Returns what moved, or `None` if nothing did.
    pub fn move_folder(
        &mut self,
        collection_id: Uuid,
        folder_id: Uuid,
        to: Destination,
        cx: &mut Context<Self>,
    ) -> Option<MovedItems> {
        let moved = collection::move_folder(&mut self.collections, collection_id, folder_id, to)?;
        self.changed(collection_id, cx);
        if to.collection_id != collection_id {
            self.enqueue_save(to.collection_id, cx);
        }
        Some(moved)
    }

    pub fn remove_item(&mut self, collection_id: Uuid, item_id: Uuid, cx: &mut Context<Self>) {
        let removed = self.modify(collection_id, |collection| {
            collection.remove(item_id).is_some()
        });
        if removed {
            self.changed(collection_id, cx);
        }
    }

    pub fn delete(&mut self, collection_id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.collections.iter().position(|c| c.id == collection_id) else {
            return;
        };
        self.collections.remove(index);
        cx.emit(CollectionsEvent::Changed);
        cx.notify();
        self.enqueue_delete(collection_id, cx);
    }

    fn add_item(
        &mut self,
        collection_id: Uuid,
        parent: Option<Uuid>,
        item: Item,
        cx: &mut Context<Self>,
    ) -> bool {
        let added = self.modify(collection_id, |collection| {
            collection.add_item(parent, item)
        });
        if added {
            self.changed(collection_id, cx);
        }
        added
    }

    /// Resolves once every save requested so far has finished or failed, with
    /// the collections whose last save or delete failed.
    pub fn flushed(&self) -> impl Future<Output = Vec<Uuid>> + use<> {
        let flushed = self.jobs.flush();
        async move { flushed.await.unwrap_or_default() }
    }

    /// Saves each of `ids` again, or deletes it again if it's gone.
    pub fn retry(&self, ids: &[Uuid], cx: &mut Context<Self>) {
        for &id in ids {
            if self.get(id).is_some() {
                self.enqueue_save(id, cx);
            } else {
                self.enqueue_delete(id, cx);
            }
        }
    }

    fn modify(&mut self, collection_id: Uuid, f: impl FnOnce(&mut Collection) -> bool) -> bool {
        collection::modify(&mut self.collections, collection_id, f)
    }

    fn changed(&mut self, collection_id: Uuid, cx: &mut Context<Self>) {
        cx.emit(CollectionsEvent::Changed);
        cx.notify();
        self.enqueue_save(collection_id, cx);
    }

    fn enqueue_save(&self, collection_id: Uuid, cx: &mut Context<Self>) {
        let Some(collection) = self.shared(collection_id) else {
            return;
        };
        let title = "Couldn't save collection";
        self.enqueue(collection_id, title, cx, move |store| {
            store.save(&collection)
        });
    }

    fn enqueue_delete(&self, collection_id: Uuid, cx: &mut Context<Self>) {
        let title = "Couldn't delete collection";
        self.enqueue(collection_id, title, cx, move |store| {
            store.delete(collection_id)
        });
    }

    fn enqueue(
        &self,
        collection_id: Uuid,
        title: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&CollectionStore) -> anyhow::Result<()> + Send + 'static,
    ) {
        if !self.jobs.write(collection_id, title, work) {
            cx.emit(CollectionsEvent::Failed {
                title,
                error: anyhow!("collections are unavailable"),
            });
        }
    }

    fn sort(&mut self) {
        self.collections
            .sort_by_cached_key(|collection| collection.name.to_lowercase());
    }

    /// Returns the store if it opened. Collections created while loading are kept.
    fn finish_loading(
        &mut self,
        opened: anyhow::Result<(CollectionStore, anyhow::Result<Vec<Collection>>)>,
        cx: &mut Context<Self>,
    ) -> Option<CollectionStore> {
        let (store, loaded) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                self.load_state = LoadState::Failed;
                cx.emit(CollectionsEvent::Failed {
                    title: "Collections are unavailable",
                    error,
                });
                cx.emit(CollectionsEvent::Loaded);
                return None;
            }
        };

        match loaded {
            Ok(loaded) => {
                self.load_state = LoadState::Loaded;
                self.collections.extend(loaded.into_iter().map(Arc::new));
                self.sort();
                cx.emit(CollectionsEvent::Changed);
                cx.notify();
            }
            Err(error) => {
                self.load_state = LoadState::Failed;
                cx.emit(CollectionsEvent::Failed {
                    title: "Couldn't load collections",
                    error,
                });
            }
        }
        cx.emit(CollectionsEvent::Loaded);
        Some(store)
    }
}

fn open_and_load() -> anyhow::Result<(CollectionStore, anyhow::Result<Vec<Collection>>)> {
    let store = CollectionStore::new(store::data_dir()?.join("collections"))?;
    let loaded = store.load_all();
    Ok((store, loaded))
}
