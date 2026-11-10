use std::ops::Range;

use chrono::{Datelike, Local, NaiveDate};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, IndexPath, Sizable, StyledExt, h_flex,
    list::{List, ListDelegate, ListItem, ListState},
    tag::Tag,
    tooltip::Tooltip,
};

use crate::{
    domain::store::history::{self, HistorySummary, Outcome},
    ui::status_tag::status_tag,
};

pub struct HistoryPanel {
    list: Entity<ListState<HistoryDelegate>>,
}

impl HistoryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list =
            cx.new(|cx| ListState::new(HistoryDelegate::default(), window, cx).selectable(false));
        Self { list }
    }

    pub fn set_entries(&mut self, entries: Vec<HistorySummary>, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_entries(entries);
            cx.notify();
        });
    }

    pub fn push(&mut self, entry: HistorySummary, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.delegate_mut().push(entry);
            cx.notify();
        });
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        List::new(&self.list).size_full()
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
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("Requests you send show up here.")
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

const FAILED_LABEL: &str = "Error";

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
