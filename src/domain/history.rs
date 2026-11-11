use std::{ops::Range, time::Duration};

use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, TimeZone, Utc};

use super::request::{HttpMethod, Request};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub status: u16,
    pub elapsed: Duration,
}

/// A sent request, before it's saved. `outcome` is `None` when no response arrived.
#[derive(Clone, Debug)]
pub struct HistoryRecord {
    pub sent_at: DateTime<Utc>,
    pub request: Request,
    pub outcome: Option<Outcome>,
}

/// What the history list shows; the full request stays in the database.
#[derive(Clone, Debug)]
pub struct HistorySummary {
    pub id: i64,
    pub sent_at: DateTime<Utc>,
    pub method: HttpMethod,
    pub url: String,
    pub outcome: Option<Outcome>,
}

/// What to do with a history entry once its full request is loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Load,
    Send,
}

/// Start of the oldest day to keep: local midnight `retention_days - 1` days
/// before `now`, so 1 keeps only today.
pub fn retention_cutoff<Tz: TimeZone>(now: &DateTime<Tz>, retention_days: u32) -> DateTime<Utc> {
    let first_day = now.date_naive() - Days::new(retention_days.max(1) as u64 - 1);
    day_start(first_day, &now.timezone())
}

/// Local midnight of `day`. Where a DST change skips midnight, the day starts
/// at its first valid hour.
fn day_start<Tz: TimeZone>(day: NaiveDate, tz: &Tz) -> DateTime<Utc> {
    (0..24)
        .find_map(|hour| {
            day.and_hms_opt(hour, 0, 0)?
                .and_local_timezone(tz.clone())
                .earliest()
        })
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| day.and_time(NaiveTime::MIN).and_utc())
}

/// Drops entries sent before `cutoff`. Returns whether any were.
pub fn prune_summaries(entries: &mut Vec<HistorySummary>, cutoff: DateTime<Utc>) -> bool {
    let len = entries.len();
    entries.retain(|entry| entry.sent_at >= cutoff);
    entries.len() != len
}

/// Whether `now` is on a different local day than `last_day`, so day labels
/// ("Today", "Yesterday") need refreshing.
pub fn day_rolled_over<Tz: TimeZone>(last_day: NaiveDate, now: &DateTime<Tz>) -> bool {
    now.date_naive() != last_day
}

/// How long to wait before the next prune: `interval`, or until just after
/// the next local midnight if that's sooner.
pub fn next_prune_in<Tz: TimeZone>(now: &DateTime<Tz>, interval: Duration) -> Duration {
    const PAST_MIDNIGHT: Duration = Duration::from_secs(1);
    let Some(tomorrow) = now.date_naive().succ_opt() else {
        return interval;
    };
    let until_midnight = (day_start(tomorrow, &now.timezone()) - now.with_timezone(&Utc))
        .to_std()
        .unwrap_or_default();
    interval.min(until_midnight + PAST_MIDNIGHT)
}

/// Index ranges into `entries`, one per local day. Expects `entries` newest first,
/// as `HistoryStore::since` returns them.
pub fn group_by_day<Tz: TimeZone>(
    entries: &[HistorySummary],
    tz: &Tz,
) -> Vec<(NaiveDate, Range<usize>)> {
    let day = |e: &HistorySummary| e.sent_at.with_timezone(tz).date_naive();
    let mut start = 0;
    entries
        .chunk_by(|a, b| day(a) == day(b))
        .map(|chunk| {
            let range = start..start + chunk.len();
            start = range.end;
            (day(&chunk[0]), range)
        })
        .collect()
}

/// "Today", "Yesterday", "Tue, 3 Nov", or with the year when it isn't this year's.
pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if day.year() == today.year() => day.format("%a, %-d %b").to_string(),
        _ => day.format("%a, %-d %b %Y").to_string(),
    }
}

/// When a request was sent, relative to `now`'s day: "Today 14:32".
pub fn sent_label<Tz: TimeZone>(sent_at: DateTime<Utc>, now: &DateTime<Tz>) -> String {
    let local = sent_at.with_timezone(&now.timezone());
    format!(
        "{} {}",
        day_label(local.date_naive(), now.date_naive()),
        local.time().format("%H:%M")
    )
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;
    use crate::domain::settings::HistorySettings;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    fn summary(sent_at: &str) -> HistorySummary {
        HistorySummary {
            id: 0,
            sent_at: at(sent_at),
            method: HttpMethod::Get,
            url: String::new(),
            outcome: None,
        }
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
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

    #[test]
    fn sent_labels_use_the_local_day_and_time() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 10, 15, 30, 0).unwrap();
        assert_eq!(sent_label(at("2026-11-10T12:05:00Z"), &now), "Today 14:05");
        assert_eq!(sent_label(at("2026-11-09T22:30:00Z"), &now), "Today 00:30");
        assert_eq!(
            sent_label(at("2026-11-09T21:30:00Z"), &now),
            "Yesterday 23:30"
        );
        assert_eq!(
            sent_label(at("2026-11-03T08:00:00Z"), &now),
            "Tue, 3 Nov 10:00"
        );
    }

    #[test]
    fn retention_cutoff_is_local_midnight() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 10, 15, 30, 0).unwrap();
        assert_eq!(retention_cutoff(&now, 14), at("2026-10-28T00:00:00+02:00"));
    }

    #[test]
    fn retention_cutoff_follows_the_setting() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 10, 15, 30, 0).unwrap();
        let days = HistorySettings { retention_days: 1 }.retention_days();
        assert_eq!(
            retention_cutoff(&now, days),
            at("2026-11-10T00:00:00+02:00")
        );
        let days = HistorySettings { retention_days: 0 }.retention_days();
        assert_eq!(
            retention_cutoff(&now, days),
            at("2026-11-10T00:00:00+02:00")
        );
        assert_eq!(retention_cutoff(&now, 0), at("2026-11-10T00:00:00+02:00"));
        let days = HistorySettings {
            retention_days: 365,
        }
        .retention_days();
        assert_eq!(
            retention_cutoff(&now, days),
            at("2025-11-11T00:00:00+02:00")
        );
    }

    #[test]
    fn prune_summaries_drops_entries_before_the_cutoff() {
        let mut entries = vec![
            summary("2026-11-10T08:00:00Z"),
            summary("2026-11-09T00:00:00Z"),
            summary("2026-11-08T23:59:59Z"),
        ];
        assert!(prune_summaries(&mut entries, at("2026-11-09T00:00:00Z")));
        let kept: Vec<_> = entries.iter().map(|e| e.sent_at).collect();
        assert_eq!(
            kept,
            [at("2026-11-10T08:00:00Z"), at("2026-11-09T00:00:00Z")]
        );
        assert!(!prune_summaries(&mut entries, at("2026-11-09T00:00:00Z")));
        assert!(prune_summaries(&mut entries, at("2026-11-11T00:00:00Z")));
        assert!(entries.is_empty());
    }

    #[test]
    fn day_rolls_over_at_local_midnight() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let day = date(2026, 11, 10);
        let before = tz.with_ymd_and_hms(2026, 11, 10, 23, 59, 59).unwrap();
        let after = tz.with_ymd_and_hms(2026, 11, 11, 0, 0, 1).unwrap();
        assert!(!day_rolled_over(day, &before));
        assert!(day_rolled_over(day, &after));
        // 22:30 UTC is already the next day here.
        assert!(day_rolled_over(
            day,
            &at("2026-11-10T22:30:00Z").with_timezone(&tz)
        ));
    }

    #[test]
    fn next_prune_is_hourly_or_just_after_midnight() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let hour = Duration::from_secs(3600);
        let noon = tz.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap();
        assert_eq!(next_prune_in(&noon, hour), hour);
        let late = tz.with_ymd_and_hms(2026, 11, 10, 23, 50, 0).unwrap();
        assert_eq!(next_prune_in(&late, hour), Duration::from_secs(10 * 60 + 1));
        let midnight = tz.with_ymd_and_hms(2026, 11, 11, 0, 0, 0).unwrap();
        assert_eq!(next_prune_in(&midnight, hour), hour);
    }

    #[test]
    fn group_by_day_uses_local_dates() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let entries = [
            summary("2026-11-10T08:00:00Z"),
            summary("2026-11-09T22:30:00Z"), // 00:30 local, Nov 10
            summary("2026-11-09T21:30:00Z"), // 23:30 local, Nov 9
        ];
        assert_eq!(
            group_by_day(&entries, &tz),
            [
                (NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(), 0..2),
                (NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), 2..3),
            ]
        );
    }
}
