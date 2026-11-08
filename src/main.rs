use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Theme, ThemeMode, h_resizable,
    input::{Input, InputState},
    resizable_panel,
};
use gpui_platform::application;

mod sidebar;
mod status_bar;

use sidebar::Sidebar;
use status_bar::StatusBar;

const WINDOW_TITLE: &str = "Posty pete";

const WINDOW_WIDTH: f32 = 1280.;
const WINDOW_HEIGHT: f32 = 720.;

struct BaseLayout {
    input: Entity<InputState>,
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
                        resizable_panel()
                            .p_4()
                            .child(div().w(px(400.)).child(Input::new(&self.input))),
                    ),
            )
            .child(StatusBar::new())
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
                let view = cx.new(|_| BaseLayout { input });

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
