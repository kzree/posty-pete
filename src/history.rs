use std::mem;

use anyhow::{Context as _, anyhow};
use chrono::Local;
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{App, AppContext, Context, EventEmitter, Task};

use crate::domain::{
    history::{self, HistoryRecord, HistorySummary, OpenMode},
    request::Request,
    store::{self, history::HistoryStore},
};

pub enum HistoryEvent {
    /// Sent once at startup. Always emitted before any `Recorded`.
    /// `load_failed` means older entries couldn't be read, though new ones
    /// are still recorded.
    Loaded {
        entries: Vec<HistorySummary>,
        load_failed: bool,
    },
    Recorded(HistorySummary),
    Opened {
        request: Request,
        mode: OpenMode,
    },
    Deleted(i64),
    Cleared,
    /// `title` says what didn't happen, e.g. "Couldn't clear history".
    Failed {
        title: &'static str,
        error: anyhow::Error,
    },
}

/// Owns the history database: opens it off the UI thread, prunes and loads it,
/// and saves sent requests.
///
/// Database jobs run one at a time, in the order they were requested, and each
/// job's event is emitted before the next starts. So events always match the
/// database, e.g. a save can't land after a later Clear.
pub struct History {
    state: State,
}

type Job = Box<dyn FnOnce(&HistoryStore) -> Option<HistoryEvent> + Send>;

enum State {
    /// Records that arrive before the database is open are saved once it is.
    Opening {
        queued: Vec<HistoryRecord>,
    },
    Ready(mpsc::UnboundedSender<Job>),
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
        self.run(
            cx,
            move |store| store.get(id),
            move |result| match result {
                Ok(Some(request)) => HistoryEvent::Opened { request, mode },
                Ok(None) => HistoryEvent::Deleted(id),
                Err(error) => HistoryEvent::Failed {
                    title: "Couldn't open request",
                    error,
                },
            },
        );
    }

    /// Loads an entry's full request; `None` if it's no longer in history.
    pub fn request(&self, id: i64, cx: &App) -> Task<anyhow::Result<Option<Request>>> {
        let (sender, receiver) = oneshot::channel();
        let queued = self.enqueue(Box::new(move |store| {
            sender.send(store.get(id)).ok();
            None
        }));
        if !queued {
            return Task::ready(Err(unavailable()));
        }
        cx.background_spawn(async move { receiver.await.unwrap_or_else(|_| Err(unavailable())) })
    }

    pub fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |store| store.delete(id),
            move |result| match result {
                // Already gone counts as deleted, so the panel drops the row.
                Ok(_) => HistoryEvent::Deleted(id),
                Err(error) => HistoryEvent::Failed {
                    title: "Couldn't delete request",
                    error,
                },
            },
        );
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |store| store.clear(),
            |result| match result {
                Ok(_) => HistoryEvent::Cleared,
                Err(error) => HistoryEvent::Failed {
                    title: "Couldn't clear history",
                    error,
                },
            },
        );
    }

    /// Records sent while the database opens are saved once it's ready.
    pub fn record(&mut self, record: HistoryRecord, cx: &mut Context<Self>) {
        match &mut self.state {
            State::Opening { queued } => queued.push(record),
            // Already reported once when opening failed.
            State::Unavailable => {}
            State::Ready(_) => self.run(
                cx,
                move |store| store.insert(&record),
                |result| match result {
                    Ok(summary) => HistoryEvent::Recorded(summary),
                    Err(error) => HistoryEvent::Failed {
                        title: "Couldn't save request to history",
                        error,
                    },
                },
            ),
        }
    }

    /// Queues `work` and emits the event `to_event` makes from its result.
    fn run<T: 'static>(
        &self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&HistoryStore) -> anyhow::Result<T> + Send + 'static,
        to_event: impl FnOnce(anyhow::Result<T>) -> HistoryEvent + Send + 'static,
    ) {
        if matches!(self.state, State::Ready(_)) {
            self.enqueue(Box::new(move |store| Some(to_event(work(store)))));
        } else {
            cx.emit(to_event(Err(unavailable())));
        }
    }

    /// Returns `false` if the database isn't open.
    fn enqueue(&self, job: Job) -> bool {
        let State::Ready(jobs) = &self.state else {
            return false;
        };
        jobs.unbounded_send(job).is_ok()
    }

    fn start_worker(store: HistoryStore, cx: &mut Context<Self>) -> mpsc::UnboundedSender<Job> {
        let (sender, mut jobs) = mpsc::unbounded::<Job>();
        cx.spawn(async move |this, cx| {
            let mut store = store;
            while let Some(job) = jobs.next().await {
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
        sender
    }

    fn finish_opening(
        &mut self,
        opened: anyhow::Result<(HistoryStore, anyhow::Result<Vec<HistorySummary>>)>,
        cx: &mut Context<Self>,
    ) {
        let queued = match &mut self.state {
            State::Opening { queued } => mem::take(queued),
            State::Ready(_) | State::Unavailable => Vec::new(),
        };

        match opened {
            Ok((store, loaded)) => {
                self.state = State::Ready(Self::start_worker(store, cx));
                let load_failed = loaded.is_err();
                let entries = loaded.unwrap_or_else(|e| {
                    tracing::error!("failed to load history: {e:#}");
                    Vec::new()
                });
                cx.emit(HistoryEvent::Loaded {
                    entries,
                    load_failed,
                });
                for record in queued {
                    self.record(record, cx);
                }
            }
            Err(error) => {
                self.state = State::Unavailable;
                cx.emit(HistoryEvent::Failed {
                    title: "History is unavailable",
                    error,
                });
            }
        }
    }
}

/// Fails only if the database can't be opened; a failed load still returns the store.
fn open_and_load() -> anyhow::Result<(HistoryStore, anyhow::Result<Vec<HistorySummary>>)> {
    let path = store::data_dir()?.join("history.db");
    let store =
        HistoryStore::open(&path).with_context(|| format!("failed to open {}", path.display()))?;

    let entries = store.prune_and_load(history::retention_cutoff(&Local::now()));
    Ok((store, entries))
}

fn unavailable() -> anyhow::Error {
    anyhow!("history is unavailable")
}
