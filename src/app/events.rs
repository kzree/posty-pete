use gpui::{Context, Window};
use gpui_component::WindowExt;

use super::{EditorContent, Workspace, routes::AppRoute, show_error};
use crate::{
    collection_view::CollectionViewEvent,
    collections::CollectionsEvent,
    domain::{collection::NEW_COLLECTION_NAME, history::OpenMode},
    history::HistoryEvent,
    quick_open::QuickOpenEvent,
    request_editor::RequestEditorEvent,
    sidebar::{history_panel::HistoryPanelEvent, library_panel::LibraryPanelEvent},
};

impl Workspace {
    pub(super) fn on_history_event(
        &mut self,
        event: &HistoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            HistoryEvent::Loaded(entries) => self.history_panel.update(cx, |panel, cx| {
                panel.set_entries(entries.clone(), window, cx)
            }),
            HistoryEvent::Recorded(summary) => self
                .history_panel
                .update(cx, |panel, cx| panel.push(summary.clone(), window, cx)),
            HistoryEvent::Opened { request, mode } => {
                if self.refuse_while_pending(window, cx) {
                    return;
                }
                let (request, mode) = (request.clone(), *mode);
                self.confirm_discard(
                    move |this, window, cx| {
                        this.responses.remove(EditorContent::Draft);
                        this.load_editor(EditorContent::Draft, &request, window, cx);
                        this.navigate(AppRoute::Request, window, cx);
                        if mode == OpenMode::Send {
                            this.request_editor.update(cx, |editor, cx| editor.send(cx));
                        }
                    },
                    window,
                    cx,
                );
            }
            HistoryEvent::Deleted(id) => self
                .history_panel
                .update(cx, |panel, cx| panel.remove(*id, window, cx)),
            HistoryEvent::Cleared => self
                .history_panel
                .update(cx, |panel, cx| panel.clear(window, cx)),
            HistoryEvent::Failed { title, error } => show_error(title, error, window, cx),
        }
    }

    pub(super) fn on_collections_event(
        &mut self,
        event: &CollectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CollectionsEvent::Changed => self.on_collections_changed(window, cx),
            CollectionsEvent::Loaded => self.restore_navigation(window, cx),
            CollectionsEvent::Failed { title, error } => show_error(title, error, window, cx),
        }
    }

    fn on_collections_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let collections = self.collections.read(cx).all().to_vec();
        self.library_panel.update(cx, |panel, cx| {
            panel.set_collections(collections, window, cx)
        });
        if !self.route_exists(self.editor_route(), cx) {
            self.keep_deleted_request_as_draft();
        }
        self.drop_deleted_responses(cx);
        if !self.route_exists(self.current_route(), cx) {
            self.replace_route(self.editor_route(), window, cx);
        } else {
            self.sync_editor_name(window, cx);
        }
        if let Some(id) = self.collection_view.read(cx).id() {
            let collection = self.collections.read(cx).get(id).cloned();
            self.collection_view
                .update(cx, |view, cx| view.show(id, collection, window, cx));
        }
        self.refresh_dirty(window, cx);
        self.sync_window_title(window, cx);
        self.schedule_navigation_save(cx);
        cx.notify();
    }

    /// The editor's saved request is gone; what the editor shows stays, as the draft.
    fn keep_deleted_request_as_draft(&mut self) {
        self.responses
            .rekey(self.editor_content, EditorContent::Draft);
        self.editor_content = EditorContent::Draft;
        self.draft = None;
        self.baseline = None;
    }

    pub(super) fn on_library_panel_event(
        &mut self,
        event: &LibraryPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            LibraryPanelEvent::CreateCollection => self.create_collection(window, cx),
            LibraryPanelEvent::OpenCollection(id) => {
                self.navigate(AppRoute::Collection(*id), window, cx)
            }
            LibraryPanelEvent::OpenRequest {
                collection_id,
                request_id,
            } => self.open_saved_request(*collection_id, *request_id, window, cx),
            LibraryPanelEvent::Rename {
                collection_id,
                item_id,
                name,
            } => self.collections.update(cx, |collections, cx| {
                collections.rename(*collection_id, *item_id, name.clone(), cx)
            }),
            LibraryPanelEvent::DeleteCollection(id) => self
                .collections
                .update(cx, |collections, cx| collections.delete(*id, cx)),
            LibraryPanelEvent::DeleteItem {
                collection_id,
                item_id,
            } => self.collections.update(cx, |collections, cx| {
                collections.remove_item(*collection_id, *item_id, cx)
            }),
            LibraryPanelEvent::AddFolder {
                collection_id,
                parent,
                name,
            } => {
                self.collections.update(cx, |collections, cx| {
                    collections.add_folder(*collection_id, *parent, name.clone(), cx)
                });
            }
            LibraryPanelEvent::AddRequest {
                collection_id,
                parent,
            } => self.add_saved_request(*collection_id, *parent, window, cx),
            LibraryPanelEvent::RevertRequest {
                collection_id,
                request_id,
            } => {
                let content = EditorContent::Saved {
                    collection_id: *collection_id,
                    request_id: *request_id,
                };
                if self.editor_content == content {
                    self.revert_request(window, cx);
                }
            }
        }
    }

    fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.collections.update(cx, |collections, cx| {
            collections.create(NEW_COLLECTION_NAME, cx)
        });
        self.navigate(AppRoute::Collection(id), window, cx);
        self.collection_view
            .update(cx, |view, cx| view.focus_name(window, cx));
    }

    pub(super) fn on_collection_view_event(
        &mut self,
        event: &CollectionViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CollectionViewEvent::Rename { id, name } => {
                let (id, name) = (*id, name.clone());
                self.collections
                    .update(cx, |collections, cx| collections.rename(id, id, name, cx));
            }
            CollectionViewEvent::Back => self.go_back_or_to_editor(window, cx),
        }
    }

    pub(super) fn on_history_panel_event(
        &mut self,
        event: &HistoryPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match *event {
            HistoryPanelEvent::Open { id, mode } => self
                .history
                .update(cx, |history, cx| history.open(id, mode, cx)),
            HistoryPanelEvent::CopyAsCurl(id) => self.copy_as_curl(id, window, cx),
            HistoryPanelEvent::Delete(id) => self
                .history
                .update(cx, |history, cx| history.delete(id, cx)),
            HistoryPanelEvent::Clear => self.history.update(cx, |history, cx| history.clear(cx)),
        }
    }

    pub(super) fn on_editor_event(
        &mut self,
        event: &RequestEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RequestEditorEvent::SendRequested(request) => {
                self.send_request(request.clone(), window, cx)
            }
            RequestEditorEvent::Save => self.save_request(window, cx),
            RequestEditorEvent::Edited => {
                self.schedule_dirty_check(window, cx);
                self.schedule_navigation_save(cx);
            }
        }
    }

    pub(super) fn on_quick_open_event(
        &mut self,
        event: &QuickOpenEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.close_dialog(cx);
        match *event {
            QuickOpenEvent::OpenCollection(id) => {
                self.navigate(AppRoute::Collection(id), window, cx)
            }
            QuickOpenEvent::OpenRequest {
                collection_id,
                request_id,
            } => self.open_saved_request(collection_id, request_id, window, cx),
        }
    }
}
