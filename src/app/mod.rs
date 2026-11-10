mod routes;
mod send;

use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, Role, StatefulInteractiveElement, Styled, Subscription, Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme, ResizableState, WindowExt, h_resizable, input::Escape, notification::Notification,
    resizable_panel,
};

use self::routes::AppRoute;
use crate::{
    actions::{
        FocusUrl, OpenHistory, OpenLibrary, OpenSettings, OpenTests, SendRequest, ToggleSidebar,
        WORKSPACE_CONTEXT,
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
    ui::loading_bar::LoadingBar,
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
    current_route: AppRoute,
    pending: Option<Task<()>>,
    left_panel: LeftPanelState,
    main_layout: Entity<ResizableState>,
    request_response_layout: Entity<ResizableState>,
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

        Self {
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
            current_route: AppRoute::Request,
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
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
                self.request_editor
                    .update(cx, |editor, cx| editor.load_request(request, window, cx));
                self.response_view
                    .update(cx, |view, cx| view.clear(window, cx));
                if *mode == OpenMode::Send {
                    self.request_editor.update(cx, |editor, cx| editor.send(cx));
                }
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
                if let Some(id) = self.collection_view.read(cx).id() {
                    let collection = self.collections.read(cx).get(id).cloned();
                    self.collection_view
                        .update(cx, |view, cx| view.show(id, collection, window, cx));
                }
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
                if self.current_route == AppRoute::Collection(id) {
                    self.navigate(AppRoute::Request, window, cx);
                }
                self.collections
                    .update(cx, |collections, cx| collections.delete(id, cx));
            }
        }
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
            CollectionViewEvent::Back => self.navigate(AppRoute::Request, window, cx),
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
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .child(self.status_bar.clone())
    }
}
