use futures::channel::oneshot;
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
}
