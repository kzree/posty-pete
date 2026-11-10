use anyhow::{Context as _, anyhow};
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{AppContext, Context, EventEmitter};
use uuid::Uuid;

use crate::domain::{
    collection::{self, Collection, Folder, Item, Location, SavedRequest},
    request::Request,
    store::collection::CollectionStore,
};

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
/// Store jobs run one at a time, in the order they were requested, starting
/// with the initial load. So saves of a collection can't land out of order.
pub struct Collections {
    /// Sorted by name, case-insensitively.
    collections: Vec<Collection>,
    load_state: LoadState,
    jobs: mpsc::UnboundedSender<Job>,
}

type Job = Box<dyn FnOnce(&CollectionStore) -> Option<CollectionsEvent> + Send>;

impl EventEmitter<CollectionsEvent> for Collections {}

impl Collections {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (jobs, mut queued) = mpsc::unbounded::<Job>();
        cx.spawn(async move |this, cx| {
            let opened = cx.background_spawn(async { open_and_load() }).await;
            let Ok(Some(mut store)) = this.update(cx, |this, cx| this.finish_loading(opened, cx))
            else {
                return;
            };
            while let Some(job) = queued.next().await {
                let (returned, event) = cx
                    .background_spawn(async move {
                        let event = job(&store);
                        (store, event)
                    })
                    .await;
                store = returned;
                if let Some(event) = event
                    && this.update(cx, |_, cx| cx.emit(event)).is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            collections: Vec::new(),
            load_state: LoadState::Loading,
            jobs,
        }
    }

    pub fn all(&self) -> &[Collection] {
        &self.collections
    }

    pub fn load_state(&self) -> LoadState {
        self.load_state
    }

    pub fn get(&self, id: Uuid) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    pub fn create(&mut self, name: impl Into<String>, cx: &mut Context<Self>) -> Uuid {
        let collection = Collection::new(name);
        let id = collection.id;
        self.collections.push(collection);
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
        self.enqueue("Couldn't delete collection", cx, move |store| {
            store.delete(collection_id)
        });
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

    /// Resolves once every save requested so far has finished or failed.
    pub fn flushed(&self) -> impl Future<Output = ()> + use<> {
        let (done, flushed) = oneshot::channel();
        let job: Job = Box::new(move |_| {
            done.send(()).ok();
            None
        });
        // If the queue is gone, `done` is dropped and `flushed` resolves too.
        self.jobs.unbounded_send(job).ok();
        async move {
            flushed.await.ok();
        }
    }

    /// Runs `f` on the collection; returns what it returns, or `false` if there's no such collection.
    fn modify(&mut self, collection_id: Uuid, f: impl FnOnce(&mut Collection) -> bool) -> bool {
        self.collections
            .iter_mut()
            .find(|c| c.id == collection_id)
            .is_some_and(f)
    }

    fn changed(&mut self, collection_id: Uuid, cx: &mut Context<Self>) {
        cx.emit(CollectionsEvent::Changed);
        cx.notify();
        let Some(collection) = self.get(collection_id).cloned() else {
            return;
        };
        self.enqueue("Couldn't save collection", cx, move |store| {
            store.save(&collection)
        });
    }

    fn enqueue(
        &self,
        title: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&CollectionStore) -> anyhow::Result<()> + Send + 'static,
    ) {
        let job: Job = Box::new(move |store| {
            work(store)
                .err()
                .map(|error| CollectionsEvent::Failed { title, error })
        });
        if self.jobs.unbounded_send(job).is_err() {
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
                self.collections.extend(loaded);
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
    let dirs =
        directories::ProjectDirs::from("", "", "posty-pete").context("no home directory found")?;
    let store = CollectionStore::new(dirs.data_dir().join("collections"))?;
    let loaded = store.load_all();
    Ok((store, loaded))
}
