use gpui::{App, Context, Window};
use gpui_component::{WindowExt, notification::Notification};
use uuid::Uuid;

use super::{EditorContent, Workspace, dirty::Baseline, routes::AppRoute, show_error};
use crate::{
    domain::{self, collection::SavedRequest},
    ui::{
        confirm_dialog::open_confirm_dialog,
        save_changes_dialog::{SaveChoice, open_save_changes_dialog},
    },
};

impl Workspace {
    /// Runs `then` if the editor has no unsaved changes; otherwise asks to save
    /// or discard them first, and drops `then` if the user cancels. Everything
    /// that replaces the editor's content goes through here.
    pub(super) fn confirm_discard(
        &mut self,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.discard_confirmed || !self.is_dirty_now(window, cx) {
            then(self, window, cx);
            return;
        }
        let name = self.editor_saved_request(cx).map(|saved| {
            self.request_editor
                .read(cx)
                .name(cx)
                .unwrap_or_else(|| saved.name.clone())
        });
        // Ignored while a prompt is already open.
        let Some(name) = name.filter(|_| self.discard_prompt.is_none()) else {
            self.sync_library_selection(window, cx);
            return;
        };
        self.discard_prompt = Some(Box::new(then));
        let this = cx.weak_entity();
        open_save_changes_dialog(
            name,
            move |choice, window, cx| {
                this.update(cx, |this, cx| this.on_discard_choice(choice, window, cx))
                    .ok();
            },
            window,
            cx,
        );
    }

    fn on_discard_choice(
        &mut self,
        choice: SaveChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(then) = self.discard_prompt.take() else {
            return;
        };
        let proceed = match choice {
            SaveChoice::Save => {
                let saved = self.save_editor(window, cx);
                if !saved {
                    show_error(
                        "Couldn't save request",
                        &anyhow::anyhow!("The request is no longer in its collection."),
                        window,
                        cx,
                    );
                }
                saved
            }
            SaveChoice::Discard => true,
            SaveChoice::Cancel => false,
        };
        if proceed {
            self.with_discard_confirmed(|this| then(this, window, cx));
        } else {
            self.sync_library_selection(window, cx);
        }
    }

    /// Loads `request` into the editor and shows its kept response, if any.
    /// Loading a draft replaces it; loading a saved request over the draft
    /// keeps a snapshot of it.
    pub(super) fn load_editor(
        &mut self,
        content: EditorContent,
        request: &domain::request::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_request(content, request, window, cx);
        let kept = self.responses.get(content).cloned();
        self.response_view.update(cx, |view, cx| match kept {
            Some(kept) => view.set_response(kept.result, kept.body_text, window, cx),
            None => view.clear(window, cx),
        });
    }

    /// `load_editor` without touching the response view.
    fn load_request(
        &mut self,
        content: EditorContent,
        request: &domain::request::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.discard_confirmed && self.is_dirty_now(window, cx) {
            tracing::error!("unsaved changes replaced without confirm_discard");
            window.push_notification(
                Notification::error("Your unsaved changes to the request were discarded.")
                    .title("Unsaved changes lost"),
                cx,
            );
        }
        self.draft = match (self.editor_content, content) {
            (EditorContent::Draft, EditorContent::Saved { .. }) => {
                Some(self.request_editor.read(cx).build_request(cx))
            }
            (_, EditorContent::Draft) => None,
            _ => self.draft.take(),
        };
        self.editor_content = content;
        self.request_editor
            .update(cx, |editor, cx| editor.load_request(request, window, cx));
        self.baseline = match content {
            EditorContent::Saved { .. } => Some(Baseline::new(
                request.clone(),
                self.request_editor.read(cx).build_request(cx),
            )),
            EditorContent::Draft => None,
        };
        self.sync_editor_name(window, cx);
        self.refresh_dirty(window, cx);
        self.schedule_navigation_save(cx);
    }

    /// Asks to throw away the unsaved changes to the editor's saved request,
    /// then reloads the stored request. The shown response stays.
    pub(super) fn revert_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_content == EditorContent::Draft
            || window.has_active_dialog(cx)
            || !self.is_dirty_now(window, cx)
        {
            return;
        }
        if self.refuse_while_pending(window, cx) {
            return;
        }
        let Some(name) = self
            .editor_saved_request(cx)
            .map(|saved| saved.name.trim().to_string())
        else {
            return;
        };
        let this = cx.weak_entity();
        open_confirm_dialog(
            format!("Revert changes to “{name}”?"),
            "Your unsaved changes will be lost.",
            "Revert",
            move |window, cx| {
                this.update(cx, |this, cx| this.reload_editor(window, cx))
                    .ok();
            },
            window,
            cx,
        );
    }

    /// Loads the stored version of the editor's saved request, keeping the response view as is.
    fn reload_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self
            .editor_saved_request(cx)
            .map(|saved| saved.request.clone())
        else {
            return;
        };
        let content = self.editor_content;
        self.with_discard_confirmed(|this| this.load_request(content, &request, window, cx));
    }

    /// Runs `f` with `confirm_discard` and `load_request` treating the
    /// editor's changes as already confirmed for discarding.
    fn with_discard_confirmed(&mut self, f: impl FnOnce(&mut Self)) {
        let was = std::mem::replace(&mut self.discard_confirmed, true);
        f(self);
        self.discard_confirmed = was;
    }

    pub(super) fn drop_deleted_responses(&mut self, cx: &App) {
        let collections = self.collections.read(cx);
        self.responses.retain(|content| match content {
            EditorContent::Draft => true,
            EditorContent::Saved {
                collection_id,
                request_id,
            } => collections
                .get(collection_id)
                .is_some_and(|collection| collection.find_request(request_id).is_some()),
        });
    }

    /// Whether the editor holds a saved request with unsaved changes, as of
    /// the last check. A pending check runs before the next frame is drawn.
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// `is_dirty`, running a pending check first.
    pub(super) fn is_dirty_now(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.dirty_check.is_pending() {
            self.refresh_dirty(window, cx);
        }
        self.dirty
    }

    /// Checks dirtiness once before the next frame, however many edits come first.
    pub(super) fn schedule_dirty_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty_check.request() {
            cx.on_next_frame(window, |this, window, cx| {
                if this.dirty_check.is_pending() {
                    this.refresh_dirty(window, cx);
                }
            });
        }
    }

    /// Recomputes the cached dirty flag after an edit, a load, a save or a
    /// change to the collections.
    pub(super) fn refresh_dirty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dirty_check.clear();
        let dirty = self.baseline.as_ref().is_some_and(|baseline| {
            self.editor_saved_request(cx).is_some_and(|saved| {
                let editor = self.request_editor.read(cx);
                baseline.is_dirty(saved, &editor.build_request(cx), editor.name(cx).as_deref())
            })
        });
        let dirty_request = match self.editor_content {
            EditorContent::Saved { request_id, .. } if dirty => Some(request_id),
            _ => None,
        };
        self.library_panel
            .update(cx, |panel, _| panel.set_dirty_request(dirty_request));
        if dirty == self.dirty {
            return;
        }
        self.dirty = dirty;
        self.request_editor
            .update(cx, |editor, cx| editor.set_dirty(dirty, cx));
        self.sync_window_title(window, cx);
        cx.notify();
    }

    /// Whether the main pane shows the editor's saved request with unsaved changes.
    pub(super) fn shows_dirty_request(&self) -> bool {
        self.is_dirty() && self.current_route() == self.editor_route()
    }

    /// Shows a toast and returns `true` while a request is in flight.
    pub(super) fn refuse_while_pending(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending.is_none() {
            return false;
        }
        window.push_notification(
            Notification::info("Wait for it to finish, then try again.")
                .title("A request is still running"),
            cx,
        );
        // A click may have selected another row; put the selection back.
        self.sync_library_selection(window, cx);
        true
    }

    pub(super) fn find_saved_request<'a>(
        &self,
        collection_id: Uuid,
        request_id: Uuid,
        cx: &'a App,
    ) -> Option<&'a SavedRequest> {
        self.collections
            .read(cx)
            .get(collection_id)?
            .find_request(request_id)
    }

    /// The stored version of the saved request the editor holds.
    pub(super) fn editor_saved_request<'a>(&self, cx: &'a App) -> Option<&'a SavedRequest> {
        match self.editor_content {
            EditorContent::Saved {
                collection_id,
                request_id,
            } => self.find_saved_request(collection_id, request_id, cx),
            EditorContent::Draft => None,
        }
    }

    /// Selects the library row for what the main pane shows.
    pub(super) fn sync_library_selection(&mut self, window: &mut Window, cx: &mut App) {
        let selected = match self.current_route() {
            AppRoute::SavedRequest { request_id, .. } => Some(request_id),
            AppRoute::Collection(id) => Some(id),
            AppRoute::Request | AppRoute::Settings => None,
        };
        self.library_panel
            .update(cx, |panel, cx| panel.set_selected(selected, window, cx));
    }

    /// Hands the editor the stored name of the saved request it holds.
    pub(super) fn sync_editor_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .editor_saved_request(cx)
            .map(|saved| saved.name.clone());
        self.request_editor
            .update(cx, |editor, cx| editor.set_saved_name(name, window, cx));
    }
}
