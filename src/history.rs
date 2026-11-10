use std::{
    mem,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use anyhow::Context as _;
use chrono::Local;
use gpui::{App, AppContext, Context, EventEmitter, Task};

use crate::domain::{
    request::Request,
    store::history::{self, HistoryRecord, HistoryStore, HistorySummary},
};

pub enum HistoryEvent {
    /// Sent once at startup. Always emitted before any `Recorded`.
    Loaded(Vec<HistorySummary>),
    Recorded(HistorySummary),
    Opened {
        request: Request,
        mode: OpenMode,
    },
    Deleted(i64),
    Cleared,
}

/// What to do with a history entry once its full request is loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Load,
    Send,
}

/// Owns the history database: opens it off the UI thread, prunes and loads it,
/// and saves sent requests.
pub struct History {
    state: State,
}

enum State {
    /// Records that arrive before the database is open are saved once it is.
    Opening {
        queued: Vec<HistoryRecord>,
    },
    Ready(Arc<Mutex<HistoryStore>>),
    Unavailable,
}

impl EventEmitter<HistoryEvent> for History {}

impl History {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let opened = cx.background_spawn(async { open_and_load() }).await;
            this.update(cx, |this, cx| this.finish_opening(opened, cx))
                .ok();
        })
        .detach();

        Self {
            state: State::Opening { queued: Vec::new() },
        }
    }

    pub fn open(&mut self, id: i64, mode: OpenMode, cx: &mut Context<Self>) {
        let request = self.request(id, cx);
        cx.spawn(async move |this, cx| {
            if let Some(request) = request.await {
                this.update(cx, |_, cx| cx.emit(HistoryEvent::Opened { request, mode }))
                    .ok();
            }
        })
        .detach();
    }

    /// Loads an entry's full request. Failures are logged and give `None`.
    pub fn request(&self, id: i64, cx: &App) -> Task<Option<Request>> {
        let State::Ready(store) = &self.state else {
            return Task::ready(None);
        };
        let store = store.clone();

        cx.background_spawn(async move {
            match lock(&store).get(id) {
                Ok(Some(request)) => Some(request),
                Ok(None) => {
                    tracing::warn!("history entry {id} not found");
                    None
                }
                Err(e) => {
                    tracing::warn!("failed to get request from history: {e:#}");
                    None
                }
            }
        })
    }

    pub fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        let State::Ready(store) = &self.state else {
            return;
        };
        let store = store.clone();

        cx.spawn(async move |this, cx| {
            let deleted = cx
                .background_spawn(async move { lock(&store).delete(id) })
                .await;
            match deleted {
                Ok(true) => {
                    this.update(cx, |_, cx| cx.emit(HistoryEvent::Deleted(id)))
                        .ok();
                }
                Ok(false) => tracing::warn!("history entry {id} not found"),
                Err(e) => tracing::warn!("failed to delete history entry: {e:#}"),
            }
        })
        .detach();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        let State::Ready(store) = &self.state else {
            return;
        };
        let store = store.clone();

        cx.spawn(async move |this, cx| {
            let cleared = cx
                .background_spawn(async move { lock(&store).clear() })
                .await;
            match cleared {
                Ok(_) => {
                    this.update(cx, |_, cx| cx.emit(HistoryEvent::Cleared)).ok();
                }
                Err(e) => tracing::warn!("failed to clear history: {e:#}"),
            }
        })
        .detach();
    }

    pub fn record(&mut self, record: HistoryRecord, cx: &mut Context<Self>) {
        let store = match &mut self.state {
            State::Opening { queued } => {
                queued.push(record);
                return;
            }
            State::Ready(store) => store.clone(),
            State::Unavailable => return,
        };

        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn(async move { lock(&store).insert(&record) })
                .await;
            match saved {
                Ok(summary) => {
                    this.update(cx, |_, cx| cx.emit(HistoryEvent::Recorded(summary)))
                        .ok();
                }
                Err(e) => tracing::warn!("failed to save request to history: {e:#}"),
            }
        })
        .detach();
    }

    fn finish_opening(
        &mut self,
        opened: anyhow::Result<(HistoryStore, Vec<HistorySummary>)>,
        cx: &mut Context<Self>,
    ) {
        let queued = match &mut self.state {
            State::Opening { queued } => mem::take(queued),
            State::Ready(_) | State::Unavailable => Vec::new(),
        };

        match opened {
            Ok((store, entries)) => {
                self.state = State::Ready(Arc::new(Mutex::new(store)));
                cx.emit(HistoryEvent::Loaded(entries));
                for record in queued {
                    self.record(record, cx);
                }
            }
            Err(e) => {
                tracing::error!("history is unavailable: {e:#}");
                self.state = State::Unavailable;
            }
        }
    }
}

fn open_and_load() -> anyhow::Result<(HistoryStore, Vec<HistorySummary>)> {
    let dirs =
        directories::ProjectDirs::from("", "", "posty-pete").context("no home directory found")?;
    let path = dirs.data_dir().join("history.db");
    let store =
        HistoryStore::open(&path).with_context(|| format!("failed to open {}", path.display()))?;

    let cutoff = history::retention_cutoff(&Local::now());
    if let Err(e) = store.prune(cutoff) {
        tracing::warn!("failed to prune history: {e:#}");
    }
    let entries = store.since(cutoff).unwrap_or_else(|e| {
        tracing::error!("failed to load history: {e:#}");
        Vec::new()
    });
    Ok((store, entries))
}

fn lock(store: &Mutex<HistoryStore>) -> MutexGuard<'_, HistoryStore> {
    store.lock().unwrap_or_else(PoisonError::into_inner)
}
