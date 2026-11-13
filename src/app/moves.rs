use domain::collection::{Destination, MovedItems};
use gpui::{Context, Window};
use gpui_router::use_navigate;
use uuid::Uuid;

use super::{EditorContent, Workspace, routes::AppRoute};

impl AppRoute {
    /// The route pointing at the moved request's new collection.
    pub(super) fn with_move(self, moved: &MovedItems) -> Self {
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
    pub(super) fn with_move(self, moved: &MovedItems) -> Self {
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
            let moved = MovedItems::one(request_id, collection_id, to.collection_id);
            self.items_moved(&moved, window, cx);
        }
    }

    pub(super) fn move_folder(
        &mut self,
        collection_id: Uuid,
        folder_id: Uuid,
        to: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved = self.collections.update(cx, |collections, cx| {
            collections.move_folder(collection_id, folder_id, to, cx)
        });
        if let Some(moved) = moved
            && moved.from != moved.to
        {
            self.items_moved(&moved, window, cx);
        }
    }

    /// Points everything that refers to the moved items at their new
    /// collection: the editor, kept responses, the back/forward history (also
    /// as stored), and the recents. Runs before the collections' `Changed` is
    /// handled, so the editor's request isn't taken for deleted.
    fn items_moved(&mut self, moved: &MovedItems, window: &mut Window, cx: &mut Context<Self>) {
        self.responses.map_keys(|content| content.with_move(moved));
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

    fn moved() -> (MovedItems, Uuid) {
        let request_id = Uuid::new_v4();
        let moved = MovedItems::one(request_id, Uuid::new_v4(), Uuid::new_v4());
        (moved, request_id)
    }

    #[test]
    fn routes_follow_only_the_moved_requests() {
        let (mut moved, request_id) = moved();
        let saved = |collection_id, request_id| AppRoute::SavedRequest {
            collection_id,
            request_id,
        };
        assert_eq!(
            saved(moved.from, request_id).with_move(&moved),
            saved(moved.to, request_id)
        );
        let sibling = saved(moved.from, Uuid::new_v4());
        assert_eq!(sibling.with_move(&moved), sibling);
        let elsewhere = saved(Uuid::new_v4(), request_id);
        assert_eq!(elsewhere.with_move(&moved), elsewhere);
        for route in [
            AppRoute::Request,
            AppRoute::Settings,
            AppRoute::SharedEnvironments,
            AppRoute::Collection(moved.from),
        ] {
            assert_eq!(route.with_move(&moved), route);
        }

        let AppRoute::SavedRequest {
            request_id: sibling_id,
            ..
        } = sibling
        else {
            unreachable!()
        };
        moved.item_ids.insert(sibling_id);
        assert_eq!(sibling.with_move(&moved), saved(moved.to, sibling_id));
    }

    #[test]
    fn editor_content_follows_only_the_moved_requests() {
        let (moved, request_id) = moved();
        let saved = |collection_id, request_id| EditorContent::Saved {
            collection_id,
            request_id,
        };
        assert_eq!(
            saved(moved.from, request_id).with_move(&moved),
            saved(moved.to, request_id)
        );
        let sibling = saved(moved.from, Uuid::new_v4());
        assert_eq!(sibling.with_move(&moved), sibling);
        assert_eq!(EditorContent::Draft.with_move(&moved), EditorContent::Draft);
    }
}
