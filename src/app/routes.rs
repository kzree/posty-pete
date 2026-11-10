use std::rc::Rc;

use gpui::{App, Context, ParentElement, SharedString, Styled, Window, div};
use gpui_component::{resizable_panel, v_resizable};
use gpui_router::{Route, Routes, use_navigate};

use super::Workspace;
use crate::settings_page::SettingsPage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppRoute {
    Request,
    Settings,
}

impl AppRoute {
    fn path(&self) -> SharedString {
        match self {
            AppRoute::Request => "/".into(),
            AppRoute::Settings => "/settings".into(),
        }
    }
}

impl Workspace {
    pub(super) fn navigate(&mut self, route: AppRoute, window: &mut Window, cx: &mut App) {
        // The focused element may not exist on the new route, which would drop workspace keybindings.
        window.focus(&self.focus_handle, cx);
        use_navigate(window, cx).push(route.path());
    }

    pub(super) fn routes(&self, cx: &mut Context<Self>) -> Routes {
        Routes::new()
            .child(self.request_response_route())
            .child(self.settings_route(cx))
    }

    fn request_response_route(&self) -> Route {
        let request_editor = self.request_editor.clone();
        let response_view = self.response_view.clone();
        let loading_bar = self.loading_bar.clone();
        let layout = self.request_response_layout.clone();

        Route::new().index().element(move |_, _| {
            let split = v_resizable("request-response")
                .with_state(&layout)
                .child(resizable_panel().child(request_editor.clone()))
                .child(resizable_panel().child(response_view.clone()));

            div()
                .relative()
                .size_full()
                .child(split)
                .child(loading_bar.clone())
        })
    }

    fn settings_route(&self, cx: &mut Context<Self>) -> Route {
        let on_back = Rc::new(
            cx.listener(|this, _, window, cx| this.navigate(AppRoute::Request, window, cx)),
        );
        Route::new()
            .path("settings")
            .element(move |_, _| SettingsPage::new(on_back.clone()))
    }
}
