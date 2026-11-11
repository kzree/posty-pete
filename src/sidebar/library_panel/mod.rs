mod delegate;
mod menu;
mod tree;

use std::{collections::HashMap, sync::Arc};

use gpui::{
    Anchor, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollStrategy, Styled, Subscription, Window, div,
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
use crate::domain::{collection::Collection, environment::Environment};
use delegate::LibraryDelegate;
use menu::context_menu;
use tree::Section;

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
    CreateSharedEnvironment,
    OpenSharedEnvironment(Uuid),
    RenameSharedEnvironment {
        id: Uuid,
        name: String,
    },
    DuplicateSharedEnvironment(Uuid),
    DeleteSharedEnvironment(Uuid),
}

pub struct LibraryPanel {
    list: Entity<ListState<LibraryDelegate>>,
    /// The open request, while it has unsaved changes.
    dirty_request: Option<Uuid>,
    /// How many environments are based on each shared environment.
    environment_links: HashMap<Uuid, usize>,
    /// The row `select_row` scrolls to and focuses once the panel has rendered.
    pending_reveal: Option<Uuid>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LibraryPanelEvent> for LibraryPanel {}

impl LibraryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = LibraryDelegate::default();
        let list = cx.new(|cx| ListState::new(delegate, window, cx));
        let list_sub = cx.subscribe_in(&list, window, |this, _, event, window, cx| {
            if let ListEvent::Confirm(ix) = event {
                let delegate = this.list.read(cx).delegate();
                let (collection, request, environment) = (
                    delegate.collection_at(ix.row),
                    delegate.request_at(ix.row),
                    delegate.environment_at(ix.row),
                );
                this.update_rows(cx, |delegate| delegate.toggle_at(ix.row));
                // The selection marks what the main pane shows, not the clicked row.
                if request.is_none() && environment.is_none() {
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
                if let Some(id) = environment {
                    cx.emit(LibraryPanelEvent::OpenSharedEnvironment(id));
                }
            }
        });
        Self {
            list,
            dirty_request: None,
            environment_links: HashMap::new(),
            pending_reveal: None,
            _subscriptions: vec![list_sub],
        }
    }

    pub fn set_collections(
        &mut self,
        collections: Vec<Arc<Collection>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows(cx, |delegate| delegate.set_collections(collections));
        self.list.update(cx, |list, cx| {
            list.set_right_clicked_index(None, window, cx)
        });
        self.sync_selection(window, cx);
    }

    /// Shared environments in page order; `links` counts the environments
    /// based on each.
    pub fn set_shared_environments(
        &mut self,
        environments: Vec<Arc<Environment>>,
        links: HashMap<Uuid, usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.environment_links = links;
        self.update_rows(cx, |delegate| delegate.set_environments(environments));
        self.list.update(cx, |list, cx| {
            list.set_right_clicked_index(None, window, cx)
        });
        self.sync_selection(window, cx);
    }

    /// The collection, request or shared environment shown in the main pane; its row stays selected across refreshes.
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

    /// Expands `expand`, then moves the list's keyboard selection to `id`'s
    /// row; scrolling to it and focusing the list wait until the panel has
    /// rendered, since it may have been hidden. The main pane's row is
    /// selected again on the next refresh.
    pub fn select_row(
        &mut self,
        id: Uuid,
        expand: impl IntoIterator<Item = Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reveal(expand, cx);
        self.list.update(cx, |list, cx| {
            let Some(row) = list.delegate().row_of(id) else {
                return;
            };
            let ix = IndexPath::new(row);
            list.set_selected_index(Some(ix), window, cx);
        });
        self.pending_reveal = Some(id);
        cx.notify();
    }

    fn finish_reveal(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            if let Some(row) = list.delegate().row_of(id) {
                list.scroll_to_item(IndexPath::new(row), ScrollStrategy::Center, window, cx);
            }
            list.focus(window, cx);
        });
    }

    /// Only read when a context menu opens, so nothing needs repainting.
    pub fn set_dirty_request(&mut self, id: Option<Uuid>) {
        self.dirty_request = id;
    }

    fn create_shared_environment(&mut self, cx: &mut Context<Self>) {
        self.update_rows(cx, |delegate| {
            delegate.expand_section(Section::Environments)
        });
        cx.emit(LibraryPanelEvent::CreateSharedEnvironment);
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
        let (panel, environment_panel) = (cx.weak_entity(), cx.weak_entity());
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
                                        delegate.expand_section(Section::Collections)
                                    });
                                    cx.emit(LibraryPanelEvent::CreateCollection)
                                })
                                .ok();
                        }),
                )
                .item(
                    PopupMenuItem::new("Environment")
                        .icon(Icon::new(LucideIcon::LayersPlus))
                        .on_click({
                            let panel = environment_panel.clone();
                            move |_, _, cx| {
                                panel
                                    .update(cx, |panel, cx| panel.create_shared_environment(cx))
                                    .ok();
                            }
                        }),
                )
            });

        panel_header("Library", new_button, cx)
    }
}

impl Render for LibraryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(id) = self.pending_reveal.take() {
            cx.on_next_frame(window, move |this, window, cx| {
                this.finish_reveal(id, window, cx)
            });
        }
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
    use std::sync::Arc;

    use uuid::Uuid;

    use super::tree::{Row, Section};
    use crate::domain::{
        collection::{Collection, Folder, Item, SavedRequest},
        environment::Environment,
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
    pub(super) fn fixture() -> (Vec<Arc<Collection>>, Ids) {
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
        (
            vec![Arc::new(collection), Arc::new(Collection::new("Empty"))],
            ids,
        )
    }

    /// Shared environments in the order the page lists them.
    pub(super) fn environments() -> Vec<Arc<Environment>> {
        ["Local", "Staging"]
            .into_iter()
            .map(|name| Arc::new(Environment::new(name)))
            .collect()
    }

    /// Each row as "<depth>:<name>" or "env:<name>", sections by their
    /// label and empty rows as "(none)".
    pub(super) fn outline(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Section {
                    section: Section::Collections,
                    ..
                } => "Collections".to_string(),
                Row::Section {
                    section: Section::Environments,
                    ..
                } => "Environments".to_string(),
                Row::Empty(_) => "(none)".to_string(),
                Row::Node(node) => format!("{}:{}", node.depth, node.name),
                Row::Environment { name, .. } => format!("env:{name}"),
            })
            .collect()
    }
}
