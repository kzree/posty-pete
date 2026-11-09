use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, ResizableState, Root, Theme, ThemeMode, h_resizable, resizable_panel, v_resizable,
};
use gpui_platform::application;

mod assets;
mod domain;
mod request_editor;
mod response_view;
mod sidebar;
mod status_bar;
mod ui;

use assets::AppAssets;
use sidebar::{Sidebar, SidebarTab};
use status_bar::{StatusBar, StatusBarEvent};

use crate::{
    domain::http,
    request_editor::{RequestEditor, RequestEditorEvent},
    response_view::ResponseView,
};

const WINDOW_TITLE: &str = "Posty pete";

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
    client: reqwest::Client,
    pending: Option<Task<()>>,
    left_panel: LeftPanelState,
    main_layout: Entity<ResizableState>,
    request_response_layout: Entity<ResizableState>,
    _subscriptions: Vec<Subscription>,
}

impl BaseLayout {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request_editor = cx.new(|cx| RequestEditor::new(window, cx));
        let response_view = cx.new(|cx| ResponseView::new(window, cx));
        let left_panel = LeftPanelState::Open;
        let status_bar = cx.new(|_| StatusBar::new(left_panel == LeftPanelState::Open));
        let sidebar = cx.new(|_| Sidebar::default());

        let sub = cx.subscribe_in(
            &request_editor,
            window,
            |this, _editor, event, window, cx| match event {
                RequestEditorEvent::SendRequested(request) => {
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
                            this.pending = None;
                        })
                        .ok();
                    }));
                }
            },
        );

        let status_sub = cx.subscribe(&status_bar, |this, _, event, cx| match event {
            StatusBarEvent::ToggleLeftPanel => this.toggle_left_panel(cx),
            StatusBarEvent::OpenSettings => this.open_settings(cx),
        });

        Self {
            request_editor,
            response_view,
            status_bar,
            sidebar,
            client: reqwest::Client::new(),
            pending: None,
            left_panel,
            main_layout: cx.new(|_| ResizableState::default()),
            request_response_layout: cx.new(|_| ResizableState::default()),
            _subscriptions: vec![sub, status_sub],
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

    /// Shows the sidebar's Settings tab, opening the sidebar if it's closed.
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_active_tab(SidebarTab::Settings, cx)
        });
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

        // Only build the horizontal split while the sidebar is open: a hidden
        // panel still leaves the next panel's resize handle in place.
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
