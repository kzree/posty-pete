use domain::openapi::ImportResult;
use domain::{
    collection::NEW_COLLECTION_NAME,
    history::{self, OpenMode},
    palette::Target,
    recents::Recent,
    request::{Auth, KeyValue},
};
use gpui::{Context, SharedString, Window};
use gpui_component::{WindowExt, notification::Notification};

use super::{EditorContent, Workspace, routes::AppRoute, show_error};
use crate::{
    collection_view::{CollectionViewEvent, EditTarget},
    collections::CollectionsEvent,
    command_palette::CommandPaletteEvent,
    history::HistoryEvent,
    import_dialog::{ImportDialogOptions, open_import_dialog},
    request_editor::RequestEditorEvent,
    settings::AppSettings,
    sidebar::{SidebarTab, history_panel::HistoryPanelEvent, library_panel::LibraryPanelEvent},
    ui::environments_editor::EnvironmentsEditorEvent,
};

impl Workspace {
    pub(super) fn on_history_event(
        &mut self,
        event: &HistoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            HistoryEvent::Loaded {
                entries,
                load_failed,
            } => self.history_panel.update(cx, |panel, cx| {
                panel.set_entries(entries.clone(), *load_failed, window, cx)
            }),
            HistoryEvent::Recorded(summary) => self
                .history_panel
                .update(cx, |panel, cx| panel.push(summary.clone(), window, cx)),
            HistoryEvent::Opened { entry, mode } => {
                if self.refuse_while_pending(window, cx) {
                    return;
                }
                let (entry, mode) = (entry.clone(), *mode);
                self.confirm_discard(
                    move |this, window, cx| {
                        this.responses.remove(EditorContent::Draft);
                        this.load_editor(EditorContent::Draft, &entry.request, window, cx);
                        this.navigate(AppRoute::Request, window, cx);
                        match (entry.omitted_body_size, mode) {
                            (Some(size), mode) => {
                                let note = history::omitted_body_note(size);
                                this.request_editor.update(cx, |editor, cx| {
                                    editor.show_body_note(note.clone(), cx)
                                });
                                if mode == OpenMode::Send {
                                    window.push_notification(
                                        Notification::warning(note).title("Request not sent"),
                                        cx,
                                    );
                                }
                            }
                            (None, OpenMode::Send) => {
                                this.request_editor.update(cx, |editor, cx| editor.send(cx))
                            }
                            (None, OpenMode::Load) => {}
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
            HistoryEvent::Pruned { cutoff } => self
                .history_panel
                .update(cx, |panel, cx| panel.prune(*cutoff, window, cx)),
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
        self.prune_recents(cx);
        if !self.route_exists(self.current_route(), cx) {
            self.replace_route(self.editor_route(), window, cx);
        } else {
            self.sync_editor_name(window, cx);
        }
        self.refresh_collection_view(window, cx);
        self.refresh_shared_environments_view(window, cx);
        self.refresh_library_environments(window, cx);
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
            LibraryPanelEvent::ImportOpenApi => self.open_import(window, cx),
            LibraryPanelEvent::OpenCollection(id) => self.open_collection(*id, window, cx),
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
            LibraryPanelEvent::UpdateFromSpec(id) => self.update_from_spec(*id, window, cx),
            LibraryPanelEvent::DeleteCollection(id) => self
                .collections
                .update(cx, |collections, cx| collections.delete(*id, cx)),
            LibraryPanelEvent::DeleteItem {
                collection_id,
                item_id,
            } => self.collections.update(cx, |collections, cx| {
                collections.remove_item(*collection_id, *item_id, cx)
            }),
            LibraryPanelEvent::DuplicateRequest {
                collection_id,
                request_id,
            } => {
                let copy = self.collections.update(cx, |collections, cx| {
                    collections.duplicate_request(*collection_id, *request_id, cx)
                });
                if let Some(copy) = copy
                    && AppSettings::get(cx).general.open_duplicated_request
                {
                    self.open_saved_request(*collection_id, copy, window, cx);
                }
            }
            LibraryPanelEvent::MoveRequest {
                collection_id,
                request_id,
                to,
            } => self.move_request(*collection_id, *request_id, *to, window, cx),
            LibraryPanelEvent::MoveFolder {
                collection_id,
                folder_id,
                to,
            } => self.move_folder(*collection_id, *folder_id, *to, window, cx),
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
            LibraryPanelEvent::CreateSharedEnvironment => self.add_shared_environment(window, cx),
            LibraryPanelEvent::OpenSharedEnvironment(id) => {
                self.open_shared_environments(Some(*id), window, cx)
            }
            LibraryPanelEvent::RenameSharedEnvironment { id, name } => {
                let (id, name) = (*id, name.clone());
                self.shared_environments
                    .update(cx, |environments, cx| environments.rename(id, name, cx));
            }
            LibraryPanelEvent::DuplicateSharedEnvironment(id) => {
                self.duplicate_shared_environment(*id, window, cx)
            }
            LibraryPanelEvent::DeleteSharedEnvironment(id) => {
                self.delete_shared_environment(*id, cx)
            }
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

    pub(super) fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.collections.update(cx, |collections, cx| {
            collections.create(NEW_COLLECTION_NAME, cx)
        });
        self.open_collection(id, window, cx);
        self.collection_view
            .update(cx, |view, cx| view.focus_name(window, cx));
    }

    pub(super) fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let client = self
            .client
            .get()
            .map_err(|error| SharedString::from(error.to_string()));
        let workspace = cx.weak_entity();
        open_import_dialog(
            ImportDialogOptions::import(),
            client,
            move |result, window, cx| {
                workspace
                    .update(cx, |this, cx| this.add_imported(result, window, cx))
                    .ok();
            },
            window,
            cx,
        );
    }

    fn add_imported(&mut self, result: ImportResult, window: &mut Window, cx: &mut Context<Self>) {
        let id = self
            .collections
            .update(cx, |collections, cx| collections.add(result.collection, cx));
        self.open_sidebar_tab(SidebarTab::Library, cx);
        self.library_panel
            .update(cx, |panel, cx| panel.reveal([id], cx));
        self.open_collection(id, window, cx);
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
            CollectionViewEvent::VariablesChanged { set, variables } => {
                self.save_variables(*set, variables.clone(), cx)
            }
            CollectionViewEvent::AuthChanged { target, auth } => {
                self.save_auth(*target, auth.clone(), cx)
            }
            CollectionViewEvent::AddEnvironment { collection_id } => {
                self.add_environment(*collection_id, window, cx)
            }
            CollectionViewEvent::RenameEnvironment {
                collection_id,
                environment_id,
                name,
            } => {
                let (collection_id, environment_id, name) =
                    (*collection_id, *environment_id, name.clone());
                self.collections.update(cx, |collections, cx| {
                    collections.rename_environment(collection_id, environment_id, name, cx)
                });
            }
            CollectionViewEvent::DuplicateEnvironment {
                collection_id,
                environment_id,
            } => self.duplicate_environment(*collection_id, *environment_id, window, cx),
            CollectionViewEvent::DeleteEnvironment {
                collection_id,
                environment_id,
            } => self.delete_environment(*collection_id, *environment_id, window, cx),
            CollectionViewEvent::UseEnvironment {
                collection_id,
                environment_id,
            } => self.set_active_environment(*collection_id, *environment_id, window, cx),
            CollectionViewEvent::SetEnvironmentBase {
                collection_id,
                environment_id,
                base,
            } => {
                let (collection_id, environment_id, base) =
                    (*collection_id, *environment_id, *base);
                self.collections.update(cx, |collections, cx| {
                    collections.set_environment_base(collection_id, environment_id, base, cx)
                });
            }
            CollectionViewEvent::ManageSharedEnvironments(id) => {
                self.open_shared_environments(*id, window, cx)
            }
            CollectionViewEvent::UpdateFromSpec(id) => self.update_from_spec(*id, window, cx),
            CollectionViewEvent::ChangeSource(id) => self.change_source(*id, window, cx),
            CollectionViewEvent::StopSyncing(id) => self.stop_syncing(*id, cx),
            CollectionViewEvent::Back => self.go_back_or_to_editor(window, cx),
        }
    }

    fn save_variables(
        &mut self,
        set: EditTarget,
        variables: Vec<KeyValue>,
        cx: &mut Context<Self>,
    ) {
        self.collections.update(cx, |collections, cx| match set {
            EditTarget::Collection(id) => collections.set_variables(id, variables, cx),
            EditTarget::Environment {
                collection_id,
                environment_id,
            } => {
                collections.set_environment_variables(collection_id, environment_id, variables, cx)
            }
        });
    }

    fn save_auth(&mut self, target: EditTarget, auth: Auth, cx: &mut Context<Self>) {
        self.collections.update(cx, |collections, cx| match target {
            EditTarget::Collection(id) => collections.set_auth(id, auth, cx),
            EditTarget::Environment {
                collection_id,
                environment_id,
            } => collections.set_environment_auth(collection_id, environment_id, auth, cx),
        });
    }

    /// Saves the collection and shared environment variables and auth edits
    /// still waiting for their save delay. Returns whether there were any.
    pub(super) fn flush_pending_edits(&mut self, cx: &mut Context<Self>) -> bool {
        let (pending, auth) = self.collection_view.update(cx, |view, cx| {
            (view.take_pending_variables(cx), view.take_pending_auth(cx))
        });
        let (shared, shared_auth) = self.shared_environments_view.update(cx, |view, cx| {
            (view.take_pending_variables(cx), view.take_pending_auth(cx))
        });
        let flushed =
            !pending.is_empty() || !auth.is_empty() || shared.is_some() || shared_auth.is_some();
        for (target, variables) in pending {
            self.save_variables(target, variables, cx);
        }
        for (target, auth) in auth {
            self.save_auth(target, auth, cx);
        }
        if let Some((id, variables)) = shared {
            self.shared_environments.update(cx, |environments, cx| {
                environments.set_variables(id, variables, cx)
            });
        }
        if let Some((id, auth)) = shared_auth {
            self.shared_environments
                .update(cx, |environments, cx| environments.set_auth(id, auth, cx));
        }
        flushed
    }

    pub(super) fn on_shared_environments_view_event(
        &mut self,
        event: &EnvironmentsEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            EnvironmentsEditorEvent::Add => self.add_shared_environment(window, cx),
            EnvironmentsEditorEvent::Rename { id, name } => self
                .shared_environments
                .update(cx, |environments, cx| environments.rename(id, name, cx)),
            EnvironmentsEditorEvent::Duplicate(id) => {
                self.duplicate_shared_environment(id, window, cx)
            }
            EnvironmentsEditorEvent::Delete(id) => self.delete_shared_environment(id, cx),
            EnvironmentsEditorEvent::SelectionChanged => self.sync_library_selection(window, cx),
            // The page has no "Use environment" and no bases.
            EnvironmentsEditorEvent::Use(_)
            | EnvironmentsEditorEvent::BaseChanged { .. }
            | EnvironmentsEditorEvent::ManageBases(_) => {}
            EnvironmentsEditorEvent::VariablesChanged { id, variables } => {
                self.shared_environments.update(cx, |environments, cx| {
                    environments.set_variables(id, variables, cx)
                })
            }
            EnvironmentsEditorEvent::AuthChanged { id, auth } => self
                .shared_environments
                .update(cx, |environments, cx| environments.set_auth(id, auth, cx)),
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
            HistoryPanelEvent::CopyAsCurl(id) => self.copy_history_as_curl(id, window, cx),
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
            RequestEditorEvent::CancelRequested => self.cancel_request(window, cx),
            RequestEditorEvent::CopyAsCurl(request) => {
                self.copy_editor_as_curl(request.clone(), window, cx)
            }
            RequestEditorEvent::Save => self.save_request(window, cx),
            RequestEditorEvent::Edited => {
                self.schedule_dirty_check(window, cx);
                self.schedule_navigation_save(cx);
            }
            RequestEditorEvent::OpenAuthSource(link) => self.open_auth_source(*link, window, cx),
        }
    }

    pub(super) fn on_command_palette_event(
        &mut self,
        event: &CommandPaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.close_dialog(cx);
        match *event {
            CommandPaletteEvent::Open(target) => self.open_palette_target(target, window, cx),
            CommandPaletteEvent::Run(command) => {
                self.record_recent(Recent::Command { command }, cx);
                self.run_command(command, window, cx);
            }
        }
    }

    fn open_palette_target(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::Request {
                collection_id,
                request_id,
            } => self.open_saved_request(collection_id, request_id, window, cx),
            Target::Collection(id) => self.open_collection(id, window, cx),
            Target::Folder {
                collection_id,
                folder_id,
            } => self.reveal_folder(collection_id, folder_id, window, cx),
            Target::SharedEnvironment(id) => self.open_shared_environments(Some(id), window, cx),
            Target::CollectionEnvironment {
                collection_id,
                environment_id,
            } => self.open_environments(collection_id, Some(environment_id), window, cx),
            Target::History(id) => self
                .history
                .update(cx, |history, cx| history.open(id, OpenMode::Load, cx)),
            Target::Command(command) => self.run_command(command, window, cx),
            Target::SettingsSection(section) => {
                self.navigate(AppRoute::Settings, window, cx);
                self.settings_page
                    .update(cx, |page, cx| page.show_section(section, window, cx));
                self.record_recent(Recent::SettingsPage { section }, cx);
            }
            Target::Setting(setting) => {
                self.navigate(AppRoute::Settings, window, cx);
                self.settings_page
                    .update(cx, |page, cx| page.reveal(setting, cx));
                let section = setting.section();
                self.record_recent(Recent::SettingsPage { section }, cx);
            }
        }
    }
}
