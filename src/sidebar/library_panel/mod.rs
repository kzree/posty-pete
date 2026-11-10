mod delegate;
mod menu;
mod tree;

use gpui::{
    Anchor, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Subscription, Window, div,
};
use gpui_component::{
    Icon, IconName, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    list::{List, ListEvent, ListState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;
use uuid::Uuid;

use super::panel_header;
use crate::domain::collection::Collection;
use delegate::LibraryDelegate;
use menu::context_menu;

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
    RevertRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

pub struct LibraryPanel {
    list: Entity<ListState<LibraryDelegate>>,
    /// The open request, while it has unsaved changes.
    dirty_request: Option<Uuid>,
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
            dirty_request: None,
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

    /// Expands these collections and folders, including ones the next
    /// `set_collections` brings.
    pub fn reveal(&mut self, ids: impl IntoIterator<Item = Uuid>, cx: &mut Context<Self>) {
        self.update_rows(cx, |delegate| {
            for id in ids {
                delegate.expand(id);
            }
        });
    }

    /// Only read when a context menu opens, so nothing needs repainting.
    pub fn set_dirty_request(&mut self, id: Option<Uuid>) {
        self.dirty_request = id;
    }

    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let delegate = list.delegate();
            let row = delegate.selected.and_then(|id| delegate.row_of(id));
            list.set_selected_index(row.map(IndexPath::new), window, cx);
            cx.notify();
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

#[cfg(test)]
mod test_support {
    use uuid::Uuid;

    use super::tree::Row;
    use crate::domain::{
        collection::{Collection, Folder, Item, SavedRequest},
        request::{Auth, Body, HttpMethod, Request},
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

    pub(super) struct Ids {
        pub(super) collection: Uuid,
        pub(super) outer: Uuid,
        pub(super) inner: Uuid,
        pub(super) root: Uuid,
        pub(super) deep: Uuid,
    }

    /// API: Root request, Outer > Inner > Deep request.
    pub(super) fn fixture() -> (Vec<Collection>, Ids) {
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
    pub(super) fn outline(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Section { .. } => "Collections".to_string(),
                Row::Node(node) => format!("{}:{}", node.depth, node.name),
            })
            .collect()
    }
}
