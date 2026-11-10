use chrono::{Datelike, Local, NaiveDate};
use gpui::ParentElement;
use gpui_component::{Sizable, tag::Tag};

use crate::{
    domain::history::{HistorySummary, Outcome},
    ui::status_tag::status_tag,
};

const FAILED_LABEL: &str = "Error";

pub(super) fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub(super) fn local_day(entry: &HistorySummary) -> NaiveDate {
    entry.sent_at.with_timezone(&Local).date_naive()
}

pub(super) fn request_count(count: usize) -> String {
    match count {
        0 => "No requests".to_string(),
        1 => "1 request".to_string(),
        n => format!("{n} requests"),
    }
}

pub(super) fn outcome_tag(outcome: Option<Outcome>) -> Tag {
    match outcome {
        Some(Outcome { status, .. }) => status_tag(status, status.to_string()),
        None => Tag::danger().small().child(FAILED_LABEL),
    }
}

pub(super) fn outcome_summary(outcome: Option<Outcome>) -> String {
    match outcome {
        Some(Outcome { status, elapsed }) => format!("{status} · {} ms", elapsed.as_millis()),
        None => FAILED_LABEL.to_string(),
    }
}

pub(super) fn day_label(day: NaiveDate, today: NaiveDate) -> String {
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
