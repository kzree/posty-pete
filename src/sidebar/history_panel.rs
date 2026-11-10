use std::ops::Range;

use chrono::{Datelike, Local, NaiveDate};
use gpui::{
    App, AppContext, ClipboardItem, Context, DismissEvent, Entity, EventEmitter,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IndexPath, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants},
    h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    tag::Tag,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use crate::{
    actions::{DeleteHistoryEntry, HISTORY_PANEL_CONTEXT, SendHistoryEntryAgain},
    domain::store::history::{self, HistorySummary, Outcome},
    ui::status_tag::status_tag,
};

pub enum HistoryPanelEvent {
    Open(i64),
    /// Open the entry and send it right away.
    SendAgain(i64),
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
        let list_sub = cx.subscribe(&list, |_, list, event: &ListEvent, cx| {
            let ListEvent::Confirm(ix) = event else {
                return;
            };
            let id = list.read(cx).delegate().entry(*ix).map(|entry| entry.id);
            if let Some(id) = id {
                cx.emit(HistoryPanelEvent::Open(id));
            }
        });
        Self {
            list,
            _subscriptions: vec![list_sub],
        }
    }

    pub fn set_entries(&mut self, entries: Vec<HistorySummary>, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_entries(entries);
            cx.notify();
        });
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_entries(Vec::new());
            list.set_selected_index(None, window, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Clears the selection, since every row below the new entry shifts down one.
    pub fn push(&mut self, entry: HistorySummary, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().push(entry);
            list.set_selected_index(None, window, cx);
            cx.notify();
        });
        cx.notify();
    }

    pub fn remove(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().remove(id);
            list.set_selected_index(None, window, cx);
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
            cx.emit(HistoryPanelEvent::SendAgain(id));
        }
    }

    fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.list.read(cx).delegate().entries.len();
        let panel = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let panel = panel.clone();
            alert
                .title("Clear history?")
                .description(format!(
                    "{} will be deleted. This can't be undone.",
                    request_count(count)
                ))
                .show_cancel(true)
                .ok_text("Clear")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    panel
                        .update(cx, |_, cx| cx.emit(HistoryPanelEvent::Clear))
                        .ok();
                    true
                })
        });
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.list.read(cx).delegate().entries.len();
        h_flex()
            .flex_none()
            .justify_between()
            .pl_3()
            .pr_1()
            .py_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(request_count(count))
            .child(
                Button::new("clear-history")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(LucideIcon::Trash))
                    .tooltip("Clear history")
                    .accessibility_label("Clear history")
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_clear(window, cx))),
            )
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

#[derive(Default)]
struct HistoryDelegate {
    entries: Vec<HistorySummary>,
    days: Vec<(NaiveDate, Range<usize>)>,
}

impl HistoryDelegate {
    fn set_entries(&mut self, entries: Vec<HistorySummary>) {
        self.entries = entries;
        self.regroup();
    }

    fn push(&mut self, entry: HistorySummary) {
        self.entries.insert(0, entry);
        self.regroup();
    }

    fn remove(&mut self, id: i64) {
        self.entries.retain(|entry| entry.id != id);
        self.regroup();
    }

    fn regroup(&mut self) {
        self.days = history::group_by_day(&self.entries, &Local);
    }

    fn entry(&self, ix: IndexPath) -> Option<&HistorySummary> {
        let (_, range) = self.days.get(ix.section)?;
        self.entries.get(range.start + ix.row)
    }
}

impl ListDelegate for HistoryDelegate {
    type Item = ListItem;

    fn sections_count(&self, _cx: &App) -> usize {
        self.days.len()
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.days.get(section).map_or(0, |(_, range)| range.len())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let (day, _) = self.days.get(section)?;
        Some(
            div()
                .px_3()
                .pt_3()
                .pb_1()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(day_label(*day, Local::now().date_naive())),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let entry = self.entry(ix)?;
        let method = entry.method.clone();
        let url = entry.url.clone();
        let summary = outcome_summary(entry.outcome);
        let sent_at = entry.sent_at.with_timezone(&Local).format("%H:%M");
        let tooltip: SharedString = format!("{method} {url}\n{sent_at} · {summary}").into();

        Some(
            ListItem::new(("history-entry", entry.id as u64))
                .accessibility_label(format!("{method} {url}, {summary}, sent at {sent_at}"))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .text_sm()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .w(px(52.))
                                .flex_none()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child(method),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(url))
                        .child(outcome_tag(entry.outcome).flex_none()),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        super::placeholder("Requests you send show up here.", cx)
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

/// Entry actions apply to the right-clicked row; a right-click on empty space
/// only offers "Clear history…".
fn context_menu(
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
        Some((ix, entry.id, entry.url.clone()))
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
    if let Some((ix, id, url)) = target {
        let open_item = |label: &'static str, send: bool| {
            let (panel, list) = (panel.clone(), list.clone());
            let item = PopupMenuItem::new(label);
            let item = if send {
                item.action(Box::new(SendHistoryEntryAgain))
            } else {
                item
            };
            item.on_click(move |_, window, cx| {
                list.update(cx, |list, cx| list.set_selected_index(Some(ix), window, cx))
                    .ok();
                let event = if send {
                    HistoryPanelEvent::SendAgain(id)
                } else {
                    HistoryPanelEvent::Open(id)
                };
                panel.update(cx, |_, cx| cx.emit(event)).ok();
            })
        };
        menu = menu
            .item(open_item("Open", false))
            .item(open_item("Send again", true))
            .item(PopupMenuItem::new("Copy URL").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
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

const FAILED_LABEL: &str = "Error";

fn request_count(count: usize) -> String {
    match count {
        0 => "No requests".to_string(),
        1 => "1 request".to_string(),
        n => format!("{n} requests"),
    }
}

fn outcome_tag(outcome: Option<Outcome>) -> Tag {
    match outcome {
        Some(Outcome { status, .. }) => status_tag(status, status.to_string()),
        None => Tag::danger().small().child(FAILED_LABEL),
    }
}

fn outcome_summary(outcome: Option<Outcome>) -> String {
    match outcome {
        Some(Outcome { status, elapsed }) => format!("{status} · {} ms", elapsed.as_millis()),
        None => FAILED_LABEL.to_string(),
    }
}

fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if day.year() == today.year() => day.format("%a, %-d %b").to_string(),
        _ => day.format("%a, %-d %b %Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn request_counts() {
        assert_eq!(request_count(0), "No requests");
        assert_eq!(request_count(1), "1 request");
        assert_eq!(request_count(42), "42 requests");
    }

    #[test]
    fn outcome_summaries() {
        let outcome = Outcome {
            status: 404,
            elapsed: Duration::from_millis(42),
        };
        assert_eq!(outcome_summary(Some(outcome)), "404 · 42 ms");
        assert_eq!(outcome_summary(None), "Error");
    }

    #[test]
    fn day_labels() {
        let today = date(2026, 11, 10);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(date(2026, 11, 9), today), "Yesterday");
        assert_eq!(day_label(date(2026, 11, 3), today), "Tue, 3 Nov");
        assert_eq!(
            day_label(date(2025, 12, 31), date(2026, 1, 5)),
            "Wed, 31 Dec 2025"
        );
    }
}
