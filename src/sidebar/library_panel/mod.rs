mod delegate;
mod drag;
mod menu;
mod move_dialog;
mod navigation;
mod tree;

use std::{collections::HashMap, sync::Arc};

use domain::{
    collection::{Collection, Destination},
    environment::Environment,
    library_expansion::LibraryExpansion,
};
use gpui::{
    Anchor, App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
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
use crate::actions::{LIBRARY_PANEL_CONTEXT, SelectNextLibraryRow, SelectPreviousLibraryRow};
use delegate::LibraryDelegate;
use drag::{DraggedItem, DraggedKind};
use menu::context_menu;
use navigation::Direction;
use tree::Section;

pub enum LibraryPanelEvent {
    CreateCollection,
    ImportOpenApi,
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
    UpdateFromSpec(Uuid),
    DeleteItem {
        collection_id: Uuid,
        item_id: Uuid,
    },
    DuplicateRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
    /// Dropped on another place in the library, maybe in another collection.
    MoveRequest {
        collection_id: Uuid,
        request_id: Uuid,
        to: Destination,
    },
    /// Like `MoveRequest`, with everything in the folder.
    MoveFolder {
        collection_id: Uuid,
        folder_id: Uuid,
        to: Destination,
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
    /// A collection, folder or section was expanded or collapsed, or expanded
    /// rows were dropped with their deleted items.
    ExpansionChanged(LibraryExpansion),
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
        let mut delegate = LibraryDelegate::default();
        delegate.panel = Some(cx.weak_entity());
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

    /// `complete` is whether `collections` is the whole library, loaded.
    /// Until it is, expanded rows missing from it are kept.
    pub fn set_collections(
        &mut self,
        collections: Vec<Arc<Collection>>,
        complete: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows(cx, |delegate| {
            delegate.set_collections(collections, complete)
        });
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

    /// Opens the stored rows along with any opened since startup.
    pub fn restore_expansion(
        &mut self,
        saved: &LibraryExpansion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows(cx, |delegate| delegate.restore(saved));
        self.sync_selection(window, cx);
    }

    pub fn expansion(&self, cx: &App) -> LibraryExpansion {
        self.list.read(cx).delegate().expansion()
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

    fn move_item(&mut self, dragged: &DraggedItem, to: Destination, cx: &mut Context<Self>) {
        let collection_id = dragged.collection_id;
        cx.emit(match dragged.kind {
            DraggedKind::Request(_) => LibraryPanelEvent::MoveRequest {
                collection_id,
                request_id: dragged.id,
                to,
            },
            DraggedKind::Folder => LibraryPanelEvent::MoveFolder {
                collection_id,
                folder_id: dragged.id,
                to,
            },
        });
    }

    fn create_shared_environment(&mut self, cx: &mut Context<Self>) {
        self.update_rows(cx, |delegate| {
            delegate.expand_section(Section::Environments)
        });
        cx.emit(LibraryPanelEvent::CreateSharedEnvironment);
    }

    fn select_step(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let current = list.selected_index().map(|ix| ix.row);
            let Some(row) = list.delegate().step(current, direction) else {
                return;
            };
            let ix = IndexPath::new(row);
            list.set_selected_index(Some(ix), window, cx);
            list.scroll_to_selected_item(window, cx);
            cx.emit(ListEvent::Select(ix));
        });
    }

    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let delegate = list.delegate();
            let row = delegate.selected.and_then(|id| delegate.row_of(id));
            list.set_selected_index(row.map(IndexPath::new), window, cx);
            cx.notify();
        });
    }

    /// Emits `ExpansionChanged` if `f` expands or collapses anything.
    fn update_rows(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut LibraryDelegate)) {
        let changed = self.list.update(cx, |list, cx| {
            let before = list.delegate().expansion();
            f(list.delegate_mut());
            cx.notify();
            let after = list.delegate().expansion();
            (after != before).then_some(after)
        });
        if let Some(expansion) = changed {
            cx.emit(LibraryPanelEvent::ExpansionChanged(expansion));
        }
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
                let (collection_panel, environment_panel, import_panel) =
                    (panel.clone(), panel.clone(), panel.clone());
                menu.item(
                    PopupMenuItem::new("Collection")
                        .icon(Icon::new(LucideIcon::FolderPlus))
                        .on_click(move |_, _, cx| {
                            collection_panel
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
                        .on_click(move |_, _, cx| {
                            environment_panel
                                .update(cx, |panel, cx| panel.create_shared_environment(cx))
                                .ok();
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Import from OpenAPI…")
                        .icon(Icon::new(LucideIcon::Import))
                        .on_click(move |_, _, cx| {
                            import_panel
                                .update(cx, |_, cx| cx.emit(LibraryPanelEvent::ImportOpenApi))
                                .ok();
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
        v_flex()
            .key_context(LIBRARY_PANEL_CONTEXT)
            .on_action(
                cx.listener(|this, _: &SelectPreviousLibraryRow, window, cx| {
                    this.select_step(Direction::Up, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectNextLibraryRow, window, cx| {
                this.select_step(Direction::Down, window, cx)
            }))
            .size_full()
            .child(self.render_header(cx))
            .child(
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

    use domain::{
        collection::{Collection, Folder, Item, SavedRequest},
        environment::Environment,
        request::{Auth, Body, HttpMethod, Request},
    };
    use uuid::Uuid;

    use super::tree::{Row, Section};

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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{
        Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point, px,
    };

    use super::*;
    use crate::test_support::open_view;

    #[derive(Debug, PartialEq)]
    enum Emitted {
        Open(Uuid),
        Move(Uuid, Destination),
        MoveFolder(Uuid, Destination),
    }

    fn open_panel(
        cx: &mut TestAppContext,
    ) -> (
        Entity<LibraryPanel>,
        test_support::Ids,
        Rc<RefCell<Vec<Emitted>>>,
        &mut VisualTestContext,
    ) {
        let (panel, cx) = open_view(cx, LibraryPanel::new);
        let (collections, ids) = test_support::fixture();
        panel.update_in(cx, |panel, window, cx| {
            panel.set_collections(collections, true, window, cx);
            panel.reveal([ids.collection, ids.outer, ids.inner], cx);
        });
        let emitted = Rc::new(RefCell::new(Vec::new()));
        let sink = emitted.clone();
        cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &LibraryPanelEvent, _| match event {
                LibraryPanelEvent::OpenRequest { request_id, .. } => {
                    sink.borrow_mut().push(Emitted::Open(*request_id))
                }
                LibraryPanelEvent::MoveRequest { request_id, to, .. } => {
                    sink.borrow_mut().push(Emitted::Move(*request_id, *to))
                }
                LibraryPanelEvent::MoveFolder { folder_id, to, .. } => {
                    sink.borrow_mut().push(Emitted::MoveFolder(*folder_id, *to))
                }
                _ => {}
            })
            .detach()
        });
        cx.run_until_parked();
        (panel, ids, emitted, cx)
    }

    fn row(id: Uuid, cx: &mut VisualTestContext) -> gpui::Bounds<Pixels> {
        let selector = Box::leak(format!("library-row-{id}").into_boxed_str());
        cx.debug_bounds(selector).expect("row is rendered")
    }

    fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
        let none = Modifiers::none();
        cx.simulate_mouse_down(from, MouseButton::Left, none);
        cx.simulate_mouse_move(from + point(px(0.), px(6.)), MouseButton::Left, none);
        cx.simulate_mouse_move(to, MouseButton::Left, none);
        cx.simulate_mouse_up(to, MouseButton::Left, none);
        cx.run_until_parked();
    }

    fn top_half(bounds: gpui::Bounds<Pixels>) -> Point<Pixels> {
        point(bounds.center().x, bounds.top() + px(3.))
    }

    fn bottom_half(bounds: gpui::Bounds<Pixels>) -> Point<Pixels> {
        point(bounds.center().x, bounds.bottom() - px(3.))
    }

    #[gpui::test]
    fn dropping_a_request_emits_where_it_goes(cx: &mut TestAppContext) {
        let (panel, ids, emitted, cx) = open_panel(cx);
        let empty = panel.read_with(cx, |panel, cx| {
            panel.list.read(cx).delegate().collection_at(6).unwrap()
        });
        let at = |collection_id, folder_id, before| Destination {
            collection_id,
            folder_id,
            before,
        };
        let root = row(ids.root, cx).center();

        drag(root, top_half(row(ids.deep, cx)), cx);
        drag(root, bottom_half(row(ids.deep, cx)), cx);
        drag(root, row(ids.outer, cx).center(), cx);
        drag(root, row(empty, cx).center(), cx);
        drag(row(ids.deep, cx).center(), top_half(row(ids.outer, cx)), cx);
        drag(
            row(ids.deep, cx).center(),
            bottom_half(row(ids.outer, cx)),
            cx,
        );

        assert_eq!(
            *emitted.borrow(),
            [
                Emitted::Move(
                    ids.root,
                    at(ids.collection, Some(ids.inner), Some(ids.deep))
                ),
                Emitted::Move(ids.root, at(ids.collection, Some(ids.inner), None)),
                Emitted::Move(ids.root, at(ids.collection, Some(ids.outer), None)),
                Emitted::Move(ids.root, at(empty, None, None)),
                Emitted::Move(ids.deep, at(ids.collection, None, Some(ids.outer))),
                Emitted::Move(ids.deep, at(ids.collection, None, None)),
            ]
        );
    }

    #[gpui::test]
    fn dropping_a_folder_emits_where_it_goes(cx: &mut TestAppContext) {
        let (panel, ids, emitted, cx) = open_panel(cx);
        let empty = panel.read_with(cx, |panel, cx| {
            panel.list.read(cx).delegate().collection_at(6).unwrap()
        });
        let at = |collection_id, folder_id, before| Destination {
            collection_id,
            folder_id,
            before,
        };
        let inner = row(ids.inner, cx).center();

        drag(inner, top_half(row(ids.root, cx)), cx);
        drag(inner, top_half(row(ids.outer, cx)), cx);
        drag(inner, row(empty, cx).center(), cx);
        drag(row(ids.outer, cx).center(), top_half(row(ids.root, cx)), cx);

        assert_eq!(
            *emitted.borrow(),
            [
                Emitted::MoveFolder(ids.inner, at(ids.collection, None, Some(ids.root))),
                Emitted::MoveFolder(ids.inner, at(ids.collection, None, Some(ids.outer))),
                Emitted::MoveFolder(ids.inner, at(empty, None, None)),
                Emitted::MoveFolder(ids.outer, at(ids.collection, None, Some(ids.root))),
            ]
        );
    }

    #[gpui::test]
    fn folders_dont_drop_into_themselves(cx: &mut TestAppContext) {
        let (_, ids, emitted, cx) = open_panel(cx);
        let outer = row(ids.outer, cx);

        for target in [
            top_half(outer),
            outer.center(),
            bottom_half(outer),
            row(ids.inner, cx).center(),
            top_half(row(ids.deep, cx)),
            bottom_half(row(ids.deep, cx)),
        ] {
            drag(outer.center(), target, cx);
        }
        drag(row(ids.inner, cx).center(), row(ids.outer, cx).center(), cx);
        assert_eq!(*emitted.borrow(), []);
    }

    #[gpui::test]
    fn expanding_and_collapsing_emits_the_expansion(cx: &mut TestAppContext) {
        let (panel, ids, _, cx) = open_panel(cx);
        let expansions = Rc::new(RefCell::new(Vec::new()));
        let sink = expansions.clone();
        cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &LibraryPanelEvent, _| {
                if let LibraryPanelEvent::ExpansionChanged(expansion) = event {
                    sink.borrow_mut().push(expansion.clone())
                }
            })
            .detach()
        });

        let inner = row(ids.inner, cx).center();
        cx.simulate_click(inner, Modifiers::none());
        cx.run_until_parked();
        let last = || expansions.borrow().last().cloned().unwrap();
        assert_eq!(last().expanded, [ids.collection, ids.outer].into());

        panel.update_in(cx, |panel, _, cx| {
            panel.update_rows(cx, |delegate| {
                delegate.sections.toggle(Section::Environments)
            })
        });
        cx.run_until_parked();
        assert_eq!(last().collapsed_sections, [Section::Environments].into());

        let (other_library, _) = test_support::fixture();
        panel.update_in(cx, |panel, window, cx| {
            panel.set_collections(other_library.clone(), false, window, cx);
            panel.reveal([ids.inner], cx);
        });
        cx.run_until_parked();
        assert_eq!(expansions.borrow().len(), 3);
        assert_eq!(
            last().expanded,
            [ids.collection, ids.outer, ids.inner].into()
        );

        panel.update_in(cx, |panel, window, cx| {
            panel.set_collections(other_library, true, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(expansions.borrow().len(), 4);
        assert_eq!(last().expanded, [].into());
    }

    #[gpui::test]
    fn clicks_open_and_drops_in_place_do_nothing(cx: &mut TestAppContext) {
        let (_, ids, emitted, cx) = open_panel(cx);
        let root = row(ids.root, cx);

        cx.simulate_click(root.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(*emitted.borrow(), [Emitted::Open(ids.root)]);

        drag(root.center(), top_half(root), cx);
        drag(root.center(), bottom_half(root), cx);
        drag(row(ids.deep, cx).center(), row(ids.inner, cx).center(), cx);
        assert_eq!(*emitted.borrow(), [Emitted::Open(ids.root)]);
    }
}
