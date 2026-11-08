use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};
use gpui_platform::application;

const WINDOW_TITLE: &str = "Posty pete";

const WINDOW_WIDTH: f32 = 1280.;
const WINDOW_HEIGHT: f32 = 720.;

struct HelloWorld {
    input: Entity<InputState>,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_xl()
            .child(div().w(px(400.)).child(Input::new(&self.input)))
            .child(
                Button::new("submit-btn")
                    .label("Submit")
                    .primary()
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        let val = this.input.read(cx).value();
                        println!("submitted: {val}")
                    })),
            )
    }
}

fn run() {
    application().run(|cx: &mut App| {
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
