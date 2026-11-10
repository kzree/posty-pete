use std::{collections::HashSet, ops::Range};

use chrono::{Datelike, Local, NaiveDate};
use gpui::{
    App, Context, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    Window, div,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, h_flex,
    list::{ListDelegate, ListItem, ListState},
    tooltip::Tooltip,
};

use super::format::{day_label, local_day, outcome_summary, outcome_tag, request_count, today};
use crate::{
    domain::history::{self, HistorySummary},
    sidebar::placeholder,
    ui::method_label::method_label,
};

#[derive(Default)]
pub(super) struct HistoryDelegate {
    pub(super) entries: Vec<HistorySummary>,
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
    pub(super) fn set_entries(&mut self, entries: Vec<HistorySummary>, today: NaiveDate) {
        self.entries = entries;
        self.expanded = HashSet::from([today]);
        self.regroup();
    }

    /// Expands the entry's day, so a new request is always visible.
    pub(super) fn push(&mut self, entry: HistorySummary) {
        self.expanded.insert(local_day(&entry));
        self.entries.insert(0, entry);
        self.regroup();
    }

    pub(super) fn remove(&mut self, id: i64) {
        self.entries.retain(|entry| entry.id != id);
        self.regroup();
    }

    pub(super) fn toggle(&mut self, day: NaiveDate) {
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

    pub(super) fn entry(&self, ix: IndexPath) -> Option<&HistorySummary> {
        match self.rows.get(ix.row)? {
            Row::Entry(entry) => self.entries.get(*entry),
            Row::Day(_) => None,
        }
    }

    pub(super) fn day_at(&self, ix: IndexPath) -> Option<NaiveDate> {
        match self.rows.get(ix.row)? {
            Row::Day(day) => self.days.get(*day).map(|(day, _)| *day),
            Row::Entry(_) => None,
        }
    }

    /// `None` if there's no such entry or its day is collapsed.
    pub(super) fn index_of(&self, id: i64) -> Option<IndexPath> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Entry(ix) if self.entries[*ix].id == id))
            .map(IndexPath::new)
    }

    pub(super) fn first_entry_row(&self) -> Option<IndexPath> {
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
                        .child(div().flex_1().min_w_0().truncate().text_xs().child(url))
                        .child(outcome_tag(entry.outcome).flex_none()),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        placeholder("Requests you send show up here.", cx)
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

#[cfg(test)]
mod tests {
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
}
