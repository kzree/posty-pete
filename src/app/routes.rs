use std::{rc::Rc, sync::Arc};

use gpui::{App, Context, Div, ParentElement, SharedString, Styled, Window, div};
use gpui_component::{resizable_panel, v_resizable};
use gpui_router::{Route, Routes, use_navigate};
use uuid::Uuid;

use super::{
    EditorContent, Workspace,
    navigation::{Direction, NavHistory},
};
use crate::{
    domain::collection::Collection, settings_page::SettingsPage, window_title_bar::WindowTitle,
};

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
    SharedEnvironments,
}

impl AppRoute {
    pub(super) fn path(&self) -> SharedString {
        match self {
            AppRoute::Request => "/".into(),
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => format!("/collections/{collection_id}/requests/{request_id}").into(),
            AppRoute::Settings => "/settings".into(),
            AppRoute::Collection(id) => format!("/collections/{id}").into(),
            AppRoute::SharedEnvironments => "/environments".into(),
        }
    }

    fn shows_request(&self) -> bool {
        matches!(self, AppRoute::Request | AppRoute::SavedRequest { .. })
    }
}

/// The title for `route`, from the stored names in `collections`.
pub(super) fn route_title(route: AppRoute, collections: &[Arc<Collection>]) -> WindowTitle {
    let find = |id| collections.iter().find(|collection| collection.id == id);
    match route {
        AppRoute::Request => WindowTitle::new("Untitled request"),
        AppRoute::SavedRequest {
            collection_id,
            request_id,
        } => find(collection_id)
            .and_then(|collection| {
                let saved = collection.find_request(request_id)?;
                Some(WindowTitle::with_context(
                    collection.name.clone(),
                    saved.name.clone(),
                ))
            })
            .unwrap_or_else(|| WindowTitle::new(crate::WINDOW_TITLE)),
        AppRoute::Collection(id) => find(id)
            .map(|collection| WindowTitle::new(collection.name.clone()))
            .unwrap_or_else(|| WindowTitle::new(crate::WINDOW_TITLE)),
        AppRoute::Settings => WindowTitle::new("Settings"),
        AppRoute::SharedEnvironments => WindowTitle::new("Shared environments"),
    }
}

impl EditorContent {
    pub(super) fn route(self) -> AppRoute {
        match self {
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
}

pub(super) type RouteHistory = NavHistory<AppRoute>;

impl Workspace {
    pub(super) fn current_route(&self) -> AppRoute {
        *self.route_history.current()
    }

    pub(super) fn title(&self, cx: &App) -> WindowTitle {
        route_title(self.current_route(), self.collections.read(cx).all())
    }

    /// Keeps the OS window title and edited dot in step. Each is an AppKit call
    /// on macOS, so it only happens when the value changes.
    pub(super) fn sync_window_title(&mut self, window: &mut Window, cx: &App) {
        let title = self.title(cx).os_title();
        if self.os_window_title != title {
            window.set_window_title(&title);
            self.os_window_title = title;
        }
        let edited = self.is_dirty();
        if self.os_window_edited != edited {
            window.set_window_edited(edited);
            self.os_window_edited = edited;
        }
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
        if !self.loads_into_editor(target) {
            self.move_in_history(direction, steps, window, cx);
            return;
        }
        if self.refuse_while_pending(window, cx) {
            return;
        }
        self.confirm_discard(
            move |this, window, cx| {
                let still = this
                    .route_history
                    .peek(direction, |route| this.route_exists(*route, cx))
                    .map(|(steps, route)| (steps, *route));
                if still == Some((steps, target)) {
                    this.move_in_history(direction, steps, window, cx);
                }
            },
            window,
            cx,
        );
    }

    fn move_in_history(
        &mut self,
        direction: Direction,
        steps: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.editor_content.route()
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
            AppRoute::Request | AppRoute::Settings | AppRoute::SharedEnvironments => true,
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => self
                .find_saved_request(collection_id, request_id, cx)
                .is_some(),
            AppRoute::Collection(id) => self.collections.read(cx).get(id).is_some(),
        }
    }

    pub(super) fn loads_into_editor(&self, route: AppRoute) -> bool {
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
            AppRoute::Settings | AppRoute::Collection(_) | AppRoute::SharedEnvironments => false,
        }
    }

    /// Brings the main pane in line with the current route. The editor keeps
    /// what it holds if that already belongs to the route.
    pub(super) fn show_current_route(
        &mut self,
        previous: AppRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_pending_variables(cx);
        let route = self.current_route();
        // The focused element may not exist on the new route, which would drop workspace keybindings.
        // Request routes share their element, so focus (e.g. in the library tree) stays put.
        if !(previous.shows_request() && route.shows_request()) {
            window.focus(&self.focus_handle, cx);
        }
        match route {
            AppRoute::Request | AppRoute::SavedRequest { .. } => {
                self.load_route_into_editor(route, window, cx)
            }
            AppRoute::Collection(id) => self.show_collection(id, window, cx),
            AppRoute::SharedEnvironments => self.refresh_shared_environments_view(window, cx),
            AppRoute::Settings => {}
        }
        self.sync_library_selection(window, cx);
        self.sync_editor_name(window, cx);
        self.sync_window_title(window, cx);
        self.schedule_navigation_save(cx);
        cx.notify();
    }

    /// Loads the draft or saved request of `route`, unless the editor already holds it.
    fn load_route_into_editor(
        &mut self,
        route: AppRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.loads_into_editor(route) {
            return;
        }
        match route {
            AppRoute::Request => {
                let draft = self.draft.take().unwrap_or_default();
                self.load_editor(EditorContent::Draft, &draft, window, cx);
            }
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => {
                let Some(request) = self
                    .find_saved_request(collection_id, request_id, cx)
                    .map(|saved| saved.request.clone())
                else {
                    return;
                };
                let content = EditorContent::Saved {
                    collection_id,
                    request_id,
                };
                self.load_editor(content, &request, window, cx);
            }
            AppRoute::Settings | AppRoute::Collection(_) | AppRoute::SharedEnvironments => {}
        }
    }

    pub(super) fn routes(&self, cx: &mut Context<Self>) -> Routes {
        Routes::new()
            .child(self.request_response_route())
            .child(self.saved_request_route())
            .child(self.settings_route(cx))
            .child(self.collection_route())
            .child(self.shared_environments_route())
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

    fn shared_environments_route(&self) -> Route {
        let view = self.shared_environments_view.clone();
        Route::new()
            .path("environments")
            .element(move |_, _| view.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        collection::{Item, SavedRequest},
        request::Request,
    };

    fn library() -> (Vec<Arc<Collection>>, Uuid, Uuid) {
        let mut collection = Collection::new("My API");
        let saved = SavedRequest::new("Get users", Request::default());
        let request_id = saved.id;
        collection.add_item(None, Item::Request(saved));
        let collection_id = collection.id;
        (vec![Arc::new(collection)], collection_id, request_id)
    }

    #[test]
    fn draft_settings_and_shared_environments_have_fixed_titles() {
        let (collections, _, _) = library();
        assert_eq!(
            route_title(AppRoute::SharedEnvironments, &collections),
            WindowTitle::new("Shared environments")
        );
        assert_eq!(
            route_title(AppRoute::Request, &collections),
            WindowTitle::new("Untitled request")
        );
        assert_eq!(
            route_title(AppRoute::Settings, &collections),
            WindowTitle::new("Settings")
        );
    }

    #[test]
    fn saved_request_shows_its_collection_as_context() {
        let (collections, collection_id, request_id) = library();
        let route = AppRoute::SavedRequest {
            collection_id,
            request_id,
        };
        assert_eq!(
            route_title(route, &collections),
            WindowTitle::with_context("My API", "Get users")
        );
    }

    #[test]
    fn collection_shows_its_name() {
        let (collections, collection_id, _) = library();
        assert_eq!(
            route_title(AppRoute::Collection(collection_id), &collections),
            WindowTitle::new("My API")
        );
    }

    #[test]
    fn renamed_items_use_the_stored_name() {
        let (mut collections, collection_id, request_id) = library();
        Arc::make_mut(&mut collections[0]).rename(request_id, "List users");
        let route = AppRoute::SavedRequest {
            collection_id,
            request_id,
        };
        assert_eq!(route_title(route, &collections).name, "List users");
    }

    #[test]
    fn missing_items_fall_back_to_the_app_name() {
        let route = AppRoute::SavedRequest {
            collection_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
        };
        assert_eq!(
            route_title(route, &[]),
            WindowTitle::new(crate::WINDOW_TITLE)
        );
    }
}
