use gpui::{Context, DismissEvent, WeakEntity, Window};
use gpui_component::{
    list::ListState,
    menu::{PopupMenu, PopupMenuItem},
};
use uuid::Uuid;

use super::{LibraryPanel, LibraryPanelEvent, delegate::LibraryDelegate};
use crate::ui::{
    confirm_dialog::open_confirm_dialog, environments_editor::delete_message,
    rename_dialog::open_rename_dialog,
};

impl LibraryPanel {
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

    fn rename_environment(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.list.read(cx).delegate().environment_name(id) else {
            return;
        };
        let panel = cx.weak_entity();
        open_rename_dialog(
            "Rename environment",
            name,
            "Save",
            move |name, _, cx| {
                panel
                    .update(cx, |_, cx| {
                        cx.emit(LibraryPanelEvent::RenameSharedEnvironment { id, name })
                    })
                    .ok();
            },
            window,
            cx,
        );
    }

    fn delete_environment(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.list.read(cx).delegate().environment_name(id) else {
            return;
        };
        let links = self.environment_links.get(&id).copied().unwrap_or(0);
        let panel = cx.weak_entity();
        open_confirm_dialog(
            "Delete environment?",
            delete_message(&name, links),
            "Delete",
            move |_, cx| {
                panel
                    .update(cx, |_, cx| {
                        cx.emit(LibraryPanelEvent::DeleteSharedEnvironment(id))
                    })
                    .ok();
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
}

pub(super) fn context_menu(
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
    let Some(row) = state.right_clicked_index().map(|ix| ix.row) else {
        return menu;
    };
    let (target, environment) = (
        state.delegate().menu_target_at(row),
        state.delegate().environment_at(row),
    );
    if target.is_none() && environment.is_none() {
        return menu;
    }
    clear_right_click_on_dismiss(list, window, cx);
    match (target, environment) {
        (Some(target), _) => library_menu(panel, target, menu, cx),
        (None, Some(id)) => environment_menu(panel, id, menu),
        (None, None) => menu,
    }
}

/// The list outlines the right-clicked row until this is reset.
fn clear_right_click_on_dismiss(
    list: &WeakEntity<ListState<LibraryDelegate>>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) {
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
}

fn environment_menu(panel: &WeakEntity<LibraryPanel>, id: Uuid, menu: PopupMenu) -> PopupMenu {
    let (rename_panel, duplicate_panel, delete_panel) =
        (panel.clone(), panel.clone(), panel.clone());
    menu.item(
        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
            rename_panel
                .update(cx, |panel, cx| panel.rename_environment(id, window, cx))
                .ok();
        }),
    )
    .item(PopupMenuItem::new("Duplicate").on_click(move |_, _, cx| {
        duplicate_panel
            .update(cx, |_, cx| {
                cx.emit(LibraryPanelEvent::DuplicateSharedEnvironment(id))
            })
            .ok();
    }))
    .item(
        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
            delete_panel
                .update(cx, |panel, cx| panel.delete_environment(id, window, cx))
                .ok();
        }),
    )
}

fn library_menu(
    panel: &WeakEntity<LibraryPanel>,
    target: MenuTarget,
    menu: PopupMenu,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    if let MenuTarget::Request {
        collection_id,
        request_id,
    } = target
        && panel
            .upgrade()
            .is_some_and(|panel| panel.read(cx).dirty_request == Some(request_id))
    {
        let panel = panel.clone();
        menu = menu
            .item(
                PopupMenuItem::new("Revert changes…").on_click(move |_, _, cx| {
                    panel
                        .update(cx, |_, cx| {
                            cx.emit(LibraryPanelEvent::RevertRequest {
                                collection_id,
                                request_id,
                            })
                        })
                        .ok();
                }),
            )
            .separator();
    }
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
    if let MenuTarget::Collection(collection_id) = target
        && panel.upgrade().is_some_and(|panel| {
            panel
                .read(cx)
                .list
                .read(cx)
                .delegate()
                .is_imported(collection_id)
        })
    {
        let panel = panel.clone();
        menu = menu
            .item(
                PopupMenuItem::new("Update from spec…").on_click(move |_, _, cx| {
                    panel
                        .update(cx, |_, cx| {
                            cx.emit(LibraryPanelEvent::UpdateFromSpec(collection_id))
                        })
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
pub(super) enum MenuTarget {
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

    pub(super) fn collection_id(self) -> Uuid {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::library_panel::test_support::fixture;

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
}
