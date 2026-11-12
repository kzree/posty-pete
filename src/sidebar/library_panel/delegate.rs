use std::{collections::HashSet, sync::Arc};

use domain::{collection::Collection, environment::Environment};
use gpui::{
    App, Context, ElementId, ParentElement, Pixels, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, h_flex,
    list::{ListDelegate, ListItem, ListState},
};
use uuid::Uuid;

use super::{
    menu::MenuTarget,
    navigation::{Direction, step},
    tree::{Node, NodeKind, Row, Section, Sections, container_ids, flatten},
};
use crate::ui::method_label::method_label;

#[derive(Default)]
pub(super) struct LibraryDelegate {
    collections: Vec<Arc<Collection>>,
    /// Shared environments.
    environments: Vec<Arc<Environment>>,
    pub(super) sections: Sections,
    /// Expanded collections and folders.
    expanded: HashSet<Uuid>,
    pub(super) selected: Option<Uuid>,
    rows: Vec<Row>,
}

impl LibraryDelegate {
    pub(super) fn set_collections(&mut self, collections: Vec<Arc<Collection>>) {
        let existing = container_ids(&collections);
        self.expanded.retain(|id| existing.contains(id));
        self.collections = collections;
        self.rebuild_rows();
    }

    pub(super) fn set_environments(&mut self, environments: Vec<Arc<Environment>>) {
        self.environments = environments;
        self.rebuild_rows();
    }

    pub(super) fn toggle_at(&mut self, row: usize) {
        match self.rows.get(row) {
            Some(Row::Section { section, .. }) => self.sections.toggle(*section),
            Some(Row::Node(Node {
                id,
                kind: NodeKind::Collection { .. } | NodeKind::Folder { .. },
                ..
            })) => {
                if !self.expanded.remove(id) {
                    self.expanded.insert(*id);
                }
            }
            Some(Row::Node(_) | Row::Empty(_) | Row::Environment { .. }) | None => return,
        }
        self.rebuild_rows();
    }

    pub(super) fn expand(&mut self, id: Uuid) {
        self.sections.expand(Section::Collections);
        self.expanded.insert(id);
        self.rebuild_rows();
    }

    pub(super) fn expand_section(&mut self, section: Section) {
        self.sections.expand(section);
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
            Row::Section { .. } | Row::Empty(_) | Row::Environment { .. } => None,
        }
    }

    /// The id of a shared environment row.
    pub(super) fn environment_at(&self, row: usize) -> Option<Uuid> {
        match self.rows.get(row)? {
            Row::Environment { id, .. } => Some(*id),
            _ => None,
        }
    }

    pub(super) fn environment_name(&self, id: Uuid) -> Option<String> {
        self.environments
            .iter()
            .find(|env| env.id == id)
            .map(|env| env.name.clone())
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

    /// The visible row showing the collection, folder, request or shared
    /// environment `id`.
    pub(super) fn row_of(&self, id: Uuid) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            Row::Node(node) => node.id == id,
            Row::Environment { id: env_id, .. } => *env_id == id,
            Row::Section { .. } | Row::Empty(_) => false,
        })
    }

    /// The row the arrow keys move to from `current`.
    pub(super) fn step(&self, current: Option<usize>, direction: Direction) -> Option<usize> {
        step(self.rows.len(), current, direction, |row| {
            self.rows[row].is_selectable()
        })
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
        self.rows = flatten(
            &self.collections,
            &self.environments,
            self.sections,
            &self.expanded,
        );
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
            Row::Section { section, expanded } => render_section(*section, *expanded, cx),
            Row::Empty(section) => render_empty_section(*section, cx),
            Row::Node(node) => render_node(node, cx),
            Row::Environment { id, name } => render_environment(*id, name.clone(), cx),
        })
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

/// Every row's height, so the list's single measured height fits any row.
const ROW_HEIGHT: f32 = 24.;

fn row_item(id: impl Into<ElementId>) -> ListItem {
    ListItem::new(id).h(px(ROW_HEIGHT)).text_xs()
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

fn section_label(section: Section) -> &'static str {
    match section {
        Section::Collections => "Collections",
        Section::Environments => "Environments",
    }
}

fn render_section(section: Section, expanded: bool, cx: &App) -> ListItem {
    let label = section_label(section);
    let id = match section {
        Section::Collections => "library-collections",
        Section::Environments => "library-environments",
    };
    row_item(id)
        .accessibility_label(format!("{label}, {}", expansion_state(expanded)))
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(chevron(expanded))
                .child(label),
        )
}

/// Indent of a top-level row's name: the row inset plus the chevron slot.
fn name_inset() -> Pixels {
    px(INDENT * 2.)
}

fn render_empty_section(section: Section, cx: &App) -> ListItem {
    let (id, text) = match section {
        Section::Collections => ("library-no-collections", "No collections yet"),
        Section::Environments => ("library-no-environments", "No environments yet"),
    };
    row_item(id).disabled(true).accessibility_label(text).child(
        div()
            .pl(name_inset())
            .truncate()
            .text_color(cx.theme().muted_foreground)
            .child(text),
    )
}

fn render_environment(id: Uuid, name: SharedString, cx: &App) -> ListItem {
    row_item(id)
        .accessibility_label(format!("{name}, environment"))
        .child(
            div()
                .pl(name_inset())
                .min_w_0()
                .truncate()
                .text_color(cx.theme().foreground)
                .child(name),
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

    row_item(node.id)
        .accessibility_label(label)
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
    use crate::sidebar::library_panel::test_support::{environments, fixture, outline};

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
        assert_eq!(
            outline(&delegate.rows),
            ["Collections", "Environments", "(none)"]
        );
        delegate.toggle_at(0);
        assert_eq!(outline(&delegate.rows).len(), 8);

        delegate.set_collections(collections);
        assert_eq!(
            outline(&delegate.rows)[..6],
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
            outline(&delegate.rows)[..5],
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

    #[test]
    fn environment_rows_open_select_and_toggle_their_section() {
        let (collections, _) = fixture();
        let environments = environments();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        delegate.set_environments(environments.clone());
        let staging = environments[1].id;

        assert_eq!(delegate.environment_at(5), Some(staging));
        assert_eq!(delegate.environment_at(3), None);
        assert_eq!(delegate.row_of(staging), Some(5));
        assert_eq!(delegate.menu_target_at(5), None);
        assert_eq!(delegate.environment_name(staging), Some("Staging".into()));

        delegate.toggle_at(5);
        assert_eq!(delegate.rows.len(), 6);
        delegate.toggle_at(3);
        assert_eq!(delegate.row_of(staging), None);
        assert_eq!(delegate.environment_at(5), None);

        delegate.expand_section(Section::Environments);
        assert_eq!(delegate.row_of(staging), Some(5));
    }

    #[test]
    fn arrow_keys_skip_empty_rows() {
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(Vec::new());
        assert_eq!(delegate.step(Some(0), Direction::Down), Some(2));
        assert_eq!(delegate.step(Some(2), Direction::Down), Some(0));
        assert_eq!(delegate.step(Some(0), Direction::Up), Some(2));
        assert_eq!(delegate.step(None, Direction::Up), Some(2));

        delegate.set_environments(environments());
        assert_eq!(delegate.step(Some(2), Direction::Down), Some(3));
        assert_eq!(delegate.step(Some(4), Direction::Down), Some(0));
    }

    #[test]
    fn empty_rows_do_nothing() {
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(Vec::new());
        assert_eq!(
            outline(&delegate.rows),
            ["Collections", "(none)", "Environments", "(none)"]
        );
        delegate.toggle_at(1);
        assert_eq!(delegate.menu_target_at(1), None);
        assert_eq!(delegate.environment_at(1), None);
        assert_eq!(delegate.rows.len(), 4);
    }
}
