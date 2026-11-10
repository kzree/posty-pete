use std::{ops::Range, time::Duration};

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone, Utc};

use super::request::{HttpMethod, Request};

pub const RETENTION_DAYS: u64 = 14;

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

/// Start of the oldest day to keep: local midnight `RETENTION_DAYS - 1` days before `now`.
/// Where a DST change skips midnight, the day starts at its first valid hour.
pub fn retention_cutoff<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Utc> {
    let first_day = now.date_naive() - Days::new(RETENTION_DAYS - 1);
    (0..24)
        .find_map(|hour| {
            first_day
                .and_hms_opt(hour, 0, 0)?
                .and_local_timezone(now.timezone())
                .earliest()
        })
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| first_day.and_time(NaiveTime::MIN).and_utc())
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

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

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

    #[test]
    fn retention_cutoff_is_local_midnight() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 10, 15, 30, 0).unwrap();
        assert_eq!(retention_cutoff(&now), at("2026-10-28T00:00:00+02:00"));
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
