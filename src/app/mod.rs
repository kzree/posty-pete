mod commands;
mod dirty;
mod editor;
mod environments;
mod events;
mod focus;
mod layout;
mod menu_fallback;
mod navigation;
mod persisted_navigation;
mod quit;
mod recents;
mod render;
mod response_cache;
mod routes;
mod save;
mod send;

use std::cell::Cell;

use domain::{
    environment_selection::EnvironmentSelection,
    layout::Layout,
    palette::Scope,
    recents::{Recent, Recents},
};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, ParentElement, Styled, Subscription, Window, px,
};
use gpui_component::{
    ResizablePanelEvent, ResizableState, Root, WindowExt, notification::Notification,
};
use uuid::Uuid;

use self::{
    dirty::{Baseline, PendingCheck},
    layout::OwnResizes,
    persisted_navigation::PersistedNavigation,
    response_cache::ResponseCache,
    routes::{AppRoute, RouteHistory},
};
use crate::{
    collection_view::CollectionView,
    collections::Collections,
    command_palette::CommandPalette,
    history::History,
    request_editor::RequestEditor,
    response_view::{ResponseView, ResponseViewEvent},
    settings::{AppSettings, SettingsEvent},
    settings_page::{SettingsPage, SettingsPageEvent},
    shared_environments::{SharedEnvironments, SharedEnvironmentsEvent},
    shared_environments_view::SharedEnvironmentsView,
    sidebar::{Sidebar, SidebarTab, history_panel::HistoryPanel, library_panel::LibraryPanel},
    status_bar::StatusBar,
    ui::loading_bar::LoadingBar,
};

pub fn init(cx: &mut App) {
    menu_fallback::init(cx);
    quit::init(cx);
}

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
    shared_environments_view: Entity<SharedEnvironmentsView>,
    settings_page: Entity<SettingsPage>,
    loading_bar: Entity<LoadingBar>,
    client: domain::http::SettingsClient,
    history: Entity<History>,
    collections: Entity<Collections>,
    shared_environments: Entity<SharedEnvironments>,
    route_history: RouteHistory,
    editor_content: EditorContent,
    /// The draft, kept while the editor holds a saved request.
    draft: Option<domain::request::Request>,
    environments: EnvironmentSelection,
    /// Set while the editor holds a saved request.
    baseline: Option<Baseline>,
    dirty: bool,
    /// Set after an edit until the dirty check runs.
    dirty_check: PendingCheck,
    responses: ResponseCache<EditorContent>,
    persisted_navigation: PersistedNavigation,
    recents: Recents,
    /// Set while the save prompt is open.
    discard_prompt: Option<Continuation>,
    /// Set while a continuation runs, after the user saved or discarded.
    discard_confirmed: bool,
    /// Set while quitting or closing the window waits for saves or asks about
    /// failed ones, so neither starts again.
    closing: bool,
    command_palette_subscription: Option<Subscription>,
    pending: Option<send::PendingSend>,
    left_panel: LeftPanelState,
    main_layout: Entity<ResizableState>,
    request_response_layout: Entity<ResizableState>,
    layout: Layout,
    /// The restored sidebar width, clamped to the window, for before the sidebar is laid out.
    sidebar_initial_width: f32,
    /// Set from restoring the layout until the split is laid out and restored.
    split_pending: bool,
    /// `Resized` events still to come from restoring the layout, which
    /// mustn't overwrite the stored sizes with the clamped ones.
    own_resizes: OwnResizes,
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
        let shared_environments_view = cx.new(|cx| SharedEnvironmentsView::new(window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(window, cx));
        let loading_bar = cx.new(|_| LoadingBar::default());
        let history = cx.new(History::new);
        let collections = cx.new(Collections::new);
        let shared_environments = cx.new(SharedEnvironments::new);

        let mut workspace = Self {
            request_editor,
            response_view,
            status_bar,
            sidebar,
            history_panel,
            library_panel,
            collection_view,
            shared_environments_view,
            settings_page,
            loading_bar,
            client: domain::http::SettingsClient::new(&AppSettings::get(cx).requests),
            history,
            collections,
            shared_environments,
            route_history: RouteHistory::new(AppRoute::Request),
            editor_content: EditorContent::Draft,
            draft: None,
            environments: EnvironmentSelection::default(),
            baseline: None,
            dirty: false,
            dirty_check: PendingCheck::default(),
            responses: ResponseCache::new(),
            persisted_navigation: PersistedNavigation::new(),
            recents: Recents::default(),
            discard_prompt: None,
            discard_confirmed: false,
            closing: false,
            command_palette_subscription: None,
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            layout: Layout::default(),
            sidebar_initial_width: domain::layout::SIDEBAR_DEFAULT_WIDTH,
            split_pending: false,
            own_resizes: OwnResizes::default(),
            os_window_title: String::new(),
            os_window_edited: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: Vec::new(),
        };
        workspace._subscriptions = workspace.subscribe(window, cx);
        workspace
            ._subscriptions
            .extend(Self::keep_focus(window, cx));
        let handle = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            handle
                .update(cx, |this, cx| this.should_close_window(window, cx))
                .unwrap_or(true)
        });
        Self::load_navigation(window, cx);
        workspace.sync_window_title(window, cx);
        // Restoring a non-request route moves focus to the Workspace.
        workspace
            .request_editor
            .update(cx, |editor, cx| editor.focus_url(window, cx));
        workspace
    }

    fn subscribe(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Subscription> {
        vec![
            cx.subscribe_in(&self.history, window, |this, _, event, window, cx| {
                this.on_history_event(event, window, cx)
            }),
            cx.subscribe_in(&self.collections, window, |this, _, event, window, cx| {
                this.on_collections_event(event, window, cx)
            }),
            cx.subscribe_in(
                &self.shared_environments,
                window,
                |this, _, event, window, cx| match event {
                    SharedEnvironmentsEvent::Changed => {
                        this.shared_environments_changed(window, cx)
                    }
                    SharedEnvironmentsEvent::Failed { title, error } => {
                        show_error(title, error, window, cx)
                    }
                },
            ),
            cx.subscribe_in(
                &AppSettings::saver(cx),
                window,
                |_, _, event, window, cx| match event {
                    SettingsEvent::Failed { title, error } => show_error(title, error, window, cx),
                },
            ),
            cx.observe_global::<AppSettings>(Self::request_settings_changed),
            cx.subscribe_in(&self.library_panel, window, |this, _, event, window, cx| {
                this.on_library_panel_event(event, window, cx)
            }),
            cx.subscribe_in(
                &self.collection_view,
                window,
                |this, _, event, window, cx| this.on_collection_view_event(event, window, cx),
            ),
            cx.subscribe_in(
                &self.shared_environments_view,
                window,
                |this, _, event, window, cx| {
                    this.on_shared_environments_view_event(event, window, cx)
                },
            ),
            cx.subscribe_in(&self.history_panel, window, |this, _, event, window, cx| {
                this.on_history_panel_event(event, window, cx)
            }),
            cx.observe(&self.history_panel, |this, panel, cx| {
                let empty = panel.read(cx).entries(cx).is_empty();
                this.settings_page
                    .update(cx, |page, cx| page.set_history_empty(empty, cx));
            }),
            cx.subscribe_in(
                &self.settings_page,
                window,
                |this, _, event, window, cx| match event {
                    SettingsPageEvent::ClearHistory => this
                        .history_panel
                        .update(cx, |panel, cx| panel.confirm_clear(window, cx)),
                },
            ),
            cx.subscribe_in(
                &self.request_editor,
                window,
                |this, _, event, window, cx| this.on_editor_event(event, window, cx),
            ),
            cx.subscribe(&self.main_layout, |this, _, _: &ResizablePanelEvent, cx| {
                this.sidebar_resized(cx)
            }),
            cx.subscribe(
                &self.request_response_layout,
                |this, _, _: &ResizablePanelEvent, cx| this.split_resized(cx),
            ),
            cx.observe_in(
                &self.request_response_layout,
                window,
                |this, _, window, cx| this.apply_pending_split(window, cx),
            ),
            cx.subscribe(&self.response_view, |this, _, event, _| match event {
                ResponseViewEvent::Cleared => this.responses.remove(this.editor_content),
            }),
            cx.on_app_quit(|this, cx| {
                this.save_navigation_now(cx);
                // Best effort: gpui waits only briefly for quit handlers.
                this.flush_pending_edits(cx);
                let saved = futures::future::join(
                    this.collections.read(cx).flushed(),
                    this.shared_environments.read(cx).flushed(),
                );
                async move {
                    saved.await;
                }
            }),
        ]
    }

    /// The Workspace shown in `window`, if any.
    fn in_window(window: &Window, cx: &App) -> Option<Entity<Self>> {
        let root = window.root::<Root>()??;
        root.read(cx).view().clone().downcast::<Self>().ok()
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
        let recent = Recent::Request {
            collection_id,
            request_id,
        };
        // Already in the editor, maybe with unsaved changes: keep them.
        if !self.loads_into_editor(route) {
            self.navigate(route, window, cx);
            self.record_recent(recent, cx);
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
                this.record_recent(recent, cx);
            },
            window,
            cx,
        );
    }

    fn open_collection(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(AppRoute::Collection(id), window, cx);
        self.record_recent(Recent::Collection { id }, cx);
    }

    /// Opens the settings on the page they last showed.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(AppRoute::Settings, window, cx);
        let section = self.settings_page.read(cx).section();
        self.record_recent(Recent::SettingsPage { section }, cx);
    }

    /// Opens a blank, unlinked request on `/`.
    fn new_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_while_pending(window, cx) {
            return;
        }
        self.confirm_discard(
            |this, window, cx| {
                let blank = domain::request::Request::default();
                this.responses.remove(EditorContent::Draft);
                this.load_editor(EditorContent::Draft, &blank, window, cx);
                this.navigate(AppRoute::Request, window, cx);
                this.request_editor
                    .update(cx, |editor, cx| editor.focus_url(window, cx));
            },
            window,
            cx,
        );
    }

    fn open_command_palette(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let mut entries = domain::palette::entries(
            self.collections.read(cx).all(),
            self.shared_environments.read(cx).all(),
        );
        let dirty = self.is_dirty_now(window, cx);
        let state = self.command_state(dirty, cx);
        entries.extend(domain::palette::command_entries(
            domain::command::available(&state),
        ));
        entries.extend(domain::palette::history_entries(
            self.history_panel.read(cx).entries(cx),
            &chrono::Local::now(),
        ));
        entries.extend(domain::palette::settings_entries());
        let recent = domain::palette::recent_entries(&entries, self.recents.all());
        entries.extend(recent);
        let focus = self.command_focus(window, cx);
        let palette = cx.new(|cx| CommandPalette::new(entries, scope, &focus, window, cx));
        self.command_palette_subscription = Some(cx.subscribe_in(
            &palette,
            window,
            |this, _, event, window, cx| this.on_command_palette_event(event, window, cx),
        ));
        // The builder reruns on every rebuild of the dialog layer.
        let initial_focus = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if initial_focus.replace(false) {
                palette.update(cx, |palette, cx| palette.focus(window, cx));
            }
            dialog
                .w(px(560.))
                .p_0()
                .close_button(false)
                .child(palette.clone())
        });
    }

    /// Opens the library with folder `folder_id` expanded to and selected.
    fn reveal_folder(
        &mut self,
        collection_id: Uuid,
        folder_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ancestors) = self
            .collections
            .read(cx)
            .get(collection_id)
            .and_then(|collection| collection.ancestors(folder_id))
        else {
            return;
        };
        self.open_sidebar_tab(SidebarTab::Library, cx);
        let expand = std::iter::once(collection_id).chain(ancestors);
        self.library_panel.update(cx, |panel, cx| {
            panel.select_row(folder_id, expand, window, cx)
        });
        self.record_recent(
            Recent::Folder {
                collection_id,
                folder_id,
            },
            cx,
        );
    }

    fn toggle_left_panel(&mut self, cx: &mut Context<Self>) {
        let panel = match self.left_panel {
            LeftPanelState::Closed => LeftPanelState::Open,
            LeftPanelState::Open => LeftPanelState::Closed,
        };
        self.set_left_panel(panel, cx);
    }

    fn set_left_panel(&mut self, panel: LeftPanelState, cx: &mut Context<Self>) {
        if self.left_panel == panel {
            return;
        }
        self.left_panel = panel;
        self.left_panel_changed(cx);
        self.schedule_navigation_save(cx);
    }

    fn left_panel_changed(&mut self, cx: &mut Context<Self>) {
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
