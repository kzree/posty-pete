use std::collections::HashSet;

use gpui::{
    Anchor, App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;
use uuid::Uuid;

use super::{panel_header, placeholder};
use crate::{
    domain::{
        collection::{Collection, Item},
        request::HttpMethod,
    },
    ui::method_label::method_label,
};

pub enum LibraryPanelEvent {
    CreateCollection,
    OpenCollection(Uuid),
}

pub struct LibraryPanel {
    list: Entity<ListState<LibraryDelegate>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LibraryPanelEvent> for LibraryPanel {}

impl LibraryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| {
            let mut list = ListState::new(LibraryDelegate::default(), window, cx);
            // Every row gets the measured row's height, so measure a tree row
            // rather than the shorter section header. Falls back to row 0.
            list.set_item_to_measure_index(IndexPath::new(1), window, cx);
            list
        });
        let list_sub = cx.subscribe(&list, |this, _, event, cx| {
            if let ListEvent::Confirm(ix) = event {
                let collection = this.list.read(cx).delegate().collection_at(ix.row);
                // Rows above a toggled row don't move, so its row stays selected.
                this.update_rows(cx, |delegate| delegate.toggle_at(ix.row));
                if let Some(id) = collection {
                    cx.emit(LibraryPanelEvent::OpenCollection(id));
                }
            }
        });
        Self {
            list,
            _subscriptions: vec![list_sub],
        }
    }

    pub fn set_collections(
        &mut self,
        collections: Vec<Collection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows(cx, |delegate| delegate.set_collections(collections));
        self.list
            .update(cx, |list, cx| list.set_selected_index(None, window, cx));
    }

    fn update_rows(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut LibraryDelegate)) {
        self.list.update(cx, |list, cx| {
            f(list.delegate_mut());
            cx.notify();
        });
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.weak_entity();
        let new_button = Button::new("library-new")
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .tooltip("New")
            .accessibility_label("New")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let panel = panel.clone();
                menu.item(
                    PopupMenuItem::new("Collection")
                        .icon(Icon::new(LucideIcon::FolderPlus))
                        .on_click(move |_, _, cx| {
                            panel
                                .update(cx, |_, cx| cx.emit(LibraryPanelEvent::CreateCollection))
                                .ok();
                        }),
                )
            });

        panel_header("Library", new_button, cx)
    }
}

impl Render for LibraryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(self.render_header(cx)).child(
            div()
                .flex_1()
                .min_h_0()
                .child(List::new(&self.list).size_full()),
        )
    }
}

struct LibraryDelegate {
    collections: Vec<Collection>,
    section_expanded: bool,
    /// Expanded collections and folders.
    expanded: HashSet<Uuid>,
    rows: Vec<Row>,
}

impl Default for LibraryDelegate {
    fn default() -> Self {
        Self {
            collections: Vec::new(),
            section_expanded: true,
            expanded: HashSet::new(),
            rows: Vec::new(),
        }
    }
}

impl LibraryDelegate {
    fn set_collections(&mut self, collections: Vec<Collection>) {
        let existing = container_ids(&collections);
        self.expanded.retain(|id| existing.contains(id));
        self.collections = collections;
        self.rebuild_rows();
    }

    fn toggle_at(&mut self, row: usize) {
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

    fn collection_at(&self, row: usize) -> Option<Uuid> {
        match self.rows.get(row)? {
            Row::Node(Node {
                id,
                kind: NodeKind::Collection { .. },
                ..
            }) => Some(*id),
            _ => None,
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
                .child(div().flex_1().min_w_0().truncate().child(name)),
        )
}

#[derive(Clone, Debug, PartialEq)]
enum Row {
    /// The collapsible "Collections" header.
    Section {
        expanded: bool,
    },
    Node(Node),
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    id: Uuid,
    name: SharedString,
    /// 0 for collections.
    depth: usize,
    kind: NodeKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NodeKind {
    Collection { expanded: bool },
    Folder { expanded: bool },
    Request(HttpMethod),
}

/// No rows at all when there are no collections, so the empty state shows.
fn flatten(
    collections: &[Collection],
    section_expanded: bool,
    expanded: &HashSet<Uuid>,
) -> Vec<Row> {
    if collections.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![Row::Section {
        expanded: section_expanded,
    }];
    if !section_expanded {
        return rows;
    }
    for collection in collections {
        let is_expanded = expanded.contains(&collection.id);
        rows.push(Row::Node(Node {
            id: collection.id,
            name: collection.name.clone().into(),
            depth: 0,
            kind: NodeKind::Collection {
                expanded: is_expanded,
            },
        }));
        if is_expanded {
            flatten_items(&collection.items, 1, expanded, &mut rows);
        }
    }
    rows
}

fn flatten_items(items: &[Item], depth: usize, expanded: &HashSet<Uuid>, rows: &mut Vec<Row>) {
    for item in items {
        match item {
            Item::Folder(folder) => {
                let is_expanded = expanded.contains(&folder.id);
                rows.push(Row::Node(Node {
                    id: folder.id,
                    name: folder.name.clone().into(),
                    depth,
                    kind: NodeKind::Folder {
                        expanded: is_expanded,
                    },
                }));
                if is_expanded {
                    flatten_items(&folder.items, depth + 1, expanded, rows);
                }
            }
            Item::Request(saved) => rows.push(Row::Node(Node {
                id: saved.id,
                name: saved.name.clone().into(),
                depth,
                kind: NodeKind::Request(saved.request.method),
            })),
        }
    }
}

fn container_ids(collections: &[Collection]) -> HashSet<Uuid> {
    fn walk(items: &[Item], ids: &mut HashSet<Uuid>) {
        for item in items {
            if let Item::Folder(folder) = item {
                ids.insert(folder.id);
                walk(&folder.items, ids);
            }
        }
    }
    let mut ids = HashSet::new();
    for collection in collections {
        ids.insert(collection.id);
        walk(&collection.items, &mut ids);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        collection::{Folder, SavedRequest},
        request::{Auth, Body, Request},
    };

    fn request(method: HttpMethod) -> Request {
        Request {
            method,
            url: "https://example.com".into(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::None,
        }
    }

    struct Ids {
        collection: Uuid,
        outer: Uuid,
        inner: Uuid,
    }

    /// API: Root request, Outer > Inner > Deep request.
    fn fixture() -> (Vec<Collection>, Ids) {
        let mut collection = Collection::new("API");
        let outer = Folder::new("Outer");
        let inner = Folder::new("Inner");
        let ids = Ids {
            collection: collection.id,
            outer: outer.id,
            inner: inner.id,
        };
        collection.add_item(
            None,
            Item::Request(SavedRequest::new("Root", request(HttpMethod::Get))),
        );
        collection.add_item(None, Item::Folder(outer));
        collection.add_item(Some(ids.outer), Item::Folder(inner));
        collection.add_item(
            Some(ids.inner),
            Item::Request(SavedRequest::new("Deep", request(HttpMethod::Post))),
        );
        (vec![collection, Collection::new("Empty")], ids)
    }

    /// Each row as "<depth>:<name>", with "Collections" for the section header.
    fn outline(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Section { .. } => "Collections".to_string(),
                Row::Node(node) => format!("{}:{}", node.depth, node.name),
            })
            .collect()
    }

    #[test]
    fn no_collections_means_no_rows() {
        assert!(flatten(&[], true, &HashSet::new()).is_empty());
    }

    #[test]
    fn nothing_expanded_shows_only_collections() {
        let (collections, _) = fixture();
        let rows = flatten(&collections, true, &HashSet::new());
        assert_eq!(outline(&rows), ["Collections", "0:API", "0:Empty"]);
    }

    #[test]
    fn collapsed_section_shows_only_its_header() {
        let (collections, ids) = fixture();
        let rows = flatten(&collections, false, &HashSet::from([ids.collection]));
        assert_eq!(outline(&rows), ["Collections"]);
    }

    #[test]
    fn nested_expansion_indents_by_depth() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.outer, ids.inner]);
        let rows = flatten(&collections, true, &expanded);
        assert_eq!(
            outline(&rows),
            [
                "Collections",
                "0:API",
                "1:Root",
                "1:Outer",
                "2:Inner",
                "3:Deep",
                "0:Empty"
            ]
        );
        assert!(matches!(
            &rows[5],
            Row::Node(Node {
                kind: NodeKind::Request(HttpMethod::Post),
                ..
            })
        ));
    }

    #[test]
    fn expanded_folder_inside_collapsed_parent_stays_hidden() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.inner]);
        let rows = flatten(&collections, true, &expanded);
        assert_eq!(
            outline(&rows),
            ["Collections", "0:API", "1:Root", "1:Outer", "0:Empty"]
        );
    }

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
    fn refresh_prunes_removed_ids() {
        let (mut collections, ids) = fixture();
        let mut delegate = LibraryDelegate {
            expanded: HashSet::from([ids.collection, ids.outer, ids.inner]),
            ..Default::default()
        };
        collections[0].remove(ids.outer);
        delegate.set_collections(collections);
        assert_eq!(delegate.expanded, HashSet::from([ids.collection]));
    }
}
