use gpui::{Anchor, App, AppContext, Bounds, WindowOptions, px, size};
use gpui_component::{Root, Theme, TitleBar};
use gpui_platform::application;

mod actions;
mod app;
mod assets;
mod collection_view;
mod collections;
mod command_palette;
mod environment_picker;
mod history;
mod import_dialog;
mod menus;
mod request_editor;
mod response_view;
mod save_as_dialog;
mod save_queue;
mod settings;
mod settings_page;
mod shared_environments;
mod shared_environments_view;
mod sidebar;
mod spec_update_dialog;
mod status_bar;
mod theme;
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
        settings::AppSettings::init(cx);
        theme::init(cx);
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
                theme::follow_system_appearance(window).detach();
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
