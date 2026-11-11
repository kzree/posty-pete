use std::sync::Arc;

use anyhow::anyhow;
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{AppContext, Context, EventEmitter};
use uuid::Uuid;

use crate::domain::{
    environment::{self, Environment},
    request::KeyValue,
    store::{self, environment::EnvironmentStore},
};

pub enum SharedEnvironmentsEvent {
    Changed,
    /// `title` says what didn't happen, e.g. "Couldn't save environment".
    Failed {
        title: &'static str,
        error: anyhow::Error,
    },
}

/// Owns the shared environments: loads them off the UI thread and saves
/// every change in the background, one store job at a time and in order.
pub struct SharedEnvironments {
    /// Sorted by name, case-insensitively.
    environments: Vec<Arc<Environment>>,
    loaded: bool,
    jobs: mpsc::UnboundedSender<Job>,
}

type Job = Box<dyn FnOnce(&EnvironmentStore) -> Option<SharedEnvironmentsEvent> + Send>;

impl EventEmitter<SharedEnvironmentsEvent> for SharedEnvironments {}

impl SharedEnvironments {
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
}

#[expect(dead_code, reason = "used by the shared environments page, task 56")]
impl SharedEnvironments {
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

    /// Collections with environments based on it are left to the caller.
    pub fn delete(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if !environment::delete(&mut self.environments, id) {
            return;
        }
        cx.emit(SharedEnvironmentsEvent::Changed);
        cx.notify();
        self.enqueue("Couldn't delete environment", cx, move |store| {
            store.delete(id)
        });
    }

    /// Saves only if `variables` differ from the stored ones.
    pub fn set_variables(&mut self, id: Uuid, variables: Vec<KeyValue>, cx: &mut Context<Self>) {
        if environment::set_variables(&mut self.environments, id, variables) {
            self.changed(id, cx);
        }
    }
}

impl SharedEnvironments {
    fn changed(&mut self, id: Uuid, cx: &mut Context<Self>) {
        cx.emit(SharedEnvironmentsEvent::Changed);
        cx.notify();
        let Some(environment) = self.environments.iter().find(|env| env.id == id).cloned() else {
            return;
        };
        self.enqueue("Couldn't save environment", cx, move |store| {
            store.save(&environment)
        });
    }

    fn enqueue(
        &self,
        title: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&EnvironmentStore) -> anyhow::Result<()> + Send + 'static,
    ) {
        let job: Job = Box::new(move |store| {
            work(store)
                .err()
                .map(|error| SharedEnvironmentsEvent::Failed { title, error })
        });
        if self.jobs.unbounded_send(job).is_err() {
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
