use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, WindowOptions, div, px, rgb, size,
};
use gpui_platform::application;

const TEXT_COLOR: u32 = 0xffffff;
const BACKGROUND_COLOR: u32 = 0x1e1e1e;

struct HelloWorld {
    text: SharedString,
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
            .child(format!("Hello, {}!", self.text))
    }
}

fn run() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|_| HelloWorld {
                    text: "World".into(),
                })
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
