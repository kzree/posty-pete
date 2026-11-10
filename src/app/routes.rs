use std::rc::Rc;

use gpui::{App, Context, Div, ParentElement, SharedString, Styled, Window, div};
use gpui_component::{resizable_panel, v_resizable};
use gpui_router::{Route, Routes, use_navigate};
use uuid::Uuid;

use super::{
    EditorContent, Workspace,
    navigation::{Direction, NavHistory},
};
use crate::settings_page::SettingsPage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppRoute {
    /// The unsaved draft.
    Request,
    SavedRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
    Settings,
    Collection(Uuid),
}

impl AppRoute {
    fn path(&self) -> SharedString {
        match self {
            AppRoute::Request => "/".into(),
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => format!("/collections/{collection_id}/requests/{request_id}").into(),
            AppRoute::Settings => "/settings".into(),
            AppRoute::Collection(id) => format!("/collections/{id}").into(),
        }
    }

    fn shows_request(&self) -> bool {
        matches!(self, AppRoute::Request | AppRoute::SavedRequest { .. })
    }
}

pub(super) type RouteHistory = NavHistory<AppRoute>;

impl Workspace {
    pub(super) fn current_route(&self) -> AppRoute {
        *self.route_history.current()
    }

    /// Shows `route` and records it in the history.
    pub(super) fn navigate(
        &mut self,
        route: AppRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.current_route();
        if self.route_history.push(route) {
            use_navigate(window, cx).push(route.path());
        }
        self.show_current_route(previous, window, cx);
    }

    /// Shows `route` in place of the current history entry.
    pub(super) fn replace_route(
        &mut self,
        route: AppRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.current_route();
        self.route_history.replace_current(route);
        use_navigate(window, cx).replace(route.path());
        self.show_current_route(previous, window, cx);
    }

    /// Goes back or forward, skipping entries whose collection or request is gone.
    pub(super) fn go(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some((steps, target)) = self
            .route_history
            .peek(direction, |route| self.route_exists(*route, cx))
            .map(|(steps, route)| (steps, *route))
        else {
            return;
        };
        if self.loads_into_editor(target) && self.refuse_while_pending(window, cx) {
            return;
        }
        let previous = self.current_route();
        self.route_history.go(direction, steps);
        let mut navigator = use_navigate(window, cx);
        for _ in 0..steps {
            match direction {
                Direction::Back => navigator.back(),
                Direction::Forward => navigator.forward(),
            }
        }
        self.show_current_route(previous, window, cx);
    }

    /// The route of what the editor holds.
    pub(super) fn editor_route(&self) -> AppRoute {
        match self.editor_content {
            EditorContent::Draft => AppRoute::Request,
            EditorContent::Saved {
                collection_id,
                request_id,
            } => AppRoute::SavedRequest {
                collection_id,
                request_id,
            },
        }
    }

    /// Goes back if possible, otherwise to the editor's route, which needs no load.
    pub(super) fn go_back_or_to_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_go(Direction::Back, cx) {
            self.go(Direction::Back, window, cx);
        } else {
            self.navigate(self.editor_route(), window, cx);
        }
    }

    pub(super) fn can_go(&self, direction: Direction, cx: &App) -> bool {
        self.route_history
            .can_go(direction, |route| self.route_exists(*route, cx))
    }

    pub(super) fn route_exists(&self, route: AppRoute, cx: &App) -> bool {
        match route {
            AppRoute::Request | AppRoute::Settings => true,
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => self
                .find_saved_request(collection_id, request_id, cx)
                .is_some(),
            AppRoute::Collection(id) => self.collections.read(cx).get(id).is_some(),
        }
    }

    fn loads_into_editor(&self, route: AppRoute) -> bool {
        match route {
            AppRoute::Request => self.editor_content != EditorContent::Draft,
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => {
                self.editor_content
                    != EditorContent::Saved {
                        collection_id,
                        request_id,
                    }
            }
            AppRoute::Settings | AppRoute::Collection(_) => false,
        }
    }

    /// Brings the main pane in line with the current route. The editor keeps
    /// what it holds if that already belongs to the route.
    fn show_current_route(
        &mut self,
        previous: AppRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let route = self.current_route();
        // The focused element may not exist on the new route, which would drop workspace keybindings.
        // Request routes share their element, so focus (e.g. in the library tree) stays put.
        if !(previous.shows_request() && route.shows_request()) {
            window.focus(&self.focus_handle, cx);
        }
        match route {
            AppRoute::Request => {
                if self.loads_into_editor(route) {
                    let draft = self.draft.take().unwrap_or_default();
                    self.load_editor(EditorContent::Draft, &draft, window, cx);
                }
            }
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => {
                if self.loads_into_editor(route)
                    && let Some(request) = self
                        .find_saved_request(collection_id, request_id, cx)
                        .map(|saved| saved.request.clone())
                {
                    let content = EditorContent::Saved {
                        collection_id,
                        request_id,
                    };
                    self.load_editor(content, &request, window, cx);
                }
            }
            AppRoute::Collection(id) => {
                let collection = self.collections.read(cx).get(id).cloned();
                self.collection_view
                    .update(cx, |view, cx| view.show(id, collection, window, cx));
            }
            AppRoute::Settings => {}
        }
        self.sync_library_selection(window, cx);
        self.sync_editor_name(window, cx);
        cx.notify();
    }

    pub(super) fn routes(&self, cx: &mut Context<Self>) -> Routes {
        Routes::new()
            .child(self.request_response_route())
            .child(self.saved_request_route())
            .child(self.settings_route(cx))
            .child(self.collection_route())
    }

    fn request_response_route(&self) -> Route {
        Route::new()
            .index()
            .element(self.request_response_element())
    }

    fn saved_request_route(&self) -> Route {
        // Keeps `:id` from `collections/:id`; matchit may reject a differently named parameter there.
        Route::new()
            .path("collections/:id/requests/:request_id")
            .element(self.request_response_element())
    }

    fn request_response_element(&self) -> impl Fn(&mut Window, &mut App) -> Div + 'static {
        let request_editor = self.request_editor.clone();
        let response_view = self.response_view.clone();
        let loading_bar = self.loading_bar.clone();
        let layout = self.request_response_layout.clone();

        move |_, _| {
            let split = v_resizable("request-response")
                .with_state(&layout)
                .child(resizable_panel().child(request_editor.clone()))
                .child(resizable_panel().child(response_view.clone()));

            div()
                .relative()
                .size_full()
                .child(split)
                .child(loading_bar.clone())
        }
    }

    fn settings_route(&self, cx: &mut Context<Self>) -> Route {
        let on_back =
            Rc::new(cx.listener(|this, _, window, cx| this.go_back_or_to_editor(window, cx)));
        Route::new()
            .path("settings")
            .element(move |_, _| SettingsPage::new(on_back.clone()))
    }

    fn collection_route(&self) -> Route {
        let collection_view = self.collection_view.clone();
        Route::new()
            .path("collections/:id")
            .element(move |_, _| collection_view.clone())
    }
}
