use std::{collections::HashSet, sync::Arc};

use gpui::{
    App, Context, IntoElement, ParentElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, h_flex,
    list::{ListDelegate, ListItem, ListState},
};
use uuid::Uuid;

use super::{
    menu::MenuTarget,
    tree::{Node, NodeKind, Row, container_ids, flatten},
};
use crate::{domain::collection::Collection, sidebar::placeholder, ui::method_label::method_label};

pub(super) struct LibraryDelegate {
    collections: Vec<Arc<Collection>>,
    pub(super) section_expanded: bool,
    /// Expanded collections and folders.
    expanded: HashSet<Uuid>,
    pub(super) selected: Option<Uuid>,
    rows: Vec<Row>,
}

impl Default for LibraryDelegate {
    fn default() -> Self {
        Self {
            collections: Vec::new(),
            section_expanded: true,
            expanded: HashSet::new(),
            selected: None,
            rows: Vec::new(),
        }
    }
}

impl LibraryDelegate {
    pub(super) fn set_collections(&mut self, collections: Vec<Arc<Collection>>) {
        let existing = container_ids(&collections);
        self.expanded.retain(|id| existing.contains(id));
        self.collections = collections;
        self.rebuild_rows();
    }

    pub(super) fn toggle_at(&mut self, row: usize) {
        match self.rows.get(row) {
            Some(Row::Section { .. }) => self.section_expanded = !self.section_expanded,
            Some(Row::Node(Node {
                id,
                kind: NodeKind::Collection { .. } | NodeKind::Folder { .. },
                ..
            })) => {
                if !self.expanded.remove(id) {
                    self.expanded.insert(*id);
                }
            }
            Some(Row::Node(_)) | None => return,
        }
        self.rebuild_rows();
    }

    pub(super) fn expand(&mut self, id: Uuid) {
        self.section_expanded = true;
        self.expanded.insert(id);
        self.rebuild_rows();
    }

    pub(super) fn menu_target_at(&self, row: usize) -> Option<MenuTarget> {
        match self.rows.get(row)? {
            Row::Node(Node {
                id,
                kind: NodeKind::Collection { .. },
                ..
            }) => Some(MenuTarget::Collection(*id)),
            Row::Node(Node {
                id,
                collection_id,
                kind: NodeKind::Folder { .. },
                ..
            }) => Some(MenuTarget::Folder {
                collection_id: *collection_id,
                folder_id: *id,
            }),
            Row::Node(Node {
                id,
                collection_id,
                kind: NodeKind::Request(_),
                ..
            }) => Some(MenuTarget::Request {
                collection_id: *collection_id,
                request_id: *id,
            }),
            Row::Section { .. } => None,
        }
    }

    pub(super) fn collection_at(&self, row: usize) -> Option<Uuid> {
        match self.rows.get(row)? {
            Row::Node(Node {
                id,
                kind: NodeKind::Collection { .. },
                ..
            }) => Some(*id),
            _ => None,
        }
    }

    /// `(collection_id, request_id)` of a request row.
    pub(super) fn request_at(&self, row: usize) -> Option<(Uuid, Uuid)> {
        match self.rows.get(row)? {
            Row::Node(Node {
                id,
                collection_id,
                kind: NodeKind::Request(_),
                ..
            }) => Some((*collection_id, *id)),
            _ => None,
        }
    }

    /// The visible row showing the collection, folder or request `id`.
    pub(super) fn row_of(&self, id: Uuid) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Node(node) if node.id == id))
    }

    pub(super) fn target_name(&self, target: MenuTarget) -> Option<String> {
        let collection = self
            .collections
            .iter()
            .find(|collection| collection.id == target.collection_id())?;
        match target {
            MenuTarget::Collection(_) => Some(collection.name.clone()),
            MenuTarget::Folder { folder_id, .. } => collection
                .find_folder(folder_id)
                .map(|folder| folder.name.clone()),
            MenuTarget::Request { request_id, .. } => collection
                .find_request(request_id)
                .map(|request| request.name.clone()),
        }
    }

    fn rebuild_rows(&mut self) {
        self.rows = flatten(&self.collections, self.section_expanded, &self.expanded);
    }
}

impl ListDelegate for LibraryDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.rows.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        Some(match self.rows.get(ix.row)? {
            Row::Section { expanded } => render_section(*expanded, cx),
            Row::Node(node) => render_node(node, cx),
        })
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        placeholder("Your library is empty.", cx)
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

/// Chevron slot width plus the gap after it, so a child's chevron sits under
/// its parent's label.
const INDENT: f32 = 16.;

fn chevron(expanded: bool) -> Icon {
    let name = if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    Icon::new(name).xsmall()
}

fn expansion_state(expanded: bool) -> &'static str {
    if expanded { "expanded" } else { "collapsed" }
}

fn render_section(expanded: bool, cx: &App) -> ListItem {
    ListItem::new("library-collections")
        .accessibility_label(format!("Collections, {}", expansion_state(expanded)))
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(chevron(expanded))
                .child("Collections"),
        )
}

fn render_node(node: &Node, cx: &App) -> ListItem {
    let name = node.name.clone();
    let (label, chevron, method) = match node.kind {
        NodeKind::Collection { expanded } => (
            format!("{name}, collection, {}", expansion_state(expanded)),
            Some(chevron(expanded)),
            None,
        ),
        NodeKind::Folder { expanded } => (
            format!("{name}, folder, {}", expansion_state(expanded)),
            Some(chevron(expanded)),
            None,
        ),
        NodeKind::Request(method) => (format!("{} {name}", method.as_str()), None, Some(method)),
    };

    ListItem::new(node.id)
        .accessibility_label(label)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_1()
                .pl(px((node.depth + 1) as f32 * INDENT))
                .child(h_flex().w_3().flex_none().children(chevron))
                .when_some(method, |row, method| row.child(method_label(method, cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(method.is_some(), |name| {
                            name.text_color(cx.theme().foreground)
                        })
                        .child(name),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::library_panel::test_support::{fixture, outline};

    #[test]
    fn toggling_keeps_nested_state_and_survives_refresh() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections.clone());

        delegate.toggle_at(1);
        delegate.toggle_at(3);
        assert_eq!(
            delegate.expanded,
            HashSet::from([ids.collection, ids.outer])
        );

        delegate.toggle_at(0);
        assert_eq!(outline(&delegate.rows), ["Collections"]);
        delegate.toggle_at(0);
        assert_eq!(outline(&delegate.rows).len(), 6);

        delegate.set_collections(collections);
        assert_eq!(
            outline(&delegate.rows),
            [
                "Collections",
                "0:API",
                "1:Root",
                "1:Outer",
                "2:Inner",
                "0:Empty"
            ]
        );
    }

    #[test]
    fn toggling_a_request_does_nothing() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        delegate.toggle_at(1);
        delegate.toggle_at(2);
        assert_eq!(delegate.expanded, HashSet::from([ids.collection]));
    }

    #[test]
    fn only_collection_rows_open_a_collection() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        delegate.toggle_at(1);
        delegate.toggle_at(3);
        assert_eq!(delegate.collection_at(0), None);
        assert_eq!(delegate.collection_at(1), Some(ids.collection));
        assert_eq!(delegate.collection_at(2), None);
        assert_eq!(delegate.collection_at(3), None);
        assert_eq!(delegate.collection_at(99), None);
    }

    #[test]
    fn only_request_rows_open_a_request() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        delegate.toggle_at(1);
        assert_eq!(delegate.request_at(0), None);
        assert_eq!(delegate.request_at(1), None);
        assert_eq!(delegate.request_at(2), Some((ids.collection, ids.root)));
        assert_eq!(delegate.request_at(3), None);
        assert_eq!(delegate.request_at(99), None);
    }

    #[test]
    fn row_of_finds_only_visible_rows() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        assert_eq!(delegate.row_of(ids.collection), Some(1));
        assert_eq!(delegate.row_of(ids.root), None);

        delegate.toggle_at(1);
        assert_eq!(delegate.row_of(ids.root), Some(2));
        assert_eq!(delegate.row_of(ids.outer), Some(3));
        assert_eq!(delegate.row_of(ids.inner), None);
        assert_eq!(delegate.row_of(Uuid::new_v4()), None);

        delegate.toggle_at(0);
        assert_eq!(delegate.row_of(ids.collection), None);
    }

    #[test]
    fn target_names_come_from_collections_and_nested_items() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        let folder = |folder_id| MenuTarget::Folder {
            collection_id: ids.collection,
            folder_id,
        };
        assert_eq!(
            delegate.target_name(MenuTarget::Collection(ids.collection)),
            Some("API".into())
        );
        assert_eq!(
            delegate.target_name(folder(ids.inner)),
            Some("Inner".into())
        );
        assert_eq!(delegate.target_name(folder(Uuid::new_v4())), None);
        let request = |request_id| MenuTarget::Request {
            collection_id: ids.collection,
            request_id,
        };
        assert_eq!(delegate.target_name(request(ids.root)), Some("Root".into()));
        assert_eq!(delegate.target_name(request(ids.deep)), Some("Deep".into()));
        assert_eq!(delegate.target_name(request(ids.inner)), None);
        assert_eq!(delegate.target_name(folder(ids.deep)), None);
        assert_eq!(
            delegate.target_name(MenuTarget::Collection(Uuid::new_v4())),
            None
        );
    }

    #[test]
    fn expand_opens_the_section_and_the_parent() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections.clone());
        delegate.toggle_at(0);
        delegate.expand(ids.collection);
        assert_eq!(
            outline(&delegate.rows),
            ["Collections", "0:API", "1:Root", "1:Outer", "0:Empty"]
        );

        delegate.expand(ids.collection);
        delegate.set_collections(collections);
        assert_eq!(delegate.expanded, HashSet::from([ids.collection]));
    }

    #[test]
    fn refresh_prunes_removed_ids() {
        let (mut collections, ids) = fixture();
        let mut delegate = LibraryDelegate {
            expanded: HashSet::from([ids.collection, ids.outer, ids.inner]),
            ..Default::default()
        };
        Arc::make_mut(&mut collections[0]).remove(ids.outer);
        delegate.set_collections(collections);
        assert_eq!(delegate.expanded, HashSet::from([ids.collection]));
    }
}
