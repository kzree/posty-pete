use domain::store::jobs::{FailedWrites, QueuedJobs, WriteFailure};
use gpui::{AppContext as _, AsyncApp, Context, WeakEntity};

/// Runs the queued store jobs one at a time in the background until the
/// queue or `this` is dropped, reporting each failed write through `report`.
/// `store` is `None` if it couldn't be opened, so every write fails.
pub async fn run<T: 'static, S: Send + 'static>(
    this: WeakEntity<T>,
    cx: &mut AsyncApp,
    mut store: Option<S>,
    mut queued: QueuedJobs<S>,
    report: impl Fn(WriteFailure, &mut Context<T>),
) {
    let mut failed = FailedWrites::default();
    while let Some(job) = queued.next().await {
        let failure;
        (store, failed, failure) = cx
            .background_spawn(async move {
                let failure = failed.run(store.as_ref(), job);
                (store, failed, failure)
            })
            .await;
        if let Some(failure) = failure
            && this.update(cx, |_, cx| report(failure, cx)).is_err()
        {
            break;
        }
    }
}
