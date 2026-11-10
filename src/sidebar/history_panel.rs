use std::{collections::HashSet, ops::Range};

use chrono::{Datelike, Local, NaiveDate};
use gpui::{
    App, AppContext, Context, DismissEvent, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    WeakEntity, Window, div,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants},
    h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    tag::Tag,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use super::panel_header;
use crate::{
    actions::{DeleteHistoryEntry, HISTORY_PANEL_CONTEXT, SendHistoryEntryAgain},
    domain::history::{self, HistorySummary, OpenMode, Outcome},
    ui::{clipboard::copy_with_notification, method_label::method_label, status_tag::status_tag},
};

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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_rows_and_deselect(window, cx, |delegate| {
            delegate.set_entries(entries, today())
        });
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_rows_and_deselect(window, cx, |delegate| {
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
    expanded: HashSet<NaiveDate>,
    rows: Vec<Row>,
}

/// Day rows are list items too, since the list hides a section header once
/// its section has no rows.
enum Row {
    /// Index into `days`.
    Day(usize),
    /// Index into `entries`.
    Entry(usize),
}

impl HistoryDelegate {
    /// Only today starts expanded.
    fn set_entries(&mut self, entries: Vec<HistorySummary>, today: NaiveDate) {
        self.entries = entries;
        self.expanded = HashSet::from([today]);
        self.regroup();
    }

    /// Expands the entry's day, so a new request is always visible.
    fn push(&mut self, entry: HistorySummary) {
        self.expanded.insert(local_day(&entry));
        self.entries.insert(0, entry);
        self.regroup();
    }

    fn remove(&mut self, id: i64) {
        self.entries.retain(|entry| entry.id != id);
        self.regroup();
    }

    fn toggle(&mut self, day: NaiveDate) {
        if !self.expanded.remove(&day) {
            self.expanded.insert(day);
        }
        self.rebuild_rows();
    }

    fn regroup(&mut self) {
        self.days = history::group_by_day(&self.entries, &Local);
        self.rebuild_rows();
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (ix, (day, range)) in self.days.iter().enumerate() {
            self.rows.push(Row::Day(ix));
            if self.expanded.contains(day) {
                self.rows.extend(range.clone().map(Row::Entry));
            }
        }
    }

    fn entry(&self, ix: IndexPath) -> Option<&HistorySummary> {
        match self.rows.get(ix.row)? {
            Row::Entry(entry) => self.entries.get(*entry),
            Row::Day(_) => None,
        }
    }

    fn day_at(&self, ix: IndexPath) -> Option<NaiveDate> {
        match self.rows.get(ix.row)? {
            Row::Day(day) => self.days.get(*day).map(|(day, _)| *day),
            Row::Entry(_) => None,
        }
    }

    /// `None` if there's no such entry or its day is collapsed.
    fn index_of(&self, id: i64) -> Option<IndexPath> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Entry(ix) if self.entries[*ix].id == id))
            .map(IndexPath::new)
    }

    fn first_entry_row(&self) -> Option<IndexPath> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Entry(_)))
            .map(IndexPath::new)
    }

    fn render_day(&self, day: usize, cx: &App) -> Option<ListItem> {
        let (day, range) = self.days.get(day)?;
        let expanded = self.expanded.contains(day);
        let label = day_label(*day, today());
        let state = if expanded { "expanded" } else { "collapsed" };
        let chevron = if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        Some(
            ListItem::new(("history-day", day.num_days_from_ce() as u64))
                .accessibility_label(format!("{label}, {}, {state}", request_count(range.len())))
                .child(
                    h_flex()
                        .w_full()
                        .gap_1()
                        .text_xs()
                        .font_medium()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(chevron).xsmall())
                        .child(div().flex_1().child(label))
                        .child(range.len().to_string()),
                ),
        )
    }
}

impl ListDelegate for HistoryDelegate {
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
        if let Some(Row::Day(day)) = self.rows.get(ix.row) {
            return self.render_day(*day, cx);
        }
        let entry = self.entry(ix)?;
        let method = entry.method.as_str();
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
                        .child(method_label(entry.method, cx))
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

const FAILED_LABEL: &str = "Error";

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn local_day(entry: &HistorySummary) -> NaiveDate {
    entry.sent_at.with_timezone(&Local).date_naive()
}

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

    fn summary(id: i64, day: NaiveDate, hour: u32) -> HistorySummary {
        use chrono::{TimeZone, Utc};
        let local = Local
            .from_local_datetime(&day.and_hms_opt(hour, 0, 0).unwrap())
            .earliest()
            .unwrap();
        HistorySummary {
            id,
            sent_at: local.with_timezone(&Utc),
            method: crate::domain::request::HttpMethod::Get,
            url: format!("https://example.com/{id}"),
            outcome: None,
        }
    }

    fn ids(delegate: &HistoryDelegate) -> Vec<String> {
        (0..delegate.rows.len())
            .map(|row| {
                let ix = IndexPath::new(row);
                match (delegate.day_at(ix), delegate.entry(ix)) {
                    (Some(day), _) => day.format("%d").to_string(),
                    (_, Some(entry)) => entry.id.to_string(),
                    _ => unreachable!(),
                }
            })
            .collect()
    }

    fn two_days() -> (HistoryDelegate, NaiveDate, NaiveDate) {
        let (today, yesterday) = (date(2026, 11, 10), date(2026, 11, 9));
        let mut delegate = HistoryDelegate::default();
        delegate.set_entries(
            vec![
                summary(3, today, 12),
                summary(2, today, 10),
                summary(1, yesterday, 12),
            ],
            today,
        );
        (delegate, today, yesterday)
    }

    #[test]
    fn only_today_starts_expanded() {
        let (delegate, _, _) = two_days();
        assert_eq!(ids(&delegate), ["10", "3", "2", "09"]);
    }

    #[test]
    fn toggle_expands_and_collapses_a_day() {
        let (mut delegate, today, yesterday) = two_days();
        delegate.toggle(yesterday);
        assert_eq!(ids(&delegate), ["10", "3", "2", "09", "1"]);
        delegate.toggle(today);
        assert_eq!(ids(&delegate), ["10", "09", "1"]);
    }

    #[test]
    fn push_expands_its_day() {
        let (mut delegate, today, _) = two_days();
        delegate.toggle(today);
        delegate.push(summary(4, today, 14));
        assert_eq!(ids(&delegate), ["10", "4", "3", "2", "09"]);
    }

    #[test]
    fn index_of_skips_collapsed_days() {
        let (mut delegate, _, yesterday) = two_days();
        assert_eq!(delegate.index_of(2), Some(IndexPath::new(2)));
        assert_eq!(delegate.index_of(1), None);
        assert_eq!(delegate.index_of(9), None);
        assert_eq!(delegate.first_entry_row(), Some(IndexPath::new(1)));

        delegate.toggle(yesterday);
        assert_eq!(delegate.index_of(1), Some(IndexPath::new(4)));

        delegate.remove(3);
        assert_eq!(delegate.index_of(1), Some(IndexPath::new(3)));
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
