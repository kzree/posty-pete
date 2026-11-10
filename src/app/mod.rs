mod dirty;
mod navigation;
mod routes;
mod send;

use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, NavigationDirection, ParentElement, Render, Role, StatefulInteractiveElement,
    Styled, Subscription, Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme, ResizableState, WindowExt, h_resizable, input::Escape, notification::Notification,
    resizable_panel,
};
use uuid::Uuid;

use self::{
    dirty::Baseline,
    navigation::Direction,
    routes::{AppRoute, RouteHistory},
};
use crate::{
    actions::{
        FocusUrl, GoBack, GoForward, NewRequest, OpenHistory, OpenLibrary, OpenSettings, OpenTests,
        SaveRequest, SendRequest, ToggleSidebar, WORKSPACE_CONTEXT,
    },
    collection_view::{CollectionView, CollectionViewEvent},
    collections::{Collections, CollectionsEvent},
    domain::{self, history::OpenMode},
    history::{History, HistoryEvent},
    request_editor::{RequestEditor, RequestEditorEvent},
    response_view::ResponseView,
    sidebar::{
        Sidebar, SidebarTab,
        history_panel::{HistoryPanel, HistoryPanelEvent},
        library_panel::{LibraryPanel, LibraryPanelEvent},
    },
    status_bar::StatusBar,
    ui::{
        loading_bar::LoadingBar,
        save_changes_dialog::{SaveChoice, open_save_changes_dialog},
    },
    window_title_bar::WindowTitleBar,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Logs `error` and shows it as a toast.
fn show_error(title: &str, error: &anyhow::Error, window: &mut Window, cx: &mut App) {
    tracing::warn!("{title}: {error:#}");
    window.push_notification(
        Notification::error(format!("{error:#}")).title(title.to_string()),
        cx,
    );
}

/// What the request editor holds. It matches the route whenever the route is
/// `/` or a saved request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorContent {
    Draft,
    Saved {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

/// Work that replaces the editor's content, run once unsaved changes are dealt with.
type Continuation = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeftPanelState {
    Closed,
    Open,
}

pub struct Workspace {
    request_editor: Entity<RequestEditor>,
    response_view: Entity<ResponseView>,
    status_bar: Entity<StatusBar>,
    sidebar: Entity<Sidebar>,
    history_panel: Entity<HistoryPanel>,
    library_panel: Entity<LibraryPanel>,
    collection_view: Entity<CollectionView>,
    loading_bar: Entity<LoadingBar>,
    client: reqwest::Client,
    history: Entity<History>,
    collections: Entity<Collections>,
    route_history: RouteHistory,
    editor_content: EditorContent,
    /// The draft, kept while the editor holds a saved request.
    draft: Option<domain::request::Request>,
    /// Set while the editor holds a saved request.
    baseline: Option<Baseline>,
    dirty: bool,
    /// Set while the save prompt is open.
    discard_prompt: Option<Continuation>,
    /// Set while a continuation runs, after the user saved or discarded.
    discard_confirmed: bool,
    pending: Option<Task<()>>,
    left_panel: LeftPanelState,
    main_layout: Entity<ResizableState>,
    request_response_layout: Entity<ResizableState>,
    /// The OS window title last set.
    os_window_title: String,
    os_window_edited: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request_editor = cx.new(|cx| RequestEditor::new(window, cx));
        let response_view = cx.new(|cx| ResponseView::new(window, cx));
        let left_panel = LeftPanelState::Open;
        let status_bar = cx.new(|_| StatusBar::new(left_panel == LeftPanelState::Open));
        let history_panel = cx.new(|cx| HistoryPanel::new(window, cx));
        let library_panel = cx.new(|cx| LibraryPanel::new(window, cx));
        let sidebar = cx.new(|_| Sidebar::new(library_panel.clone(), history_panel.clone()));
        let collection_view = cx.new(|cx| CollectionView::new(window, cx));
        let loading_bar = cx.new(|_| LoadingBar::default());
        let history = cx.new(History::new);
        let collections = cx.new(Collections::new);

        let subscriptions = vec![
            cx.subscribe_in(&history, window, |this, _, event, window, cx| {
                this.on_history_event(event, window, cx)
            }),
            cx.subscribe_in(&collections, window, |this, _, event, window, cx| {
                this.on_collections_event(event, window, cx)
            }),
            cx.subscribe_in(&library_panel, window, |this, _, event, window, cx| {
                this.on_library_panel_event(event, window, cx)
            }),
            cx.subscribe_in(&collection_view, window, |this, _, event, window, cx| {
                this.on_collection_view_event(event, window, cx)
            }),
            cx.subscribe_in(&history_panel, window, |this, _, event, window, cx| {
                this.on_history_panel_event(event, window, cx)
            }),
            cx.subscribe_in(&request_editor, window, |this, _, event, window, cx| {
                this.on_editor_event(event, window, cx)
            }),
        ];

        let mut workspace = Self {
            request_editor,
            response_view,
            status_bar,
            sidebar,
            history_panel,
            library_panel,
            collection_view,
            loading_bar,
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .redirect(domain::http::redirect_policy())
                .build()
                .expect("failed to build HTTP client"),
            history,
            collections,
            route_history: RouteHistory::new(AppRoute::Request),
            editor_content: EditorContent::Draft,
            draft: None,
            baseline: None,
            dirty: false,
            discard_prompt: None,
            discard_confirmed: false,
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            os_window_title: String::new(),
            os_window_edited: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        workspace.sync_window_title(window, cx);
        workspace
    }

    fn on_history_event(
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
            HistoryEvent::Opened { .. } if self.pending.is_some() => {
                window.push_notification(
                    Notification::info("Wait for it to finish, then try again.")
                        .title("A request is still running"),
                    cx,
                );
            }
            HistoryEvent::Opened { request, mode } => {
                let (request, mode) = (request.clone(), *mode);
                self.confirm_discard(
                    move |this, window, cx| {
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

    fn on_collections_event(
        &mut self,
        event: &CollectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CollectionsEvent::Changed => {
                let collections = self.collections.read(cx).all().to_vec();
                self.library_panel.update(cx, |panel, cx| {
                    panel.set_collections(collections, window, cx)
                });
                if !self.route_exists(self.editor_route(), cx) {
                    // What was in the editor stays there, as the draft.
                    self.editor_content = EditorContent::Draft;
                    self.draft = None;
                    self.baseline = None;
                }
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
                cx.notify();
            }
            CollectionsEvent::Failed { title, error } => show_error(title, error, window, cx),
        }
    }

    fn on_library_panel_event(
        &mut self,
        event: &LibraryPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            LibraryPanelEvent::CreateCollection => {
                let id = self.collections.update(cx, |collections, cx| {
                    collections.create("New Collection", cx)
                });
                self.navigate(AppRoute::Collection(id), window, cx);
                self.collection_view
                    .update(cx, |view, cx| view.focus_name(window, cx));
            }
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
            } => {
                let (collection_id, item_id, name) = (*collection_id, *item_id, name.clone());
                self.collections.update(cx, |collections, cx| {
                    collections.rename(collection_id, item_id, name, cx)
                });
            }
            LibraryPanelEvent::DeleteCollection(id) => {
                let id = *id;
                self.collections
                    .update(cx, |collections, cx| collections.delete(id, cx));
            }
            LibraryPanelEvent::DeleteItem {
                collection_id,
                item_id,
            } => {
                let (collection_id, item_id) = (*collection_id, *item_id);
                self.collections.update(cx, |collections, cx| {
                    collections.remove_item(collection_id, item_id, cx)
                });
            }
            LibraryPanelEvent::AddFolder {
                collection_id,
                parent,
                name,
            } => {
                let (collection_id, parent, name) = (*collection_id, *parent, name.clone());
                self.collections.update(cx, |collections, cx| {
                    collections.add_folder(collection_id, parent, name, cx)
                });
            }
            LibraryPanelEvent::AddRequest {
                collection_id,
                parent,
            } => self.add_saved_request(*collection_id, *parent, window, cx),
        }
    }

    fn add_saved_request(
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

    fn open_saved_request(
        &mut self,
        collection_id: Uuid,
        request_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let route = AppRoute::SavedRequest {
            collection_id,
            request_id,
        };
        // Already in the editor, maybe with unsaved changes: keep them.
        if !self.loads_into_editor(route) {
            self.navigate(route, window, cx);
            return;
        }
        if self.refuse_while_pending(window, cx) {
            return;
        }
        self.confirm_discard(
            move |this, window, cx| {
                let Some(request) = this
                    .find_saved_request(collection_id, request_id, cx)
                    .map(|saved| saved.request.clone())
                else {
                    this.sync_library_selection(window, cx);
                    return;
                };
                let content = EditorContent::Saved {
                    collection_id,
                    request_id,
                };
                this.load_editor(content, &request, window, cx);
                this.navigate(route, window, cx);
            },
            window,
            cx,
        );
    }

    /// Opens a blank, unlinked request on `/`.
    fn new_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_while_pending(window, cx) {
            return;
        }
        self.confirm_discard(
            |this, window, cx| {
                let blank = domain::request::Request::default();
                this.load_editor(EditorContent::Draft, &blank, window, cx);
                this.navigate(AppRoute::Request, window, cx);
                this.request_editor
                    .update(cx, |editor, cx| editor.focus_url(window, cx));
            },
            window,
            cx,
        );
    }

    /// Runs `then` if the editor has no unsaved changes; otherwise asks to save
    /// or discard them first, and drops `then` if the user cancels. Everything
    /// that replaces the editor's content goes through here.
    fn confirm_discard(
        &mut self,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dirty() || self.discard_confirmed {
            then(self, window, cx);
            return;
        }
        let name = match self.editor_content {
            EditorContent::Saved {
                collection_id,
                request_id,
            } => self
                .find_saved_request(collection_id, request_id, cx)
                .map(|saved| saved.name.clone()),
            EditorContent::Draft => None,
        };
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
            self.discard_confirmed = true;
            then(self, window, cx);
            self.discard_confirmed = false;
        } else {
            self.sync_library_selection(window, cx);
        }
    }

    /// Loads `request` into the editor and clears the response. Loading a draft
    /// replaces it; loading a saved request over the draft keeps a snapshot of it.
    fn load_editor(
        &mut self,
        content: EditorContent,
        request: &domain::request::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        debug_assert!(
            !self.is_dirty() || self.discard_confirmed,
            "unsaved changes replaced without confirm_discard"
        );
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
        self.response_view
            .update(cx, |view, cx| view.clear(window, cx));
        self.sync_editor_name(window, cx);
        self.refresh_dirty(window, cx);
    }

    /// Whether the editor holds a saved request with unsaved changes.
    fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Recomputes the cached dirty flag after an edit, a load, a save or a
    /// change to the collections.
    fn refresh_dirty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = match (self.editor_content, &self.baseline) {
            (
                EditorContent::Saved {
                    collection_id,
                    request_id,
                },
                Some(baseline),
            ) => self
                .find_saved_request(collection_id, request_id, cx)
                .is_some_and(|saved| {
                    let editor = self.request_editor.read(cx);
                    baseline.is_dirty(saved, &editor.build_request(cx), editor.name(cx).as_deref())
                }),
            _ => false,
        };
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
    fn shows_dirty_request(&self) -> bool {
        self.is_dirty() && self.current_route() == self.editor_route()
    }

    /// Shows a toast and returns `true` while a request is in flight.
    fn refuse_while_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
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

    fn find_saved_request<'a>(
        &self,
        collection_id: Uuid,
        request_id: Uuid,
        cx: &'a App,
    ) -> Option<&'a domain::collection::SavedRequest> {
        self.collections
            .read(cx)
            .get(collection_id)?
            .find_request(request_id)
    }

    /// Selects the library row for what the main pane shows.
    fn sync_library_selection(&mut self, window: &mut Window, cx: &mut App) {
        let selected = match self.current_route() {
            AppRoute::SavedRequest { request_id, .. } => Some(request_id),
            AppRoute::Collection(id) => Some(id),
            AppRoute::Request | AppRoute::Settings => None,
        };
        self.library_panel
            .update(cx, |panel, cx| panel.set_selected(selected, window, cx));
    }

    /// Hands the editor the stored name of the saved request it holds.
    fn sync_editor_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = match self.editor_content {
            EditorContent::Saved {
                collection_id,
                request_id,
            } => self
                .find_saved_request(collection_id, request_id, cx)
                .map(|saved| saved.name.clone()),
            EditorContent::Draft => None,
        };
        self.request_editor
            .update(cx, |editor, cx| editor.set_saved_name(name, window, cx));
    }

    /// Saves the editor's request if the main pane shows it.
    fn save_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == self.editor_route() {
            self.save_editor(window, cx);
        }
    }

    /// Saves the editor's request to its collection. Returns `false` if it
    /// isn't a saved request (any more). Writing to disk happens later and
    /// reports its own errors.
    fn save_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
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

    fn on_collection_view_event(
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

    fn on_history_panel_event(
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

    fn on_editor_event(
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
            RequestEditorEvent::Edited => self.refresh_dirty(window, cx),
        }
    }

    fn toggle_left_panel(&mut self, cx: &mut Context<Self>) {
        match self.left_panel {
            LeftPanelState::Closed => self.left_panel = LeftPanelState::Open,
            LeftPanelState::Open => self.left_panel = LeftPanelState::Closed,
        }
        let open = self.left_panel == LeftPanelState::Open;
        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_left_panel_open(open, cx)
        });
        cx.notify();
    }

    fn open_sidebar_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.set_active_tab(tab, cx));
        if self.left_panel == LeftPanelState::Closed {
            self.toggle_left_panel(cx);
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_content = self.routes(cx);

        // A hidden panel would still leave its resize handle behind.
        let body = if self.left_panel == LeftPanelState::Open {
            h_resizable("main-layout")
                .with_state(&self.main_layout)
                .child(
                    resizable_panel()
                        .size(px(280.))
                        .size_range(px(200.)..px(400.))
                        .fixed()
                        .child(self.sidebar.clone()),
                )
                .child(resizable_panel().child(main_content))
                .into_any_element()
        } else {
            main_content.into_any_element()
        };

        div()
            .id("workspace")
            .role(Role::Group)
            .aria_label("Workspace")
            .track_focus(&self.focus_handle)
            .key_context(WORKSPACE_CONTEXT)
            .on_action(cx.listener(|this, _: &SendRequest, _, cx| {
                this.request_editor.update(cx, |editor, cx| editor.send(cx));
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| {
                this.save_request(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewRequest, window, cx| {
                this.new_request(window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| {
                this.go(Direction::Back, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoForward, window, cx| {
                this.go(Direction::Forward, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_left_panel(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenLibrary, _, cx| {
                this.open_sidebar_tab(SidebarTab::Library, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenHistory, _, cx| {
                this.open_sidebar_tab(SidebarTab::History, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenTests, _, cx| {
                this.open_sidebar_tab(SidebarTab::Tests, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.navigate(AppRoute::Settings, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusUrl, window, cx| {
                this.request_editor
                    .update(cx, |editor, cx| editor.focus_url(window, cx));
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                window.focus(&this.focus_handle, cx);
            }))
            // Capture phase: disabled inputs and scrollbars stop mouse-downs before they bubble.
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                let direction = match event.button {
                    MouseButton::Navigate(NavigationDirection::Back) => Direction::Back,
                    MouseButton::Navigate(NavigationDirection::Forward) => Direction::Forward,
                    _ => return,
                };
                this.go(direction, window, cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                WindowTitleBar::new(self.title(cx))
                    .edited(self.shows_dirty_request())
                    .can_go_back(self.can_go(Direction::Back, cx))
                    .can_go_forward(self.can_go(Direction::Forward, cx))
                    .on_back(
                        cx.listener(|this, _, window, cx| this.go(Direction::Back, window, cx)),
                    )
                    .on_forward(
                        cx.listener(|this, _, window, cx| this.go(Direction::Forward, window, cx)),
                    )
                    .on_new_request(
                        cx.listener(|this, _, window, cx| this.new_request(window, cx)),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .child(self.status_bar.clone())
    }
}
