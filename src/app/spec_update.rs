use domain::http::Client;
use gpui::{Context, SharedString, Window};
use gpui_component::WindowExt;
use uuid::Uuid;

use super::{EditorContent, Workspace, show_error};
use crate::{
    import_dialog::{ImportDialogOptions, open_import_dialog},
    spec_update_dialog::{SpecUpdate, SpecUpdateStart, open_spec_update_dialog},
};

impl Workspace {
    fn spec_client(&mut self) -> Result<Client, SharedString> {
        self.client
            .get()
            .map_err(|error| SharedString::from(error.to_string()))
    }

    pub(super) fn update_from_spec(
        &mut self,
        collection_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_spec_update(collection_id, SpecUpdateStart::Reload, window, cx);
    }

    /// Picks another file or URL for an imported collection, then previews
    /// updating from it.
    pub(super) fn change_source(
        &mut self,
        collection_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(location) = self
            .collections
            .read(cx)
            .get(collection_id)
            .and_then(|collection| collection.source.as_ref())
            .map(|source| source.location.clone())
        else {
            return;
        };
        let options = ImportDialogOptions {
            title: "Change source",
            confirm_label: "Continue",
            initial: Some(location),
        };
        let client = self.spec_client();
        let workspace = cx.weak_entity();
        open_import_dialog(
            options,
            client,
            move |result, window, cx| {
                let workspace = workspace.clone();
                // Once the source dialog has closed.
                window.defer(cx, move |window, cx| {
                    workspace
                        .update(cx, |this, cx| {
                            let start = SpecUpdateStart::Loaded(Box::new(result));
                            this.open_spec_update(collection_id, start, window, cx)
                        })
                        .ok();
                });
            },
            window,
            cx,
        );
    }

    fn open_spec_update(
        &mut self,
        collection_id: Uuid,
        start: SpecUpdateStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        self.flush_pending_edits(cx);
        let Some(current) = self
            .collections
            .read(cx)
            .shared(collection_id)
            .filter(|collection| collection.source.is_some())
        else {
            return;
        };
        let client = self.spec_client();
        let workspace = cx.weak_entity();
        open_spec_update_dialog(
            current,
            start,
            client,
            move |update, window, cx| {
                workspace
                    .update(cx, |this, cx| {
                        this.apply_spec_update(collection_id, update, window, cx)
                    })
                    .ok();
            },
            window,
            cx,
        );
    }

    /// An open request the update changes reloads, unless it has unsaved
    /// edits; one it removes becomes the draft, as when it's deleted.
    fn apply_spec_update(
        &mut self,
        collection_id: Uuid,
        update: SpecUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_pending_edits(cx);
        let open = match self.editor_content {
            EditorContent::Saved {
                collection_id: open_collection,
                request_id,
            } if open_collection == collection_id => self
                .find_saved_request(collection_id, request_id, cx)
                .cloned(),
            _ => None,
        };
        let was_dirty = self.is_dirty_now(window, cx);
        let applied = self.collections.update(cx, |collections, cx| {
            collections.apply_spec_update(collection_id, &update.fresh, &update.choices, cx)
        });
        if !applied {
            let error = anyhow::anyhow!("The collection no longer exists.");
            show_error("Couldn't update collection", &error, window, cx);
            return;
        }
        let Some(before) = open.filter(|_| !was_dirty) else {
            return;
        };
        let changed = self
            .find_saved_request(collection_id, before.id, cx)
            .is_some_and(|after| after.request != before.request || after.name != before.name);
        if changed {
            self.reload_editor(window, cx);
        }
    }

    pub(super) fn stop_syncing(&mut self, collection_id: Uuid, cx: &mut Context<Self>) {
        self.collections.update(cx, |collections, cx| {
            collections.stop_syncing(collection_id, cx)
        });
    }
}
