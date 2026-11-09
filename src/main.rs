use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Role, StatefulInteractiveElement, Styled, Subscription, Task, Window,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, ResizableState, Root, Theme, ThemeMode, h_resizable, input::Escape,
    resizable_panel, v_resizable,
};
use gpui_platform::application;

mod actions;
mod assets;
mod domain;
mod request_editor;
mod response_view;
mod sidebar;
mod status_bar;
mod ui;

use actions::{
    FocusUrl, OpenHistory, OpenLibrary, OpenSettings, SendRequest, ToggleSidebar, WORKSPACE_CONTEXT,
};
use assets::AppAssets;
use sidebar::{Sidebar, SidebarTab};
use status_bar::StatusBar;

use crate::{
    domain::http,
    request_editor::{RequestEditor, RequestEditorEvent},
    response_view::ResponseView,
    ui::loading_bar::{GlobalLoading, LoadingBar},
};

const WINDOW_TITLE: &str = "Posty pete";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

const WINDOW_WIDTH: f32 = 1600.;
const WINDOW_HEIGHT: f32 = 900.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeftPanelState {
    Closed,
    Open,
}

struct BaseLayout {
    request_editor: Entity<RequestEditor>,
    response_view: Entity<ResponseView>,
    status_bar: Entity<StatusBar>,
    sidebar: Entity<Sidebar>,
    loading_bar: Entity<LoadingBar>,
    client: reqwest::Client,
    pending: Option<Task<()>>,
    left_panel: LeftPanelState,
    main_layout: Entity<ResizableState>,
    request_response_layout: Entity<ResizableState>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl BaseLayout {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request_editor = cx.new(|cx| RequestEditor::new(window, cx));
        let response_view = cx.new(|cx| ResponseView::new(window, cx));
        let left_panel = LeftPanelState::Open;
        let status_bar = cx.new(|_| StatusBar::new(left_panel == LeftPanelState::Open));
        let sidebar = cx.new(|_| Sidebar::default());
        let loading_bar = cx.new(LoadingBar::new);

        let sub = cx.subscribe_in(
            &request_editor,
            window,
            |this, _editor, event, window, cx| match event {
                RequestEditorEvent::SendRequested(request) => {
                    if this.pending.is_some() {
                        return;
                    }
                    GlobalLoading::begin(cx);
                    this.request_editor
                        .update(cx, |editor, cx| editor.set_loading(true, cx));

                    let handle = http::spawn_send(this.client.clone(), request.clone());
                    this.pending = Some(cx.spawn_in(window, async move |this, cx| {
                        let result = match handle.await {
                            Ok(Ok(resp)) => Ok(resp),
                            Ok(Err(e)) => Err(format!("{e:#}")),
                            Err(e) => Err(format!("task failed: {e}")),
                        };
                        this.update_in(cx, |this, window, cx| {
                            this.response_view
                                .update(cx, |view, cx| view.set_response(result, window, cx));
                            this.request_editor
                                .update(cx, |editor, cx| editor.set_loading(false, cx));
                            GlobalLoading::end(cx);
                            this.pending = None;
                        })
                        .ok();
                    }));
                }
            },
        );

        Self {
            request_editor,
            response_view,
            status_bar,
            sidebar,
            loading_bar,
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("failed to build HTTP client"),
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![sub],
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

impl Render for BaseLayout {
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
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.open_sidebar_tab(SidebarTab::Settings, cx);
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

fn run() {
    application().with_assets(AppAssets).run(|cx: &mut App| {
        gpui_component::init(cx);
        actions::bind_keys(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(WINDOW_TITLE.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| BaseLayout::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .unwrap();

        cx.activate(true);
    });
}

fn main() {
    tracing_subscriber::fmt::init();
    run();
}
