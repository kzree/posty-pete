use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions, div, px, rgb, size,
};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use gpui_platform::application;

const TEXT_COLOR: u32 = 0xffffff;
const BACKGROUND_COLOR: u32 = 0x1e1e1e;
const WINDOW_TITLE: &str = "Posty pete";

const WINDOW_WIDTH: f32 = 1280.;
const WINDOW_HEIGHT: f32 = 720.;

struct HelloWorld {
    input: Entity<InputState>,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .bg(rgb(BACKGROUND_COLOR))
            .text_color(rgb(TEXT_COLOR))
            .text_xl()
            .child(
                div()
                    .w(px(400.))
                    .text_color(rgb(0x000000))
                    .child(Input::new(&self.input)),
            )
    }
}

fn run() {
    application().run(|cx: &mut App| {
        gpui_component::init(cx);
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
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Type here..."));
                let view = cx.new(|_| HelloWorld { input });

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
