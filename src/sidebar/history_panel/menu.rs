use gpui::{Context, DismissEvent, WeakEntity, Window};
use gpui_component::{
    list::ListState,
    menu::{PopupMenu, PopupMenuItem},
};

use super::{HistoryPanel, HistoryPanelEvent, delegate::HistoryDelegate};
use crate::{
    actions::{DeleteHistoryEntry, SendHistoryEntryAgain},
    domain::history::OpenMode,
    ui::clipboard::copy_with_notification,
};

/// Entry actions apply to the right-clicked row; a right-click on empty space
/// only offers "Clear history…".
pub(super) fn context_menu(
    panel: &WeakEntity<HistoryPanel>,
    list: &WeakEntity<ListState<HistoryDelegate>>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(list_state) = list.upgrade() else {
        return menu;
    };
    let state = list_state.read(cx);
    let target = state.right_clicked_index().and_then(|ix| {
        let entry = state.delegate().entry(ix)?;
        Some((entry.id, entry.url.clone()))
    });
    let has_entries = !state.delegate().entries.is_empty();

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
    if let Some((id, url)) = target {
        let open_item = |label: &'static str, mode: OpenMode| {
            let (panel, list) = (panel.clone(), list.clone());
            let item = PopupMenuItem::new(label);
            let item = match mode {
                OpenMode::Send => item.action(Box::new(SendHistoryEntryAgain)),
                OpenMode::Load => item,
            };
            item.on_click(move |_, window, cx| {
                // Rows may have shifted since the menu opened.
                list.update(cx, |list, cx| {
                    let ix = list.delegate().index_of(id);
                    list.set_selected_index(ix, window, cx)
                })
                .ok();
                panel
                    .update(cx, |_, cx| cx.emit(HistoryPanelEvent::Open { id, mode }))
                    .ok();
            })
        };
        menu = menu
            .item(open_item("Open", OpenMode::Load))
            .item(open_item("Send again", OpenMode::Send))
            .item(
                PopupMenuItem::new("Copy URL").on_click(move |_, window, cx| {
                    copy_with_notification(url.clone(), "URL copied", window, cx);
                }),
            )
            .item(PopupMenuItem::new("Copy as cURL").on_click({
                let panel = panel.clone();
                move |_, _, cx| {
                    panel
                        .update(cx, |_, cx| cx.emit(HistoryPanelEvent::CopyAsCurl(id)))
                        .ok();
                }
            }))
            .separator()
            .item(
                PopupMenuItem::new("Delete")
                    .action(Box::new(DeleteHistoryEntry))
                    .on_click({
                        let panel = panel.clone();
                        move |_, _, cx| {
                            panel
                                .update(cx, |_, cx| cx.emit(HistoryPanelEvent::Delete(id)))
                                .ok();
                        }
                    }),
            );
    }

    let panel = panel.clone();
    menu.item(
        PopupMenuItem::new("Clear history…")
            .disabled(!has_entries)
            .on_click(move |_, window, cx| {
                panel
                    .update(cx, |panel, cx| panel.confirm_clear(window, cx))
                    .ok();
            }),
    )
}
