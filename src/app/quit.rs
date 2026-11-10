use std::time::Duration;

use gpui::{App, Context, FutureExt as _, Window};

use super::Workspace;
use crate::actions::Quit;

/// How long closing waits for queued collection saves.
const SAVE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
enum Close {
    Window,
    App,
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

    /// The window's should-close handler. With unsaved changes it keeps the
    /// window open and asks first; the window is closed once the user chose.
    pub(super) fn should_close_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.closing || !self.is_dirty() {
            self.save_navigation_now(cx);
            return true;
        }
        self.confirm_discard(
            |this, window, cx| this.close(Close::Window, window, cx),
            window,
            cx,
        );
        false
    }

    /// Closes once the collection saves are on disk, so choosing Save isn't lost.
    fn close(&mut self, close: Close, window: &mut Window, cx: &mut Context<Self>) {
        self.closing = true;
        let saved = self.collections.read(cx).flushed();
        cx.spawn_in(window, async move |this, cx| {
            if saved
                .with_timeout(SAVE_TIMEOUT, cx.background_executor())
                .await
                .is_err()
            {
                tracing::warn!("closing before the collections were saved");
            }
            this.update_in(cx, |this, window, cx| match close {
                Close::Window => {
                    this.save_navigation_now(cx);
                    window.remove_window();
                }
                // `on_app_quit` saves the navigation.
                Close::App => cx.quit(),
            })
            .ok();
        })
        .detach();
    }
}
