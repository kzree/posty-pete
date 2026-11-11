use gpui::{Context, Window};
use uuid::Uuid;

use super::Workspace;

impl Workspace {
    /// `None` = no environment.
    #[allow(dead_code)] // Until the environment picker (task 53).
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

    fn environments_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_dirty(window, cx);
        self.schedule_navigation_save(cx);
        cx.notify();
    }
}
