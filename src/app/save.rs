use gpui::{Context, Window};
use gpui_component::{WindowExt, notification::Notification};
use uuid::Uuid;

use super::{EditorContent, Workspace, dirty::Baseline, show_error};
use crate::{
    domain::{self, collection::Location},
    save_as_dialog::{self, open_save_as_dialog},
};

impl Workspace {
    /// Saves the editor's request if the main pane shows it; an unlinked
    /// request opens Save As.
    pub(super) fn save_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() != self.editor_route() {
            return;
        }
        match self.editor_content {
            EditorContent::Draft => self.open_save_as(window, cx),
            EditorContent::Saved { .. } => {
                self.save_editor(window, cx);
            }
        }
    }

    /// Asks where to save the editor's request as a new saved request, if the main pane shows it.
    pub(super) fn open_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() != self.editor_route() || window.has_active_dialog(cx) {
            return;
        }
        if self.refuse_while_pending(window, cx) {
            return;
        }
        let collections = self.collections.read(cx).all();
        let (name, location) = match self.editor_content {
            EditorContent::Saved {
                collection_id,
                request_id,
            } => {
                let name = self.request_editor.read(cx).name(cx).or_else(|| {
                    self.editor_saved_request(cx)
                        .map(|saved| saved.name.clone())
                });
                let folder_id = collections
                    .iter()
                    .find(|collection| collection.id == collection_id)
                    .and_then(|collection| collection.ancestors(request_id))
                    .and_then(|ancestors| ancestors.last().copied());
                let location = Location::Folder {
                    collection_id,
                    folder_id,
                };
                (name, location)
            }
            EditorContent::Draft => {
                let location = collections
                    .first()
                    .map_or(Location::NewCollection, |collection| Location::Folder {
                        collection_id: collection.id,
                        folder_id: None,
                    });
                (None, location)
            }
        };
        let name = name.unwrap_or_else(|| "New Request".to_string());
        let locations = save_as_dialog::locations(collections);
        let this = cx.weak_entity();
        open_save_as_dialog(
            name,
            locations,
            location,
            move |name, location, window, cx| {
                this.update(cx, |this, cx| this.save_as(name, location, window, cx))
                    .ok();
            },
            window,
            cx,
        );
    }

    /// Saves the editor's request as a new saved request at `location` and
    /// links the editor to it, in place of the current history entry. A saved
    /// request the editor held stays as it was stored.
    fn save_as(
        &mut self,
        name: String,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_pending(window, cx) {
            return;
        }
        let request = self.request_editor.read(cx).build_request(cx);
        let Some((collection_id, request_id)) = self.collections.update(cx, |collections, cx| {
            collections.add_request_at(location, name.clone(), request.clone(), cx)
        }) else {
            show_error(
                "Couldn't save request",
                &anyhow::anyhow!("The chosen folder is no longer there."),
                window,
                cx,
            );
            return;
        };
        let content = EditorContent::Saved {
            collection_id,
            request_id,
        };
        if self.editor_content == EditorContent::Draft {
            self.responses.rekey(self.editor_content, content);
            self.draft = None;
        } else {
            self.responses.copy(self.editor_content, content);
        }
        self.editor_content = content;
        self.baseline = Some(Baseline::new(request.clone(), request));
        self.request_editor
            .update(cx, |editor, cx| editor.link(name, window, cx));
        self.refresh_dirty(window, cx);

        let ancestors = self
            .collections
            .read(cx)
            .get(collection_id)
            .and_then(|collection| collection.ancestors(request_id))
            .unwrap_or_default();
        self.library_panel.update(cx, |panel, cx| {
            panel.reveal(std::iter::once(collection_id).chain(ancestors), cx)
        });
        self.replace_route(content.route(), window, cx);
        window.push_notification(Notification::success("Saved"), cx);
    }

    /// Saves the editor's request to its collection. Returns `false` if it
    /// isn't a saved request (any more). Writing to disk happens later and
    /// reports its own errors.
    pub(super) fn save_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let EditorContent::Saved {
            collection_id,
            request_id,
        } = self.editor_content
        else {
            return false;
        };
        let (request, name) = self.request_editor.update(cx, |editor, cx| {
            editor.tidy_name(window, cx);
            (editor.build_request(cx), editor.name(cx))
        });
        let baseline = Baseline::new(request.clone(), request.clone());
        let saved = self.collections.update(cx, |collections, cx| {
            collections.update_request(collection_id, request_id, request, name, cx)
        });
        if !saved {
            return false;
        }
        self.baseline = Some(baseline);
        self.refresh_dirty(window, cx);
        window.push_notification(Notification::success("Saved"), cx);
        true
    }

    pub(super) fn add_saved_request(
        &mut self,
        collection_id: Uuid,
        parent: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_pending(window, cx) {
            return;
        }
        self.confirm_discard(
            move |this, window, cx| {
                let Some(request_id) = this.collections.update(cx, |collections, cx| {
                    collections.add_request(
                        collection_id,
                        parent,
                        "New Request",
                        domain::request::Request::default(),
                        cx,
                    )
                }) else {
                    return;
                };
                this.open_saved_request(collection_id, request_id, window, cx);
            },
            window,
            cx,
        );
    }
}
