use gpui::{Anchor, App, AppContext, Bounds, WindowOptions, px, size};
use gpui_component::{Root, Theme, ThemeMode, TitleBar};
use gpui_platform::application;

mod actions;
mod app;
mod assets;
mod collection_view;
mod collections;
mod domain;
mod history;
mod menus;
mod request_editor;
mod response_view;
mod settings_page;
mod sidebar;
mod status_bar;
mod ui;
mod window_title_bar;

use app::Workspace;
use assets::AppAssets;

const WINDOW_TITLE: &str = "Posty pete";

const WINDOW_WIDTH: f32 = 1600.;
const WINDOW_HEIGHT: f32 = 900.;

fn run() {
    application().with_assets(AppAssets).run(|cx: &mut App| {
        gpui_component::init(cx);
        gpui_router::init(cx);
        actions::bind_keys(cx);
        menus::init(WINDOW_TITLE, cx);
        app::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        Theme::global_mut(cx).notification.placement = Anchor::BottomRight;

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(WINDOW_TITLE.into()),
                    ..TitleBar::title_bar_options()
                }),
                ..TitleBar::window_options()
            },
            |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
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
