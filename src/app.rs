use std::time::Duration;

use chrono::Utc;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, Role, StatefulInteractiveElement, Styled, Subscription, Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme, ResizableState, WindowExt, h_resizable, input::Escape, notification::Notification,
    resizable_panel, v_resizable,
};
use tokio::task::JoinError;

use crate::{
    actions::{
        FocusUrl, OpenHistory, OpenLibrary, OpenSettings, OpenTests, SendRequest, ToggleSidebar,
        WORKSPACE_CONTEXT,
    },
    domain::{
        self, curl, http,
        request::Request,
        response::Response,
        store::history::{HistoryRecord, Outcome},
    },
    history::{History, HistoryEvent, OpenMode},
    request_editor::{RequestEditor, RequestEditorEvent},
    response_view::ResponseView,
    sidebar::{
        Sidebar, SidebarTab,
        history_panel::{HistoryPanel, HistoryPanelEvent},
    },
    status_bar::StatusBar,
    ui::{
        clipboard::copy_with_notification,
        loading_bar::{GlobalLoading, LoadingBar},
    },
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

fn flatten_send_result(
    joined: Result<anyhow::Result<Response>, JoinError>,
) -> Result<Response, String> {
    match joined {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(format!("task failed: {e}")),
    }
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
    loading_bar: Entity<LoadingBar>,
    client: reqwest::Client,
    history: Entity<History>,
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
        let sidebar = cx.new(|_| Sidebar::new(history_panel.clone()));
        let loading_bar = cx.new(LoadingBar::new);
        let history = cx.new(History::new);

        let history_sub = cx.subscribe_in(
            &history,
            window,
            |this, _, event: &HistoryEvent, window, cx| match event {
                HistoryEvent::Loaded(entries) => this
                    .history_panel
                    .update(cx, |panel, cx| panel.set_entries(entries.clone(), cx)),
                HistoryEvent::Recorded(summary) => this
                    .history_panel
                    .update(cx, |panel, cx| panel.push(summary.clone(), window, cx)),
                HistoryEvent::Opened { .. } if this.pending.is_some() => {
                    window.push_notification(
                        Notification::info("Wait for it to finish, then try again.")
                            .title("A request is still running"),
                        cx,
                    );
                }
                HistoryEvent::Opened { request, mode } => {
                    this.request_editor
                        .update(cx, |editor, cx| editor.load_request(request, window, cx));
                    this.response_view
                        .update(cx, |view, cx| view.clear(window, cx));
                    if *mode == OpenMode::Send {
                        this.request_editor.update(cx, |editor, cx| editor.send(cx));
                    }
                }
                HistoryEvent::Deleted(id) => this
                    .history_panel
                    .update(cx, |panel, cx| panel.remove(*id, window, cx)),
                HistoryEvent::Cleared => this
                    .history_panel
                    .update(cx, |panel, cx| panel.clear(window, cx)),
                HistoryEvent::Failed { title, error } => show_error(title, error, window, cx),
            },
        );

        let history_panel_sub = cx.subscribe_in(
            &history_panel,
            window,
            |this, _, event: &HistoryPanelEvent, window, cx| match event {
                HistoryPanelEvent::Open(id) => {
                    let id = *id;
                    this.history
                        .update(cx, |history, cx| history.open(id, OpenMode::Load, cx));
                }
                HistoryPanelEvent::SendAgain(id) => {
                    let id = *id;
                    this.history
                        .update(cx, |history, cx| history.open(id, OpenMode::Send, cx));
                }
                HistoryPanelEvent::CopyAsCurl(id) => {
                    let request = this.history.read(cx).request(*id, cx);
                    let id = *id;
                    cx.spawn_in(window, async move |this, cx| {
                        let request = request.await;
                        this.update_in(cx, |this, window, cx| match request {
                            Ok(Some(request)) => copy_with_notification(
                                curl::to_curl(&request),
                                "Copied as cURL",
                                window,
                                cx,
                            ),
                            Ok(None) => this
                                .history_panel
                                .update(cx, |panel, cx| panel.remove(id, window, cx)),
                            Err(error) => show_error("Couldn't copy as cURL", &error, window, cx),
                        })
                        .ok();
                    })
                    .detach();
                }
                HistoryPanelEvent::Delete(id) => {
                    let id = *id;
                    this.history
                        .update(cx, |history, cx| history.delete(id, cx));
                }
                HistoryPanelEvent::Clear => {
                    this.history.update(cx, |history, cx| history.clear(cx));
                }
            },
        );

        let sub = cx.subscribe_in(
            &request_editor,
            window,
            |this, _editor, event, window, cx| match event {
                RequestEditorEvent::SendRequested(request) => {
                    this.send_request(request.clone(), window, cx)
                }
            },
        );

        Self {
            request_editor,
            response_view,
            status_bar,
            sidebar,
            history_panel,
            loading_bar,
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .redirect(domain::http::redirect_policy())
                .build()
                .expect("failed to build HTTP client"),
            history,
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![sub, history_sub, history_panel_sub],
        }
    }

    fn send_request(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        GlobalLoading::begin(cx);
        self.request_editor
            .update(cx, |editor, cx| editor.set_loading(true, cx));

        let sent_at = Utc::now();
        let handle = http::spawn_send(self.client.clone(), request.clone());
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            let result = flatten_send_result(handle.await);
            let outcome = result.as_ref().ok().map(|r| Outcome {
                status: r.status,
                elapsed: r.elapsed,
            });
            let (result, body_text) = cx
                .background_spawn(async move {
                    let text = result
                        .as_ref()
                        .map(|resp| resp.pretty_body())
                        .unwrap_or_default();
                    (result, text)
                })
                .await;
            let record = HistoryRecord {
                sent_at,
                request,
                outcome,
            };
            this.update_in(cx, |this, window, cx| {
                this.finish_request(result, body_text, record, window, cx)
            })
            .ok();
        }));
    }

    fn finish_request(
        &mut self,
        result: Result<Response, String>,
        body_text: String,
        record: HistoryRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.response_view.update(cx, |view, cx| {
            view.set_response(result, body_text, window, cx)
        });
        self.request_editor
            .update(cx, |editor, cx| editor.set_loading(false, cx));
        GlobalLoading::end(cx);
        self.history
            .update(cx, |history, cx| history.record(record, cx));
        self.pending = None;
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
        let main_content = v_resizable("request-response")
            .with_state(&self.request_response_layout)
            .child(resizable_panel().child(self.request_editor.clone()))
            .child(resizable_panel().child(self.response_view.clone()));

        let main_content = div()
            .relative()
            .size_full()
            .child(main_content)
            .child(self.loading_bar.clone());

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
            .on_action(cx.listener(|_, _: &OpenSettings, _, _| {
                tracing::info!("settings page isn't implemented yet");
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
