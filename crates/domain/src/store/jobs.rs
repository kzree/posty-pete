use std::{
    collections::VecDeque,
    mem,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use uuid::Uuid;

type Work<S> = Box<dyn FnOnce(&S) -> anyhow::Result<()> + Send>;

/// A job for a store that runs its jobs one at a time, in order.
pub enum StoreJob<S> {
    /// Saves or deletes item `id`. `title` says what didn't happen if it fails.
    Write {
        id: Uuid,
        title: &'static str,
        work: Work<S>,
    },
    /// Answers with the items whose last write failed, once every job before it ran.
    Flush(oneshot::Sender<Vec<Uuid>>),
}

impl<S> StoreJob<S> {
    pub fn write(
        id: Uuid,
        title: &'static str,
        work: impl FnOnce(&S) -> anyhow::Result<()> + Send + 'static,
    ) -> Self {
        Self::Write {
            id,
            title,
            work: Box::new(work),
        }
    }

    pub fn flush() -> (Self, oneshot::Receiver<Vec<Uuid>>) {
        let (done, flushed) = oneshot::channel();
        (Self::Flush(done), flushed)
    }
}

type Pending<S> = Arc<Mutex<Queue<S>>>;

struct Queue<S> {
    jobs: VecDeque<StoreJob<S>>,
    /// Set once the jobs no longer run.
    closed: bool,
}

/// Creates a queue of store jobs that run one at a time, in order.
pub fn save_queue<S>() -> (SaveQueue<S>, QueuedJobs<S>) {
    let pending = Arc::new(Mutex::new(Queue {
        jobs: VecDeque::new(),
        closed: false,
    }));
    let (wake, woken) = mpsc::unbounded();
    (
        SaveQueue {
            pending: pending.clone(),
            wake,
        },
        QueuedJobs { pending, woken },
    )
}

/// Queues writes and flushes for a store. Only the newest pending write of
/// an item is kept: it takes the place of the older one, so items are written
/// in the order they were first queued and a flush still waits for it.
pub struct SaveQueue<S> {
    pending: Pending<S>,
    wake: mpsc::UnboundedSender<()>,
}

impl<S> SaveQueue<S> {
    /// Queues a save or delete of item `id`. Returns `false` if the jobs no
    /// longer run.
    pub fn write(
        &self,
        id: Uuid,
        title: &'static str,
        work: impl FnOnce(&S) -> anyhow::Result<()> + Send + 'static,
    ) -> bool {
        let job = StoreJob::write(id, title, work);
        let mut pending = lock(&self.pending);
        if pending.closed {
            drop(pending);
            return false;
        }
        let queued = pending
            .jobs
            .iter_mut()
            .find(|queued| matches!(queued, StoreJob::Write { id: queued, .. } if *queued == id));
        match queued {
            Some(queued) => {
                let replaced = mem::replace(queued, job);
                drop(pending);
                drop(replaced);
            }
            None => {
                pending.jobs.push_back(job);
                drop(pending);
                self.wake.unbounded_send(()).ok();
            }
        }
        true
    }

    /// Resolves with the items whose last write failed, once every write
    /// queued before it ran. Resolves with an error if the jobs no longer run.
    pub fn flush(&self) -> oneshot::Receiver<Vec<Uuid>> {
        let (job, flushed) = StoreJob::flush();
        let mut pending = lock(&self.pending);
        if pending.closed {
            drop(pending);
            drop(job);
        } else {
            pending.jobs.push_back(job);
            drop(pending);
            self.wake.unbounded_send(()).ok();
        }
        flushed
    }
}

/// The receiving end of a [`SaveQueue`].
pub struct QueuedJobs<S> {
    pending: Pending<S>,
    woken: mpsc::UnboundedReceiver<()>,
}

impl<S> QueuedJobs<S> {
    /// The next job, or `None` once the [`SaveQueue`] is dropped and drained.
    pub async fn next(&mut self) -> Option<StoreJob<S>> {
        loop {
            self.woken.next().await?;
            if let Some(job) = lock(&self.pending).jobs.pop_front() {
                return Some(job);
            }
        }
    }
}

impl<S> Drop for QueuedJobs<S> {
    fn drop(&mut self) {
        let jobs = {
            let mut pending = lock(&self.pending);
            pending.closed = true;
            mem::take(&mut pending.jobs)
        };
        drop(jobs);
    }
}

fn lock<S>(pending: &Pending<S>) -> MutexGuard<'_, Queue<S>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct WriteFailure {
    pub title: &'static str,
    pub error: anyhow::Error,
}

/// The items whose last write failed. A later successful write of the same
/// item clears it.
#[derive(Debug, Default)]
pub struct FailedWrites {
    ids: Vec<Uuid>,
}

impl FailedWrites {
    /// Runs `job` against `store`, which is `None` if it couldn't be opened.
    pub fn run<S>(&mut self, store: Option<&S>, job: StoreJob<S>) -> Option<WriteFailure> {
        match job {
            StoreJob::Write { id, title, work } => {
                let result = match store {
                    Some(store) => work(store),
                    None => Err(anyhow::anyhow!("the data folder couldn't be opened")),
                };
                self.ids.retain(|failed| *failed != id);
                let error = result.err()?;
                self.ids.push(id);
                Some(WriteFailure { title, error })
            }
            StoreJob::Flush(done) => {
                done.send(self.ids.clone()).ok();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Store {
        fail: Cell<bool>,
    }

    fn write(id: Uuid) -> StoreJob<Store> {
        StoreJob::write(id, "Couldn't save", |store: &Store| {
            if store.fail.get() {
                anyhow::bail!("disk full")
            }
            Ok(())
        })
    }

    fn flush(failed: &mut FailedWrites, store: Option<&Store>) -> Vec<Uuid> {
        let (job, mut flushed) = StoreJob::flush();
        assert!(failed.run(store, job).is_none());
        flushed.try_recv().unwrap().unwrap()
    }

    #[test]
    fn flush_reports_a_failed_write() {
        let store = Store {
            fail: Cell::new(true),
        };
        let mut failed = FailedWrites::default();
        let id = Uuid::new_v4();
        let failure = failed.run(Some(&store), write(id)).unwrap();
        assert_eq!(failure.title, "Couldn't save");
        assert_eq!(flush(&mut failed, Some(&store)), vec![id]);
    }

    #[test]
    fn a_later_successful_write_clears_the_failure() {
        let store = Store {
            fail: Cell::new(true),
        };
        let mut failed = FailedWrites::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        failed.run(Some(&store), write(a));
        failed.run(Some(&store), write(b));
        failed.run(Some(&store), write(a));
        store.fail.set(false);
        assert!(failed.run(Some(&store), write(a)).is_none());
        assert_eq!(flush(&mut failed, Some(&store)), vec![b]);
    }

    #[test]
    fn successful_writes_flush_empty() {
        let store = Store {
            fail: Cell::new(false),
        };
        let mut failed = FailedWrites::default();
        failed.run(Some(&store), write(Uuid::new_v4()));
        assert!(flush(&mut failed, Some(&store)).is_empty());
    }

    #[test]
    fn writes_fail_without_a_store() {
        let mut failed = FailedWrites::default();
        let id = Uuid::new_v4();
        assert!(failed.run(None, write(id)).is_some());
        assert_eq!(flush(&mut failed, None), vec![id]);
    }

    mod queue {
        use std::collections::HashMap;

        use futures::FutureExt;

        use super::super::*;

        #[derive(Default)]
        struct Files {
            fail: bool,
            saved: Mutex<HashMap<Uuid, &'static str>>,
            writes: Mutex<Vec<(Uuid, &'static str)>>,
        }

        impl Files {
            fn record(&self, id: Uuid, value: &'static str) -> anyhow::Result<()> {
                self.writes.lock().unwrap().push((id, value));
                if self.fail {
                    anyhow::bail!("disk full")
                }
                Ok(())
            }

            fn has(&self, id: Uuid) -> Option<&'static str> {
                self.saved.lock().unwrap().get(&id).copied()
            }

            fn writes(&self) -> Vec<(Uuid, &'static str)> {
                self.writes.lock().unwrap().clone()
            }
        }

        fn save(queue: &SaveQueue<Files>, id: Uuid, value: &'static str) {
            assert!(queue.write(id, "Couldn't save", move |files: &Files| {
                files.record(id, value)?;
                files.saved.lock().unwrap().insert(id, value);
                Ok(())
            }));
        }

        fn delete(queue: &SaveQueue<Files>, id: Uuid) {
            assert!(queue.write(id, "Couldn't delete", move |files: &Files| {
                files.record(id, "deleted")?;
                files.saved.lock().unwrap().remove(&id);
                Ok(())
            }));
        }

        /// Runs every queued job, like the store's job loop does.
        fn run_all(jobs: &mut QueuedJobs<Files>, files: &Files, failed: &mut FailedWrites) {
            while let Some(Some(job)) = jobs.next().now_or_never() {
                failed.run(Some(files), job);
            }
        }

        #[test]
        fn only_the_newest_pending_save_is_written() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let id = Uuid::new_v4();
            save(&queue, id, "a");
            save(&queue, id, "b");
            save(&queue, id, "c");
            run_all(&mut jobs, &files, &mut FailedWrites::default());
            assert_eq!(files.writes(), [(id, "c")]);
            assert_eq!(files.has(id), Some("c"));
        }

        #[test]
        fn save_then_delete_does_not_recreate_the_item() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let id = Uuid::new_v4();
            save(&queue, id, "a");
            delete(&queue, id);
            run_all(&mut jobs, &files, &mut FailedWrites::default());
            assert_eq!(files.has(id), None);
            assert_eq!(files.writes(), [(id, "deleted")]);
        }

        #[test]
        fn delete_then_save_recreates_the_item() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let mut failed = FailedWrites::default();
            let id = Uuid::new_v4();
            save(&queue, id, "a");
            run_all(&mut jobs, &files, &mut failed);
            delete(&queue, id);
            save(&queue, id, "b");
            run_all(&mut jobs, &files, &mut failed);
            assert_eq!(files.has(id), Some("b"));

            delete(&queue, id);
            run_all(&mut jobs, &files, &mut failed);
            save(&queue, id, "c");
            run_all(&mut jobs, &files, &mut failed);
            assert_eq!(files.has(id), Some("c"));
        }

        #[test]
        fn items_are_written_in_the_order_first_queued() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
            save(&queue, a, "a1");
            save(&queue, b, "b1");
            save(&queue, a, "a2");
            run_all(&mut jobs, &files, &mut FailedWrites::default());
            assert_eq!(files.writes(), [(a, "a2"), (b, "b1")]);
        }

        #[test]
        fn a_running_save_is_followed_by_the_newer_one() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let mut failed = FailedWrites::default();
            let id = Uuid::new_v4();
            save(&queue, id, "a");
            let running = jobs.next().now_or_never().flatten().unwrap();
            save(&queue, id, "b");
            failed.run(Some(&files), running);
            run_all(&mut jobs, &files, &mut failed);
            assert_eq!(files.writes(), [(id, "a"), (id, "b")]);
        }

        #[test]
        fn flush_waits_for_pending_writes() {
            let (queue, mut jobs) = save_queue();
            let files = Files::default();
            let mut failed = FailedWrites::default();
            let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
            save(&queue, a, "a1");
            let mut flushed = queue.flush();
            save(&queue, a, "a2");
            save(&queue, b, "b1");

            let first = jobs.next().now_or_never().flatten().unwrap();
            assert!(flushed.try_recv().unwrap().is_none());
            failed.run(Some(&files), first);
            assert!(flushed.try_recv().unwrap().is_none());
            let barrier = jobs.next().now_or_never().flatten().unwrap();
            failed.run(Some(&files), barrier);
            assert_eq!(flushed.try_recv().unwrap(), Some(Vec::new()));
            assert_eq!(files.writes(), [(a, "a2")]);

            run_all(&mut jobs, &files, &mut failed);
            assert_eq!(files.writes(), [(a, "a2"), (b, "b1")]);
        }

        #[test]
        fn flush_reports_a_failed_coalesced_write() {
            let (queue, mut jobs) = save_queue();
            let files = Files {
                fail: true,
                ..Files::default()
            };
            let id = Uuid::new_v4();
            save(&queue, id, "a");
            save(&queue, id, "b");
            let mut flushed = queue.flush();
            let mut failed = FailedWrites::default();
            let mut failures = Vec::new();
            while let Some(Some(job)) = jobs.next().now_or_never() {
                failures.extend(failed.run(Some(&files), job));
            }
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].title, "Couldn't save");
            assert_eq!(flushed.try_recv().unwrap(), Some(vec![id]));
        }

        #[test]
        fn a_closed_queue_refuses_writes_and_resolves_flushes() {
            let (queue, jobs) = save_queue::<Files>();
            let id = Uuid::new_v4();
            assert!(queue.write(id, "Couldn't save", |_| Ok(())));
            let mut queued_flush = queue.flush();
            drop(jobs);
            assert!(queued_flush.try_recv().is_err());
            assert!(!queue.write(id, "Couldn't save", |_| Ok(())));
            assert!(queue.flush().try_recv().is_err());
        }

        #[test]
        fn jobs_end_once_the_queue_is_dropped_and_drained() {
            let (queue, mut jobs) = save_queue();
            save(&queue, Uuid::new_v4(), "a");
            drop(queue);
            assert!(jobs.next().now_or_never().flatten().is_some());
            assert!(matches!(jobs.next().now_or_never(), Some(None)));
        }
    }
}
