use domain::collection::{Destination, MovedRequest};
use gpui::{Context, Window};
use gpui_router::use_navigate;
use uuid::Uuid;

use super::{EditorContent, Workspace, routes::AppRoute};

impl AppRoute {
    /// The route pointing at the moved request's new collection.
    pub(super) fn with_move(self, moved: MovedRequest) -> Self {
        match self {
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => AppRoute::SavedRequest {
                collection_id: moved.collection_of(collection_id, request_id),
                request_id,
            },
            route => route,
        }
    }
}

impl EditorContent {
    /// The content pointing at the moved request's new collection.
    pub(super) fn with_move(self, moved: MovedRequest) -> Self {
        match self {
            EditorContent::Saved {
                collection_id,
                request_id,
            } => EditorContent::Saved {
                collection_id: moved.collection_of(collection_id, request_id),
                request_id,
            },
            EditorContent::Draft => EditorContent::Draft,
        }
    }
}

impl Workspace {
    pub(super) fn move_request(
        &mut self,
        collection_id: Uuid,
        request_id: Uuid,
        to: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved = self.collections.update(cx, |collections, cx| {
            collections.move_request(collection_id, request_id, to, cx)
        });
        if moved && to.collection_id != collection_id {
            let moved = MovedRequest {
                request_id,
                from: collection_id,
                to: to.collection_id,
            };
            self.request_moved(moved, window, cx);
        }
    }

    /// Points everything that refers to the moved request at its new
    /// collection: the editor, its kept response, the back/forward history
    /// (also as stored), and the recents. Runs before the collections'
    /// `Changed` is handled, so the editor's request isn't taken for deleted.
    fn request_moved(&mut self, moved: MovedRequest, window: &mut Window, cx: &mut Context<Self>) {
        let old = EditorContent::Saved {
            collection_id: moved.from,
            request_id: moved.request_id,
        };
        self.responses.rekey(old, old.with_move(moved));
        self.editor_content = self.editor_content.with_move(moved);

        let current = self.current_route();
        self.route_history
            .map_entries(|route| route.with_move(moved));
        if self.current_route() != current {
            use_navigate(window, cx).replace(self.current_route().path());
        }
        self.recents.apply_move(moved);
        self.persisted_navigation.apply_move(moved);
        self.schedule_navigation_save(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved() -> MovedRequest {
        MovedRequest {
            request_id: Uuid::new_v4(),
            from: Uuid::new_v4(),
            to: Uuid::new_v4(),
        }
    }

    #[test]
    fn routes_follow_only_the_moved_request() {
        let moved = moved();
        let saved = |collection_id, request_id| AppRoute::SavedRequest {
            collection_id,
            request_id,
        };
        assert_eq!(
            saved(moved.from, moved.request_id).with_move(moved),
            saved(moved.to, moved.request_id)
        );
        let sibling = saved(moved.from, Uuid::new_v4());
        assert_eq!(sibling.with_move(moved), sibling);
        let elsewhere = saved(Uuid::new_v4(), moved.request_id);
        assert_eq!(elsewhere.with_move(moved), elsewhere);
        for route in [
            AppRoute::Request,
            AppRoute::Settings,
            AppRoute::SharedEnvironments,
            AppRoute::Collection(moved.from),
        ] {
            assert_eq!(route.with_move(moved), route);
        }
    }

    #[test]
    fn editor_content_follows_only_the_moved_request() {
        let moved = moved();
        let saved = |collection_id, request_id| EditorContent::Saved {
            collection_id,
            request_id,
        };
        assert_eq!(
            saved(moved.from, moved.request_id).with_move(moved),
            saved(moved.to, moved.request_id)
        );
        let sibling = saved(moved.from, Uuid::new_v4());
        assert_eq!(sibling.with_move(moved), sibling);
        assert_eq!(EditorContent::Draft.with_move(moved), EditorContent::Draft);
    }
}
