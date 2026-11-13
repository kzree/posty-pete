use domain::{collection::Destination, request::HttpMethod};
use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Point,
    Render, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use gpui_component::{ActiveTheme, h_flex, list::ListItem};
use uuid::Uuid;

use super::{
    LibraryPanel,
    tree::{Node, NodeKind},
};
use crate::ui::method_label::method_label;

/// The payload of a request row being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DraggedRequest {
    pub(super) collection_id: Uuid,
    pub(super) request_id: Uuid,
    /// Where it is now, to tell drops that wouldn't move it.
    parent: Option<Uuid>,
    next: Option<Uuid>,
    method: HttpMethod,
    name: SharedString,
}

impl DraggedRequest {
    /// `None` unless `node` is a request.
    fn of(node: &Node) -> Option<Self> {
        let NodeKind::Request(method) = node.kind else {
            return None;
        };
        Some(Self {
            collection_id: node.collection_id,
            request_id: node.id,
            parent: node.parent,
            next: node.next,
            method,
            name: node.name.clone(),
        })
    }

    /// Whether dropping at `to` would leave it where it is.
    fn stays_at(&self, to: Destination) -> bool {
        to.collection_id == self.collection_id
            && (to.before == Some(self.request_id)
                || (to.folder_id == self.parent && to.before == self.next))
    }
}

/// Where on a row a drop lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Before,
    After,
    Into,
}

/// Where dropping on `node` at `place` puts a request: before or after a
/// request, or at the end of a folder or collection. `None` if `node` doesn't
/// take a drop there.
fn destination(node: &Node, place: Place) -> Option<Destination> {
    let (folder_id, before) = match (node.kind, place) {
        (NodeKind::Request(_), Place::Before) => (node.parent, Some(node.id)),
        (NodeKind::Request(_), Place::After) => (node.parent, node.next),
        (NodeKind::Folder { .. }, Place::Into) => (Some(node.id), None),
        (NodeKind::Collection { .. }, Place::Into) => (None, None),
        _ => return None,
    };
    Some(Destination {
        collection_id: node.collection_id,
        folder_id,
        before,
    })
}

/// The chip that follows the pointer while a request is dragged.
struct DragPreview {
    request: DraggedRequest,
    /// Where the row was grabbed, so the chip sits just below-right of the pointer.
    grab: Point<Pixels>,
}

impl DragPreview {
    fn new(request: DraggedRequest, grab: Point<Pixels>) -> Self {
        Self { request, grab }
    }
}

/// How far below and right of the pointer the chip sits.
const POINTER_GAP: f32 = 8.;

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .pl(self.grab.x + px(POINTER_GAP))
            .pt(self.grab.y + px(POINTER_GAP))
            .child(
                h_flex()
                    .max_w(px(240.))
                    .gap_1()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .shadow_md()
                    .child(method_label(self.request.method, cx))
                    .child(div().min_w_0().truncate().child(self.request.name.clone())),
            )
    }
}

/// Makes `row` a drag source if it's a request, and a drop target: before or
/// after a request by which half the pointer is in, into a folder or collection.
/// Before/after lines start at `inset`.
pub(super) fn drag_and_drop(
    row: ListItem,
    node: &Node,
    inset: Pixels,
    panel: &WeakEntity<LibraryPanel>,
) -> ListItem {
    let Some(dragged) = DraggedRequest::of(node) else {
        let Some(to) = destination(node, Place::Into) else {
            return row;
        };
        let panel = panel.clone();
        return row
            .drag_over::<DraggedRequest>(move |style, dragged, _, cx| {
                if dragged.stays_at(to) {
                    style
                } else {
                    style.bg(cx.theme().drop_target)
                }
            })
            .on_drop(move |dragged: &DraggedRequest, _, cx| emit_move(&panel, dragged, to, cx));
    };
    let zones: Vec<_> = [Place::Before, Place::After]
        .into_iter()
        .filter_map(|place| Some((place, destination(node, place)?)))
        .collect();
    let panel = panel.clone();
    row.on_drag(dragged, |dragged, grab, _, cx| {
        let preview = DragPreview::new(dragged.clone(), grab);
        cx.new(|_| preview)
    })
    .suffix(move |_, _| {
        div().absolute().inset_0().children(
            zones
                .iter()
                .map(|&(place, to)| drop_zone(place, to, inset, &panel)),
        )
    })
}

/// The half of a request row that drops `place` it, with a line along its edge
/// while a drop there would move the request.
fn drop_zone(
    place: Place,
    to: Destination,
    inset: Pixels,
    panel: &WeakEntity<LibraryPanel>,
) -> impl IntoElement {
    let panel = panel.clone();
    let zone = div().absolute().left(inset).right_0().h_1_2();
    let zone = if place == Place::Before {
        zone.top_0()
    } else {
        zone.bottom_0()
    };
    zone.drag_over::<DraggedRequest>(move |style, dragged, _, cx| {
        if dragged.stays_at(to) {
            return style;
        }
        let style = if place == Place::Before {
            style.border_t_2()
        } else {
            style.border_b_2()
        };
        style.border_color(cx.theme().drag_border)
    })
    .on_drop(move |dragged: &DraggedRequest, _, cx| emit_move(&panel, dragged, to, cx))
}

fn emit_move(
    panel: &WeakEntity<LibraryPanel>,
    dragged: &DraggedRequest,
    to: Destination,
    cx: &mut App,
) {
    if dragged.stays_at(to) {
        return;
    }
    panel
        .update(cx, |panel, cx| panel.move_request(dragged, to, cx))
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: NodeKind, parent: Option<Uuid>, next: Option<Uuid>) -> Node {
        Node {
            id: Uuid::new_v4(),
            collection_id: Uuid::new_v4(),
            name: "Name".into(),
            depth: 1,
            kind,
            parent,
            next,
        }
    }

    fn request(parent: Option<Uuid>, next: Option<Uuid>) -> Node {
        node(NodeKind::Request(HttpMethod::Get), parent, next)
    }

    #[test]
    fn request_rows_take_drops_before_and_after() {
        let (folder, next) = (Uuid::new_v4(), Uuid::new_v4());
        let row = request(Some(folder), Some(next));
        let at = |before| Destination {
            collection_id: row.collection_id,
            folder_id: Some(folder),
            before,
        };
        assert_eq!(destination(&row, Place::Before), Some(at(Some(row.id))));
        assert_eq!(destination(&row, Place::After), Some(at(Some(next))));
        assert_eq!(destination(&row, Place::Into), None);

        let last = request(None, None);
        assert_eq!(
            destination(&last, Place::After),
            Some(Destination {
                collection_id: last.collection_id,
                folder_id: None,
                before: None,
            })
        );
    }

    #[test]
    fn folder_and_collection_rows_take_drops_into_them_at_the_end() {
        let folder = node(NodeKind::Folder { expanded: false }, None, None);
        assert_eq!(
            destination(&folder, Place::Into),
            Some(Destination {
                collection_id: folder.collection_id,
                folder_id: Some(folder.id),
                before: None,
            })
        );
        assert_eq!(destination(&folder, Place::Before), None);

        let collection = node(NodeKind::Collection { expanded: true }, None, None);
        assert_eq!(
            destination(&collection, Place::Into),
            Some(Destination {
                collection_id: collection.collection_id,
                folder_id: None,
                before: None,
            })
        );
        assert_eq!(destination(&collection, Place::After), None);
    }

    #[test]
    fn only_request_rows_are_dragged() {
        let row = request(None, None);
        let dragged = DraggedRequest::of(&row).unwrap();
        assert_eq!(
            (dragged.collection_id, dragged.request_id),
            (row.collection_id, row.id)
        );
        let folder = node(NodeKind::Folder { expanded: true }, None, None);
        assert_eq!(DraggedRequest::of(&folder), None);
    }

    #[test]
    fn drops_next_to_itself_or_at_its_own_place_stay() {
        let (folder, next) = (Uuid::new_v4(), Uuid::new_v4());
        let row = request(Some(folder), Some(next));
        let dragged = DraggedRequest::of(&row).unwrap();
        let at = |folder_id, before| Destination {
            collection_id: row.collection_id,
            folder_id,
            before,
        };
        assert!(dragged.stays_at(at(Some(folder), Some(row.id))));
        assert!(dragged.stays_at(at(Some(folder), Some(next))));
        assert!(!dragged.stays_at(at(Some(folder), None)));
        assert!(!dragged.stays_at(at(None, Some(next))));
        assert!(!dragged.stays_at(Destination {
            collection_id: Uuid::new_v4(),
            ..at(Some(folder), Some(next))
        }));

        let last = DraggedRequest::of(&request(Some(folder), None)).unwrap();
        assert!(last.stays_at(Destination {
            collection_id: last.collection_id,
            folder_id: Some(folder),
            before: None,
        }));
    }
}
