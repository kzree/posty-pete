mod delegate;
mod format;
mod menu;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, div,
};
use gpui_component::{
    Disableable, Icon, Sizable,
    button::{Button, ButtonVariants},
    list::{List, ListEvent, ListState},
    menu::ContextMenuExt,
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use super::{panel_header, placeholder};
use crate::{
    actions::{DeleteHistoryEntry, HISTORY_PANEL_CONTEXT, SendHistoryEntryAgain},
    domain::history::{HistorySummary, OpenMode},
    ui::confirm_dialog::open_confirm_dialog,
};
use delegate::HistoryDelegate;
use format::{request_count, today};
use menu::context_menu;

pub enum HistoryPanelEvent {
    Open {
        id: i64,
        mode: OpenMode,
    },
    CopyAsCurl(i64),
    Delete(i64),
    /// The user confirmed clearing all history.
    Clear,
}

pub struct HistoryPanel {
    list: Entity<ListState<HistoryDelegate>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<HistoryPanelEvent> for HistoryPanel {}

impl HistoryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| ListState::new(HistoryDelegate::default(), window, cx));
        let list_sub = cx.subscribe_in(&list, window, |this, list, event, window, cx| {
            let ListEvent::Confirm(ix) = event else {
                return;
            };
            let delegate = list.read(cx).delegate();
            if let Some(day) = delegate.day_at(*ix) {
                // Rows above the day don't move, so its row stays selected.
                this.update_rows(window, cx, |delegate| delegate.toggle(day));
            } else if let Some(entry) = delegate.entry(*ix) {
                cx.emit(HistoryPanelEvent::Open {
                    id: entry.id,
                    mode: OpenMode::Load,
                });
            }
        });
        Self {
            list,
            _subscriptions: vec![list_sub],
        }
    }

    pub fn set_entries(
        &mut self,
        entries: Vec<HistorySummary>,
        load_failed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows_and_deselect(window, cx, |delegate| {
            delegate.load_failed = load_failed;
            delegate.set_entries(entries, today())
        });
    }

    /// Newest first.
    pub fn entries<'a>(&self, cx: &'a App) -> &'a [HistorySummary] {
        &self.list.read(cx).delegate().entries
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_rows_and_deselect(window, cx, |delegate| {
            delegate.load_failed = false;
            delegate.set_entries(Vec::new(), today())
        });
    }

    pub fn push(&mut self, entry: HistorySummary, window: &mut Window, cx: &mut Context<Self>) {
        self.update_rows_and_deselect(window, cx, |delegate| delegate.push(entry));
    }

    pub fn remove(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.update_rows_and_deselect(window, cx, |delegate| delegate.remove(id));
    }

    /// Clears the selection too, since rows below the change shift.
    fn update_rows_and_deselect(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut HistoryDelegate),
    ) {
        self.update_rows(window, cx, f);
        self.list
            .update(cx, |list, cx| list.set_selected_index(None, window, cx));
    }

    fn update_rows(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut HistoryDelegate),
    ) {
        self.list.update(cx, |list, cx| {
            f(list.delegate_mut());
            // Every row gets the measured row's height, so measure an entry
            // rather than the shorter day rows.
            if let Some(ix) = list.delegate().first_entry_row() {
                list.set_item_to_measure_index(ix, window, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn selected_id(&self, cx: &App) -> Option<i64> {
        let list = self.list.read(cx);
        let ix = list.selected_index()?;
        list.delegate().entry(ix).map(|entry| entry.id)
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_id(cx) {
            cx.emit(HistoryPanelEvent::Delete(id));
        }
    }

    fn send_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_id(cx) {
            cx.emit(HistoryPanelEvent::Open {
                id,
                mode: OpenMode::Send,
            });
        }
    }

    fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.list.read(cx).delegate().entries.len();
        let panel = cx.weak_entity();
        open_confirm_dialog(
            "Clear history?",
            format!(
                "{} will be deleted. This can't be undone.",
                request_count(count)
            ),
            "Clear",
            move |_, cx| {
                panel
                    .update(cx, |_, cx| cx.emit(HistoryPanelEvent::Clear))
                    .ok();
            },
            window,
            cx,
        );
    }

    fn render_load_failed(&self, cx: &App) -> Option<impl IntoElement> {
        self.list.read(cx).delegate().load_failed.then(|| {
            placeholder(
                "Couldn't load earlier history. New requests are still saved.",
                cx,
            )
        })
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.list.read(cx).delegate().entries.len();
        let clear_button = Button::new("clear-history")
            .ghost()
            .xsmall()
            .icon(Icon::new(LucideIcon::Trash))
            .tooltip("Clear history")
            .accessibility_label("Clear history")
            .disabled(count == 0)
            .on_click(cx.listener(|this, _, window, cx| this.confirm_clear(window, cx)));
        panel_header(request_count(count), clear_button, cx)
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.weak_entity();
        let list = self.list.downgrade();
        // On the root, so the context menu's trigger sits inside this key
        // context and can show the Delete shortcut.
        v_flex()
            .key_context(HISTORY_PANEL_CONTEXT)
            .on_action(cx.listener(|this, _: &DeleteHistoryEntry, _, cx| this.delete_selected(cx)))
            .on_action(cx.listener(|this, _: &SendHistoryEntryAgain, _, cx| this.send_selected(cx)))
            .size_full()
            .child(self.render_header(cx))
            .children(self.render_load_failed(cx))
            .child(
                div()
                    .id("history-list")
                    .flex_1()
                    .min_h_0()
                    .child(List::new(&self.list).size_full())
                    .context_menu(move |menu, window, cx| {
                        context_menu(&panel, &list, menu, window, cx)
                    }),
            )
    }
}
