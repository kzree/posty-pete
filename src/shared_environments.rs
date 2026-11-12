use std::sync::Arc;

use anyhow::anyhow;
use gpui::{AppContext, Context, EventEmitter};
use uuid::Uuid;

use domain::{
    environment::{self, Environment},
    request::{Auth, KeyValue},
    store::{
        self,
        environment::EnvironmentStore,
        jobs::{self, SaveQueue},
    },
};

use crate::save_queue;

pub enum SharedEnvironmentsEvent {
    Changed,
    /// `title` says what didn't happen, e.g. "Couldn't save environment".
    Failed {
        title: &'static str,
        error: anyhow::Error,
    },
}

/// Owns the shared environments: loads them off the UI thread and saves
/// every change in the background, one store job at a time, keeping only the
/// newest pending save or delete of each environment.
pub struct SharedEnvironments {
    /// Sorted by name, case-insensitively.
    environments: Vec<Arc<Environment>>,
    loaded: bool,
    jobs: SaveQueue<EnvironmentStore>,
}

impl EventEmitter<SharedEnvironmentsEvent> for SharedEnvironments {}

impl SharedEnvironments {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (jobs, queued) = jobs::save_queue();
        cx.spawn(async move |this, cx| {
            let opened = cx.background_spawn(async { open_and_load() }).await;
            let Ok(store) = this.update(cx, |this, cx| this.finish_loading(opened, cx)) else {
                return;
            };
            save_queue::run(this, cx, store, queued, |failure, cx| {
                cx.emit(SharedEnvironmentsEvent::Failed {
                    title: failure.title,
                    error: failure.error,
                })
            })
            .await;
        })
        .detach();

        Self {
            environments: Vec::new(),
            loaded: false,
            jobs,
        }
    }

    pub fn all(&self) -> &[Arc<Environment>] {
        &self.environments
    }

    /// Whether the stored environments loaded, so [`Self::all`] is complete.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get(&self, id: Uuid) -> Option<&Environment> {
        self.environments
            .iter()
            .find(|env| env.id == id)
            .map(|env| &**env)
    }

    /// Resolves once every save requested so far has finished or failed, with
    /// the environments whose last save or delete failed.
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

    pub fn add(&mut self, cx: &mut Context<Self>) -> Uuid {
        let id = environment::add(&mut self.environments);
        self.sort();
        self.changed(id, cx);
        id
    }

    pub fn rename(&mut self, id: Uuid, name: impl Into<String>, cx: &mut Context<Self>) {
        if environment::rename(&mut self.environments, id, name) {
            self.sort();
            self.changed(id, cx);
        }
    }

    pub fn duplicate(&mut self, id: Uuid, cx: &mut Context<Self>) -> Option<Uuid> {
        let copy = environment::duplicate(&mut self.environments, id)?;
        self.sort();
        self.changed(copy, cx);
        Some(copy)
    }

    /// Collections with environments based on it are left to the caller.
    pub fn delete(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if !environment::delete(&mut self.environments, id) {
            return;
        }
        cx.emit(SharedEnvironmentsEvent::Changed);
        cx.notify();
        self.enqueue_delete(id, cx);
    }

    /// Saves only if `variables` differ from the stored ones.
    pub fn set_variables(&mut self, id: Uuid, variables: Vec<KeyValue>, cx: &mut Context<Self>) {
        if environment::set_variables(&mut self.environments, id, variables) {
            self.changed(id, cx);
        }
    }

    /// Saves only if `auth` differs from the stored one.
    pub fn set_auth(&mut self, id: Uuid, auth: Auth, cx: &mut Context<Self>) {
        if environment::set_auth(&mut self.environments, id, auth) {
            self.changed(id, cx);
        }
    }
}

impl SharedEnvironments {
    fn changed(&mut self, id: Uuid, cx: &mut Context<Self>) {
        cx.emit(SharedEnvironmentsEvent::Changed);
        cx.notify();
        self.enqueue_save(id, cx);
    }

    fn enqueue_save(&self, id: Uuid, cx: &mut Context<Self>) {
        let Some(environment) = self.environments.iter().find(|env| env.id == id).cloned() else {
            return;
        };
        self.enqueue(id, "Couldn't save environment", cx, move |store| {
            store.save(&environment)
        });
    }

    fn enqueue_delete(&self, id: Uuid, cx: &mut Context<Self>) {
        self.enqueue(id, "Couldn't delete environment", cx, move |store| {
            store.delete(id)
        });
    }

    fn enqueue(
        &self,
        id: Uuid,
        title: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&EnvironmentStore) -> anyhow::Result<()> + Send + 'static,
    ) {
        if !self.jobs.write(id, title, work) {
            cx.emit(SharedEnvironmentsEvent::Failed {
                title,
                error: anyhow!("shared environments are unavailable"),
            });
        }
    }

    fn sort(&mut self) {
        self.environments
            .sort_by_cached_key(|environment| environment.name.to_lowercase());
    }

    /// Returns the store if it opened. Environments added while loading are kept.
    fn finish_loading(
        &mut self,
        opened: anyhow::Result<(EnvironmentStore, anyhow::Result<Vec<Environment>>)>,
        cx: &mut Context<Self>,
    ) -> Option<EnvironmentStore> {
        let (store, loaded) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                cx.emit(SharedEnvironmentsEvent::Failed {
                    title: "Shared environments are unavailable",
                    error,
                });
                return None;
            }
        };
        match loaded {
            Ok(loaded) => {
                self.environments.extend(loaded.into_iter().map(Arc::new));
                self.sort();
                self.loaded = true;
                cx.emit(SharedEnvironmentsEvent::Changed);
                cx.notify();
            }
            Err(error) => cx.emit(SharedEnvironmentsEvent::Failed {
                title: "Couldn't load shared environments",
                error,
            }),
        }
        Some(store)
    }
}

fn open_and_load() -> anyhow::Result<(EnvironmentStore, anyhow::Result<Vec<Environment>>)> {
    let store = EnvironmentStore::new(store::data_dir()?.join("environments"))?;
    let loaded = store.load_all();
    Ok((store, loaded))
}
