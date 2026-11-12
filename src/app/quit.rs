use std::time::Duration;

use futures::future;
use gpui::{App, Context, FutureExt as _, Window};
use uuid::Uuid;

use super::Workspace;
use crate::{
    actions::Quit,
    ui::retry_dialog::{RetryChoice, RetryDialog, open_retry_dialog},
};

/// How long closing waits for queued collection and shared environment saves.
const SAVE_TIMEOUT: Duration = Duration::from_secs(2);

/// How many unsaved items the dialog names before "and N more".
const NAMED_UNSAVED: usize = 3;

#[derive(Clone, Copy)]
enum Close {
    Window,
    App,
}

/// What closing found not saved.
enum Unsaved {
    /// The collections and shared environments whose last save failed.
    Failed {
        collections: Vec<Uuid>,
        environments: Vec<Uuid>,
    },
    TimedOut,
}

/// macOS Dock "Quit", logout and shutdown terminate the app without going
/// through `Quit`, and gpui-pre 0.3.11 offers no way to delay them.
pub(super) fn init(cx: &mut App) {
    // The window may still be borrowed by the dispatch that got us here.
    cx.on_action(|_: &Quit, cx| cx.defer(quit));
}

fn quit(cx: &mut App) {
    let asked = cx.windows().into_iter().any(|handle| {
        handle
            .update(cx, |_, window, cx| {
                let Some(workspace) = Workspace::in_window(window, cx) else {
                    return false;
                };
                workspace.update(cx, |this, cx| this.quit(window, cx));
                true
            })
            .unwrap_or(false)
    });
    if !asked {
        cx.quit();
    }
}

impl Workspace {
    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.confirm_discard(
            |this, window, cx| this.close(Close::App, window, cx),
            window,
            cx,
        );
    }

    /// The window's should-close handler. It always keeps the window open and
    /// closes it later: after asking about unsaved changes, and once the
    /// queued saves are on disk.
    pub(super) fn should_close_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.closing {
            return false;
        }
        self.confirm_discard(
            |this, window, cx| this.close(Close::Window, window, cx),
            window,
            cx,
        );
        false
    }

    /// Closes once the collection and shared environment saves are on disk,
    /// so choosing Save isn't lost. If one failed or saving takes too long,
    /// asks first.
    fn close(&mut self, close: Close, window: &mut Window, cx: &mut Context<Self>) {
        self.closing = true;
        self.flush_pending_edits(cx);
        let saved = future::join(
            self.collections.read(cx).flushed(),
            self.shared_environments.read(cx).flushed(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let unsaved = match saved
                .with_timeout(SAVE_TIMEOUT, cx.background_executor())
                .await
            {
                Ok((collections, environments))
                    if collections.is_empty() && environments.is_empty() =>
                {
                    None
                }
                Ok((collections, environments)) => Some(Unsaved::Failed {
                    collections,
                    environments,
                }),
                Err(_) => Some(Unsaved::TimedOut),
            };
            this.update_in(cx, |this, window, cx| match unsaved {
                None => this.finish_close(close, window, cx),
                Some(unsaved) => this.ask_close_anyway(close, unsaved, window, cx),
            })
            .ok();
        })
        .detach();
    }

    fn finish_close(&mut self, close: Close, window: &mut Window, cx: &mut Context<Self>) {
        match close {
            Close::Window => {
                self.save_navigation_now(cx);
                window.remove_window();
            }
            // `on_app_quit` saves the navigation.
            Close::App => cx.quit(),
        }
    }

    fn ask_close_anyway(
        &mut self,
        close: Close,
        unsaved: Unsaved,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let verb = match close {
            Close::Window => "close",
            Close::App => "quit",
        };
        let proceed_label = match close {
            Close::Window => "Close anyway",
            Close::App => "Quit anyway",
        };
        let dialog = match &unsaved {
            Unsaved::Failed {
                collections,
                environments,
            } => {
                tracing::warn!("closing found failed saves");
                let items = self.unsaved_items(collections, environments, cx);
                RetryDialog {
                    title: "Couldn't save your changes".into(),
                    description: unsaved_description(&items, verb).into(),
                    retry_label: "Try again".into(),
                    proceed_label: proceed_label.into(),
                }
            }
            Unsaved::TimedOut => {
                tracing::warn!("closing timed out waiting for the collections and environments");
                RetryDialog {
                    title: "Still saving your changes".into(),
                    description: format!(
                        "Saving is taking longer than expected. If you {verb} now, \
                         recent changes may be lost."
                    )
                    .into(),
                    retry_label: "Keep waiting".into(),
                    proceed_label: proceed_label.into(),
                }
            }
        };
        let this = cx.weak_entity();
        open_retry_dialog(
            dialog,
            move |choice, window, cx| {
                this.update(cx, |this, cx| match choice {
                    RetryChoice::Retry => {
                        this.retry_saves(&unsaved, cx);
                        this.close(close, window, cx);
                    }
                    RetryChoice::Proceed => this.finish_close(close, window, cx),
                    RetryChoice::Cancel => this.closing = false,
                })
                .ok();
            },
            window,
            cx,
        );
    }

    /// Saves the failed collections and shared environments again, as they are now.
    fn retry_saves(&mut self, unsaved: &Unsaved, cx: &mut Context<Self>) {
        let Unsaved::Failed {
            collections,
            environments,
        } = unsaved
        else {
            return;
        };
        self.collections
            .update(cx, |store, cx| store.retry(collections, cx));
        self.shared_environments
            .update(cx, |store, cx| store.retry(environments, cx));
    }

    fn unsaved_items(&self, collections: &[Uuid], environments: &[Uuid], cx: &App) -> Vec<String> {
        let stored = self.collections.read(cx);
        let shared = self.shared_environments.read(cx);
        let collections = collections.iter().map(|&id| match stored.get(id) {
            Some(collection) => format!("the collection “{}”", collection.name),
            None => "a deleted collection".to_string(),
        });
        let environments = environments.iter().map(|&id| match shared.get(id) {
            Some(environment) => format!("the shared environment “{}”", environment.name),
            None => "a deleted shared environment".to_string(),
        });
        collections.chain(environments).collect()
    }
}

/// E.g. "The collection “Pets” couldn't be saved. If you quit now, …".
fn unsaved_description(items: &[String], verb: &str) -> String {
    let named = if items.len() > NAMED_UNSAVED {
        NAMED_UNSAVED - 1
    } else {
        items.len()
    };
    let mut parts: Vec<String> = items[..named].to_vec();
    if named < items.len() {
        parts.push(format!("{} more", items.len() - named));
    }
    let list = match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => "Your changes".to_string(),
    };
    let mut chars = list.chars();
    let list = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => list,
    };
    format!("{list} couldn't be saved. If you {verb} now, the changes will be lost.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("the collection “{i}”")).collect()
    }

    #[test]
    fn describes_one_unsaved_item() {
        assert_eq!(
            unsaved_description(&items(1), "quit"),
            "The collection “1” couldn't be saved. If you quit now, the changes will be lost."
        );
    }

    #[test]
    fn lists_a_few_unsaved_items() {
        assert_eq!(
            unsaved_description(&items(3), "close"),
            "The collection “1”, the collection “2” and the collection “3” couldn't be saved. \
             If you close now, the changes will be lost."
        );
    }

    #[test]
    fn counts_the_rest_of_many_unsaved_items() {
        assert_eq!(
            unsaved_description(&items(5), "quit"),
            "The collection “1”, the collection “2” and 3 more couldn't be saved. \
             If you quit now, the changes will be lost."
        );
    }
}
