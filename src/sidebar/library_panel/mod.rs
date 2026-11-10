use std::collections::HashSet;

use gpui::{
    Anchor, App, AppContext, Context, DismissEvent, Entity, EventEmitter, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Subscription, WeakEntity, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
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
    ui::{
        confirm_dialog::open_confirm_dialog, method_label::method_label,
        rename_dialog::open_rename_dialog,
    },
};

pub enum LibraryPanelEvent {
    CreateCollection,
    OpenCollection(Uuid),
    OpenRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
    /// `item_id` equals `collection_id` when renaming the collection itself.
    Rename {
        collection_id: Uuid,
        item_id: Uuid,
        name: String,
    },
    DeleteCollection(Uuid),
    DeleteItem {
        collection_id: Uuid,
        item_id: Uuid,
    },
    /// `parent` is `None` for the collection root.
    AddFolder {
        collection_id: Uuid,
        parent: Option<Uuid>,
        name: String,
    },
    /// `parent` is `None` for the collection root.
    AddRequest {
        collection_id: Uuid,
        parent: Option<Uuid>,
    },
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
        let list_sub = cx.subscribe_in(&list, window, |this, _, event, window, cx| {
            if let ListEvent::Confirm(ix) = event {
                let delegate = this.list.read(cx).delegate();
                let (collection, request) =
                    (delegate.collection_at(ix.row), delegate.request_at(ix.row));
                this.update_rows(cx, |delegate| delegate.toggle_at(ix.row));
                // The selection marks what the main pane shows, not the clicked row.
                if request.is_none() {
                    this.sync_selection(window, cx);
                }
                if let Some(id) = collection {
                    cx.emit(LibraryPanelEvent::OpenCollection(id));
                }
                if let Some((collection_id, request_id)) = request {
                    cx.emit(LibraryPanelEvent::OpenRequest {
                        collection_id,
                        request_id,
                    });
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
        self.list.update(cx, |list, cx| {
            list.set_right_clicked_index(None, window, cx)
        });
        self.sync_selection(window, cx);
    }

    /// The collection or request shown in the main pane; its row stays selected across refreshes.
    pub fn set_selected(&mut self, id: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        self.list
            .update(cx, |list, _| list.delegate_mut().selected = id);
        self.sync_selection(window, cx);
    }

    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let delegate = list.delegate();
            let row = delegate.selected.and_then(|id| delegate.row_of(id));
            list.set_selected_index(row.map(IndexPath::new), window, cx);
            cx.notify();
        });
    }

    fn rename(&mut self, target: MenuTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.list.read(cx).delegate().target_name(target) else {
            return;
        };
        let title = match target {
            MenuTarget::Collection(_) => "Rename collection",
            MenuTarget::Folder { .. } => "Rename folder",
            MenuTarget::Request { .. } => "Rename request",
        };
        let panel = cx.weak_entity();
        open_rename_dialog(
            title,
            name,
            "Save",
            move |name, _, cx| {
                panel
                    .update(cx, |_, cx| {
                        cx.emit(LibraryPanelEvent::Rename {
                            collection_id: target.collection_id(),
                            item_id: target.id(),
                            name,
                        })
                    })
                    .ok();
            },
            window,
            cx,
        );
    }

    fn delete(&mut self, target: MenuTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.list.read(cx).delegate().target_name(target) else {
            return;
        };
        let (title, message) = match target {
            MenuTarget::Collection(_) => (
                "Delete collection?",
                format!(
                    "“{name}” and all of its folders and requests will be deleted. This can't be undone."
                ),
            ),
            MenuTarget::Folder { .. } => (
                "Delete folder?",
                format!("“{name}” and everything in it will be deleted. This can't be undone."),
            ),
            MenuTarget::Request { .. } => (
                "Delete request?",
                format!("“{name}” will be deleted. This can't be undone."),
            ),
        };
        let panel = cx.weak_entity();
        open_confirm_dialog(
            title,
            message,
            "Delete",
            move |_, cx| {
                let event = match target {
                    MenuTarget::Collection(id) => LibraryPanelEvent::DeleteCollection(id),
                    MenuTarget::Folder { .. } | MenuTarget::Request { .. } => {
                        LibraryPanelEvent::DeleteItem {
                            collection_id: target.collection_id(),
                            item_id: target.id(),
                        }
                    }
                };
                panel.update(cx, |_, cx| cx.emit(event)).ok();
            },
            window,
            cx,
        );
    }

    fn add_folder(&mut self, parent: Container, window: &mut Window, cx: &mut Context<Self>) {
        let panel = cx.weak_entity();
        open_rename_dialog(
            "New folder",
            "New Folder",
            "Create",
            move |name, _, cx| {
                panel
                    .update(cx, |this, cx| {
                        this.update_rows(cx, |delegate| delegate.expand(parent.id()));
                        cx.emit(LibraryPanelEvent::AddFolder {
                            collection_id: parent.collection_id,
                            parent: parent.folder_id,
                            name,
                        })
                    })
                    .ok();
            },
            window,
            cx,
        );
    }

    fn add_request(&mut self, parent: Container, cx: &mut Context<Self>) {
        self.update_rows(cx, |delegate| delegate.expand(parent.id()));
        cx.emit(LibraryPanelEvent::AddRequest {
            collection_id: parent.collection_id,
            parent: parent.folder_id,
        });
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
                                .update(cx, |panel, cx| {
                                    panel.update_rows(cx, |delegate| {
                                        delegate.section_expanded = true
                                    });
                                    cx.emit(LibraryPanelEvent::CreateCollection)
                                })
                                .ok();
                        }),
                )
            });

        panel_header("Library", new_button, cx)
    }
}

impl Render for LibraryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.weak_entity();
        let list = self.list.downgrade();
        v_flex().size_full().child(self.render_header(cx)).child(
            div()
                .id("library-list")
                .flex_1()
                .min_h_0()
                .child(List::new(&self.list).size_full())
                .context_menu(move |menu, window, cx| {
                    context_menu(&panel, &list, menu, window, cx)
                }),
        )
    }
}

fn context_menu(
    panel: &WeakEntity<LibraryPanel>,
    list: &WeakEntity<ListState<LibraryDelegate>>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(list_state) = list.upgrade() else {
        return menu;
    };
    let state = list_state.read(cx);
    let Some(target) = state
        .right_clicked_index()
        .and_then(|ix| state.delegate().menu_target_at(ix.row))
    else {
        return menu;
    };

    // The list outlines the right-clicked row until this is reset.
    let menu_entity = cx.entity();
    window
        .subscribe(&menu_entity, cx, {
            let list = list.clone();
            move |_, _: &DismissEvent, window, cx| {
                list.update(cx, |list, cx| {
                    list.set_right_clicked_index(None, window, cx)
                })
                .ok();
            }
        })
        .detach();

    let mut menu = menu;
    if let Some(parent) = target.container() {
        let (add_request_panel, add_folder_panel) = (panel.clone(), panel.clone());
        menu = menu
            .item(PopupMenuItem::new("Add request").on_click(move |_, _, cx| {
                add_request_panel
                    .update(cx, |panel, cx| panel.add_request(parent, cx))
                    .ok();
            }))
            .item(
                PopupMenuItem::new("Add folder").on_click(move |_, window, cx| {
                    add_folder_panel
                        .update(cx, |panel, cx| panel.add_folder(parent, window, cx))
                        .ok();
                }),
            )
            .separator();
    }
    let (rename_panel, delete_panel) = (panel.clone(), panel.clone());
    menu.item(
        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
            rename_panel
                .update(cx, |panel, cx| panel.rename(target, window, cx))
                .ok();
        }),
    )
    .item(
        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
            delete_panel
                .update(cx, |panel, cx| panel.delete(target, window, cx))
                .ok();
        }),
    )
}

/// A collection, folder or request row that has a context menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuTarget {
    Collection(Uuid),
    Folder {
        collection_id: Uuid,
        folder_id: Uuid,
    },
    Request {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

impl MenuTarget {
    fn id(self) -> Uuid {
        match self {
            Self::Collection(id) => id,
            Self::Folder { folder_id, .. } => folder_id,
            Self::Request { request_id, .. } => request_id,
        }
    }

    fn collection_id(self) -> Uuid {
        match self {
            Self::Collection(id)
            | Self::Folder {
                collection_id: id, ..
            }
            | Self::Request {
                collection_id: id, ..
            } => id,
        }
    }

    /// `None` for requests, which can't hold anything.
    fn container(self) -> Option<Container> {
        match self {
            Self::Collection(collection_id) => Some(Container {
                collection_id,
                folder_id: None,
            }),
            Self::Folder {
                collection_id,
                folder_id,
            } => Some(Container {
                collection_id,
                folder_id: Some(folder_id),
            }),
            Self::Request { .. } => None,
        }
    }
}

/// Where new folders and requests go; `folder_id` is `None` for the collection root.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Container {
    collection_id: Uuid,
    folder_id: Option<Uuid>,
}

impl Container {
    fn id(self) -> Uuid {
        self.folder_id.unwrap_or(self.collection_id)
    }
}

struct LibraryDelegate {
    collections: Vec<Collection>,
    section_expanded: bool,
    /// Expanded collections and folders.
    expanded: HashSet<Uuid>,
    selected: Option<Uuid>,
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

    fn expand(&mut self, id: Uuid) {
        self.section_expanded = true;
        self.expanded.insert(id);
        self.rebuild_rows();
    }

    fn menu_target_at(&self, row: usize) -> Option<MenuTarget> {
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

    /// `(collection_id, request_id)` of a request row.
    fn request_at(&self, row: usize) -> Option<(Uuid, Uuid)> {
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
    fn row_of(&self, id: Uuid) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Node(node) if node.id == id))
    }

    fn target_name(&self, target: MenuTarget) -> Option<String> {
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
    /// The collection this node belongs to; equals `id` for collections.
    collection_id: Uuid,
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
            collection_id: collection.id,
            name: collection.name.clone().into(),
            depth: 0,
            kind: NodeKind::Collection {
                expanded: is_expanded,
            },
        }));
        if is_expanded {
            flatten_items(collection.id, &collection.items, 1, expanded, &mut rows);
        }
    }
    rows
}

fn flatten_items(
    collection_id: Uuid,
    items: &[Item],
    depth: usize,
    expanded: &HashSet<Uuid>,
    rows: &mut Vec<Row>,
) {
    for item in items {
        match item {
            Item::Folder(folder) => {
                let is_expanded = expanded.contains(&folder.id);
                rows.push(Row::Node(Node {
                    id: folder.id,
                    collection_id,
                    name: folder.name.clone().into(),
                    depth,
                    kind: NodeKind::Folder {
                        expanded: is_expanded,
                    },
                }));
                if is_expanded {
                    flatten_items(collection_id, &folder.items, depth + 1, expanded, rows);
                }
            }
            Item::Request(saved) => rows.push(Row::Node(Node {
                id: saved.id,
                collection_id,
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
        root: Uuid,
        deep: Uuid,
    }

    /// API: Root request, Outer > Inner > Deep request.
    fn fixture() -> (Vec<Collection>, Ids) {
        let mut collection = Collection::new("API");
        let outer = Folder::new("Outer");
        let inner = Folder::new("Inner");
        let root = SavedRequest::new("Root", request(HttpMethod::Get));
        let deep = SavedRequest::new("Deep", request(HttpMethod::Post));
        let ids = Ids {
            collection: collection.id,
            outer: outer.id,
            inner: inner.id,
            root: root.id,
            deep: deep.id,
        };
        collection.add_item(None, Item::Request(root));
        collection.add_item(None, Item::Folder(outer));
        collection.add_item(Some(ids.outer), Item::Folder(inner));
        collection.add_item(Some(ids.inner), Item::Request(deep));
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
    fn menu_targets_are_collections_folders_and_requests() {
        let (collections, ids) = fixture();
        let mut delegate = LibraryDelegate::default();
        delegate.set_collections(collections);
        delegate.toggle_at(1);
        delegate.toggle_at(3);
        assert_eq!(delegate.menu_target_at(0), None);
        let collection = delegate.menu_target_at(1).unwrap();
        assert_eq!(collection, MenuTarget::Collection(ids.collection));
        assert_eq!(
            collection.container(),
            Some(Container {
                collection_id: ids.collection,
                folder_id: None
            })
        );
        assert_eq!(collection.container().unwrap().id(), ids.collection);

        let request = delegate.menu_target_at(2).unwrap();
        assert_eq!(
            request,
            MenuTarget::Request {
                collection_id: ids.collection,
                request_id: ids.root
            }
        );
        assert_eq!(request.id(), ids.root);
        assert_eq!(request.collection_id(), ids.collection);
        assert_eq!(request.container(), None);

        let folder = delegate.menu_target_at(3).unwrap();
        assert_eq!(
            folder,
            MenuTarget::Folder {
                collection_id: ids.collection,
                folder_id: ids.outer
            }
        );
        assert_eq!(folder.id(), ids.outer);
        assert_eq!(folder.collection_id(), ids.collection);
        assert_eq!(
            folder.container(),
            Some(Container {
                collection_id: ids.collection,
                folder_id: Some(ids.outer)
            })
        );
        assert_eq!(folder.container().unwrap().id(), ids.outer);
        assert_eq!(delegate.menu_target_at(99), None);
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
        collections[0].remove(ids.outer);
        delegate.set_collections(collections);
        assert_eq!(delegate.expanded, HashSet::from([ids.collection]));
    }
}
