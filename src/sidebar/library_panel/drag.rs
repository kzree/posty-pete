use std::rc::Rc;

use domain::{collection::Destination, request::HttpMethod};
use gpui::{
    AnyElement, App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
    relative,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex, list::ListItem};
use uuid::Uuid;

use super::{
    LibraryPanel,
    tree::{Node, NodeKind},
};
use crate::ui::method_label::method_label;

/// The payload of a request or folder row being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DraggedItem {
    pub(super) collection_id: Uuid,
    pub(super) id: Uuid,
    pub(super) kind: DraggedKind,
    /// Where it is now, to tell drops that wouldn't move it.
    parent: Option<Uuid>,
    next: Option<Uuid>,
    name: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DraggedKind {
    Request(HttpMethod),
    Folder,
}

impl DraggedItem {
    /// `None` unless `node` is a request or folder.
    fn of(node: &Node) -> Option<Self> {
        let kind = match node.kind {
            NodeKind::Request(method) => DraggedKind::Request(method),
            NodeKind::Folder { .. } => DraggedKind::Folder,
            NodeKind::Collection { .. } => return None,
        };
        Some(Self {
            collection_id: node.collection_id,
            id: node.id,
            kind,
            parent: node.parent(),
            next: node.next,
            name: node.name.clone(),
        })
    }

    /// Whether dropping at `to` would leave it where it is.
    fn stays_at(&self, to: Destination) -> bool {
        to.collection_id == self.collection_id
            && (to.before == Some(self.id)
                || (to.folder_id == self.parent && to.before == self.next))
    }

    /// Whether dropping on `target` moves it: not to where it is, and a
    /// folder not into itself.
    fn moves_to(&self, target: &Target) -> bool {
        let into_itself = self.kind == DraggedKind::Folder
            && target.to.collection_id == self.collection_id
            && target.folders.contains(&self.id);
        !into_itself && !self.stays_at(target.to)
    }
}

/// Where on a row a drop lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Before,
    After,
    Into,
}

/// Where a drop puts the dragged item, and the folders that place is in,
/// innermost included.
#[derive(Clone, Debug, PartialEq)]
struct Target {
    to: Destination,
    folders: Vec<Uuid>,
}

/// Where dropping on `node` at `place` puts an item: before or after a
/// request or folder, or at the end of a folder or collection. `None` if
/// `node` doesn't take a drop there.
fn target(node: &Node, place: Place) -> Option<Target> {
    let mut folders = node.folders.clone();
    let (folder_id, before) = match (node.kind, place) {
        (NodeKind::Request(_) | NodeKind::Folder { .. }, Place::Before) => {
            (node.parent(), Some(node.id))
        }
        (NodeKind::Request(_) | NodeKind::Folder { .. }, Place::After) => {
            (node.parent(), node.next)
        }
        (NodeKind::Folder { .. }, Place::Into) => {
            folders.push(node.id);
            (Some(node.id), None)
        }
        (NodeKind::Collection { .. }, Place::Into) => (None, None),
        _ => return None,
    };
    Some(Target {
        to: Destination {
            collection_id: node.collection_id,
            folder_id,
            before,
        },
        folders,
    })
}

/// The chip that follows the pointer while an item is dragged.
struct DragPreview {
    item: DraggedItem,
    /// Where the row was grabbed, so the chip sits just below-right of the pointer.
    grab: Point<Pixels>,
}

impl DragPreview {
    fn new(item: DraggedItem, grab: Point<Pixels>) -> Self {
        Self { item, grab }
    }
}

/// How far below and right of the pointer the chip sits.
const POINTER_GAP: f32 = 8.;

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let leading: AnyElement = match self.item.kind {
            DraggedKind::Request(method) => method_label(method, cx).into_any_element(),
            DraggedKind::Folder => Icon::new(IconName::Folder)
                .small()
                .text_color(theme.muted_foreground)
                .into_any_element(),
        };
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
                    .child(leading)
                    .child(div().min_w_0().truncate().child(self.item.name.clone())),
            )
    }
}

/// The places on a row, from top to bottom: before or after a request by
/// which half the pointer is in; before, into or after a folder by quarter,
/// half, quarter.
fn zones(node: &Node) -> Vec<(Place, Target)> {
    let places: &[Place] = match node.kind {
        NodeKind::Request(_) => &[Place::Before, Place::After],
        NodeKind::Folder { .. } => &[Place::Before, Place::Into, Place::After],
        NodeKind::Collection { .. } => &[],
    };
    places
        .iter()
        .filter_map(|&place| Some((place, target(node, place)?)))
        .collect()
}

/// Makes `row` a drag source if it's a request or folder, and a drop target
/// (see [`zones`]); a collection row takes drops into it anywhere.
/// Before/after lines start at `inset`.
pub(super) fn drag_and_drop(
    row: ListItem,
    node: &Node,
    inset: Pixels,
    panel: &WeakEntity<LibraryPanel>,
) -> ListItem {
    let Some(dragged) = DraggedItem::of(node) else {
        let Some(target) = target(node, Place::Into) else {
            return row;
        };
        let target = Rc::new(target);
        let panel = panel.clone();
        let over = target.clone();
        return row
            .drag_over::<DraggedItem>(move |style, dragged, _, cx| {
                if dragged.moves_to(&over) {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
            })
            .on_drop(move |dragged: &DraggedItem, _, cx| emit_move(&panel, dragged, &target, cx));
    };
    let zones: Vec<_> = zones(node)
        .into_iter()
        .map(|(place, target)| (place, Rc::new(target)))
        .collect();
    let has_into = zones.iter().any(|(place, _)| *place == Place::Into);
    let panel = panel.clone();
    let id = node.id;
    row.on_drag(dragged, |dragged, grab, _, cx| {
        let preview = DragPreview::new(dragged.clone(), grab);
        cx.new(|_| preview)
    })
    .suffix(move |_, cx| {
        div()
            .absolute()
            .inset_0()
            .children(zones.iter().map(|(place, target)| {
                let height = if has_into && *place != Place::Into {
                    relative(0.25)
                } else {
                    relative(0.5)
                };
                drop_zone(*place, target, height, inset, id, &panel, cx)
            }))
    })
}

/// The part of a row that drops `place` it, `height` tall: a line along its
/// edge for before/after, the whole row tinted for into, while a drop there
/// would move the item.
fn drop_zone(
    place: Place,
    target: &Rc<Target>,
    height: gpui::DefiniteLength,
    inset: Pixels,
    row_id: Uuid,
    panel: &WeakEntity<LibraryPanel>,
    cx: &App,
) -> AnyElement {
    let zone = div().absolute().right_0().h(height);
    let zone = match place {
        Place::Before => zone.left(inset).top_0(),
        Place::After => zone.left(inset).bottom_0(),
        Place::Into => zone.left_0().top(relative(0.25)),
    };
    let drop = {
        let (panel, target) = (panel.clone(), target.clone());
        move |dragged: &DraggedItem, _: &mut Window, cx: &mut App| {
            emit_move(&panel, dragged, &target, cx)
        }
    };
    if place == Place::Into {
        let group = SharedString::from(format!("library-drop-into-{row_id}"));
        let tint = cx.theme().drop_target;
        let target = target.clone();
        return zone
            .group(group.clone())
            .on_drop(drop)
            .child(
                // Tints the whole row while the middle half is hovered.
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(-0.5))
                    .bottom(relative(-0.5))
                    .drag_over::<DraggedItem>(|style, _, _, _| style)
                    .can_drop(move |dragged, _, _| {
                        dragged
                            .downcast_ref::<DraggedItem>()
                            .is_some_and(|dragged| dragged.moves_to(&target))
                    })
                    .group_drag_over::<DraggedItem>(group, move |style| style.bg(tint)),
            )
            .into_any_element();
    }
    let target = target.clone();
    zone.drag_over::<DraggedItem>(move |style, dragged, _, cx| {
        if !dragged.moves_to(&target) {
            return style;
        }
        let style = if place == Place::Before {
            style.border_t_2()
        } else {
            style.border_b_2()
        };
        style.border_color(cx.theme().drag_border)
    })
    .on_drop(drop)
    .into_any_element()
}

fn emit_move(
    panel: &WeakEntity<LibraryPanel>,
    dragged: &DraggedItem,
    target: &Target,
    cx: &mut App,
) {
    if !dragged.moves_to(target) {
        return;
    }
    let to = target.to;
    panel
        .update(cx, |panel, cx| panel.move_item(dragged, to, cx))
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: NodeKind, folders: Vec<Uuid>, next: Option<Uuid>) -> Node {
        Node {
            id: Uuid::new_v4(),
            collection_id: Uuid::new_v4(),
            name: "Name".into(),
            depth: folders.len() + 1,
            kind,
            folders,
            next,
        }
    }

    fn request(folders: Vec<Uuid>, next: Option<Uuid>) -> Node {
        node(NodeKind::Request(HttpMethod::Get), folders, next)
    }

    fn folder(folders: Vec<Uuid>, next: Option<Uuid>) -> Node {
        node(NodeKind::Folder { expanded: true }, folders, next)
    }

    fn at(node: &Node, folder_id: Option<Uuid>, before: Option<Uuid>) -> Destination {
        Destination {
            collection_id: node.collection_id,
            folder_id,
            before,
        }
    }

    fn places(node: &Node) -> Vec<(Place, Destination)> {
        zones(node)
            .into_iter()
            .map(|(place, target)| (place, target.to))
            .collect()
    }

    #[test]
    fn request_rows_take_drops_before_and_after() {
        let (parent, next) = (Uuid::new_v4(), Uuid::new_v4());
        let row = request(vec![parent], Some(next));
        assert_eq!(
            places(&row),
            [
                (Place::Before, at(&row, Some(parent), Some(row.id))),
                (Place::After, at(&row, Some(parent), Some(next))),
            ]
        );
        assert_eq!(target(&row, Place::Into), None);

        let last = request(vec![], None);
        assert_eq!(
            target(&last, Place::After).unwrap().to,
            at(&last, None, None)
        );
    }

    #[test]
    fn folder_rows_take_drops_before_into_and_after() {
        let (outer, next) = (Uuid::new_v4(), Uuid::new_v4());
        let row = folder(vec![outer], Some(next));
        assert_eq!(
            places(&row),
            [
                (Place::Before, at(&row, Some(outer), Some(row.id))),
                (Place::Into, at(&row, Some(row.id), None)),
                (Place::After, at(&row, Some(outer), Some(next))),
            ]
        );
        assert_eq!(target(&row, Place::Into).unwrap().folders, [outer, row.id]);
        assert_eq!(target(&row, Place::After).unwrap().folders, [outer]);
    }

    #[test]
    fn collection_rows_take_drops_into_them_at_the_end() {
        let collection = node(NodeKind::Collection { expanded: true }, vec![], None);
        assert_eq!(
            target(&collection, Place::Into).unwrap().to,
            at(&collection, None, None)
        );
        assert_eq!(target(&collection, Place::After), None);
        assert!(zones(&collection).is_empty());
    }

    #[test]
    fn request_and_folder_rows_are_dragged() {
        let row = request(vec![], None);
        let dragged = DraggedItem::of(&row).unwrap();
        assert_eq!(
            (dragged.collection_id, dragged.id, dragged.kind),
            (
                row.collection_id,
                row.id,
                DraggedKind::Request(HttpMethod::Get)
            )
        );
        let row = folder(vec![], None);
        assert_eq!(DraggedItem::of(&row).unwrap().kind, DraggedKind::Folder);
        let collection = node(NodeKind::Collection { expanded: true }, vec![], None);
        assert_eq!(DraggedItem::of(&collection), None);
    }

    #[test]
    fn drops_next_to_itself_or_at_its_own_place_stay() {
        let (parent, next) = (Uuid::new_v4(), Uuid::new_v4());
        let row = request(vec![parent], Some(next));
        let dragged = DraggedItem::of(&row).unwrap();
        assert!(dragged.stays_at(at(&row, Some(parent), Some(row.id))));
        assert!(dragged.stays_at(at(&row, Some(parent), Some(next))));
        assert!(!dragged.stays_at(at(&row, Some(parent), None)));
        assert!(!dragged.stays_at(at(&row, None, Some(next))));
        assert!(!dragged.stays_at(Destination {
            collection_id: Uuid::new_v4(),
            ..at(&row, Some(parent), Some(next))
        }));

        let last = request(vec![parent], None);
        let dragged = DraggedItem::of(&last).unwrap();
        assert!(dragged.stays_at(at(&last, Some(parent), None)));
    }

    #[test]
    fn folders_dont_move_into_themselves() {
        let outer = Uuid::new_v4();
        let dragged_row = folder(vec![outer], None);
        let collection_id = dragged_row.collection_id;
        let dragged = DraggedItem::of(&dragged_row).unwrap();
        let inside = |node: Node| Node {
            collection_id,
            ..node
        };
        let child = inside(folder(vec![outer, dragged_row.id], None));
        let grandchild = inside(request(vec![outer, dragged_row.id, child.id], None));
        let sibling = inside(folder(vec![outer], Some(dragged_row.id)));

        for row in [&dragged_row, &child, &grandchild] {
            for (place, target) in zones(row) {
                assert!(!dragged.moves_to(&target), "{place:?} {:?}", target.to);
            }
        }
        for (place, target) in zones(&sibling) {
            let stays = place == Place::After;
            assert_eq!(dragged.moves_to(&target), !stays, "{place:?}");
        }

        let request = DraggedItem::of(&grandchild).unwrap();
        let into_child = target(&child, Place::Into).unwrap();
        assert!(!dragged.moves_to(&into_child));
        assert!(DraggedItem::of(&sibling).unwrap().moves_to(&into_child));
        assert!(!request.moves_to(&into_child));
        assert!(request.moves_to(&target(&child, Place::Before).unwrap()));
    }
}
