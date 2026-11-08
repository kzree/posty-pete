use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Theme, ThemeMode, h_resizable, resizable_panel, v_resizable,
};
use gpui_kit_assets::Assets;
use gpui_platform::application;

mod domain;
mod request_editor;
mod response;
mod sidebar;
mod status_bar;

use sidebar::Sidebar;
use status_bar::StatusBar;

use crate::{
    request_editor::{RequestEditor, RequestEditorEvent},
    response::Response,
};

const WINDOW_TITLE: &str = "Posty pete";

const WINDOW_WIDTH: f32 = 1280.;
const WINDOW_HEIGHT: f32 = 720.;

struct BaseLayout {
    request_editor: Entity<RequestEditor>,
    _subscriptions: Vec<Subscription>,
}

impl BaseLayout {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request_editor = cx.new(|cx| RequestEditor::new(window, cx));

        let sub = cx.subscribe(&request_editor, |_this, _editor, event, _cx| match event {
            RequestEditorEvent::SendRequested(request) => {
                println!("send: {} {}", request.method.as_str(), request.url);
            }
        });
        Self {
            request_editor,
            _subscriptions: vec![sub],
        }
    }
}

impl Render for BaseLayout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_resizable("main-layout")
                    .child(
                        resizable_panel()
                            .size(px(280.))
                            .size_range(px(200.)..px(400.))
                            .fixed()
                            .child(Sidebar::new()),
                    )
                    .child(
                        resizable_panel().child(
                            v_resizable("request-response")
                                .child(resizable_panel().child(self.request_editor.clone()))
                                .child(resizable_panel().child(Response::new())),
                        ),
                    ),
            )
            .child(StatusBar::new())
    }
}

fn run() {
    application().with_assets(Assets).run(|cx: &mut App| {
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
