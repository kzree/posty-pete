use gpui::{Context, Window};
use uuid::Uuid;

use super::{Workspace, routes::AppRoute};

impl Workspace {
    /// `None` = no environment.
    pub(super) fn set_active_environment(
        &mut self,
        collection_id: Uuid,
        environment_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.environments
            .set_active_environment(collection_id, environment_id);
        self.environments_changed(window, cx);
    }

    /// Resolves drafts against `collection_id` (`None` = no variables), with
    /// `environment_id` as its active environment.
    #[allow(dead_code)] // Until the environment picker (task 53).
    pub(super) fn set_draft_collection(
        &mut self,
        collection_id: Option<Uuid>,
        environment_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.environments
            .set_draft_collection(collection_id, environment_id);
        self.environments_changed(window, cx);
    }

    /// Opens the collection's page on the Environments tab, with
    /// `environment_id` selected if given.
    pub(super) fn open_environments(
        &mut self,
        collection_id: Uuid,
        environment_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let route = AppRoute::Collection(collection_id);
        if self.current_route() == route {
            // Collections emits `Changed` later; show its current data now.
            self.show_collection(collection_id, window, cx);
        } else {
            self.navigate(route, window, cx);
        }
        if self.collection_view.read(cx).id() == Some(collection_id) {
            self.collection_view.update(cx, |view, cx| {
                view.open_environments(environment_id, window, cx)
            });
        }
    }

    pub(super) fn add_environment(
        &mut self,
        collection_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.collections.update(cx, |collections, cx| {
            collections.add_environment(collection_id, cx)
        });
        if let Some(id) = id {
            self.open_environments(collection_id, Some(id), window, cx);
            self.collection_view
                .update(cx, |view, cx| view.focus_environment_variables(window, cx));
        }
    }

    pub(super) fn delete_environment(
        &mut self,
        collection_id: Uuid,
        environment_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.collections.update(cx, |collections, cx| {
            collections.delete_environment(collection_id, environment_id, cx)
        });
        if self.environments.active_environment_id(collection_id) == Some(environment_id) {
            self.set_active_environment(collection_id, None, window, cx);
        }
    }

    /// Shows collection `id` in the collection view.
    pub(super) fn show_collection(
        &mut self,
        id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collection = self.collections.read(cx).shared(id);
        let active = collection
            .as_deref()
            .and_then(|collection| self.environments.active_environment(collection));
        self.collection_view
            .update(cx, |view, cx| view.show(id, collection, active, window, cx));
    }

    /// Brings the collection view in line with the stored collections.
    pub(super) fn refresh_collection_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.collection_view.read(cx).id() {
            self.show_collection(id, window, cx);
        }
    }

    fn environments_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_collection_view(window, cx);
        self.refresh_dirty(window, cx);
        self.schedule_navigation_save(cx);
        cx.notify();
    }
}
