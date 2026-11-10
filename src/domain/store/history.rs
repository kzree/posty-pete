use std::{path::Path, time::Duration};

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone, Utc};
use rusqlite::{Connection, params};

use crate::domain::request::Request;

pub const RETENTION_DAYS: u64 = 14;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub id: i64,
    pub sent_at: DateTime<Utc>,
    pub request: Request,
    pub status: Option<u16>,
    pub elapsed: Option<Duration>,
}

pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS history (
                 id         INTEGER PRIMARY KEY,
                 sent_at    INTEGER NOT NULL,
                 method     TEXT NOT NULL,
                 url        TEXT NOT NULL,
                 status     INTEGER,
                 elapsed_ms INTEGER,
                 request    TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS history_sent_at ON history(sent_at DESC);",
        )?;
        Ok(Self { conn })
    }

    pub fn insert(
        &self,
        sent_at: DateTime<Utc>,
        request: &Request,
        status: Option<u16>,
        elapsed: Option<Duration>,
    ) -> anyhow::Result<HistoryEntry> {
        let elapsed_ms = elapsed.map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        self.conn.execute(
            "INSERT INTO history (sent_at, method, url, status, elapsed_ms, request)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                sent_at.timestamp_millis(),
                request.method.as_str(),
                request.url,
                status,
                elapsed_ms,
                serde_json::to_string(request)?,
            ],
        )?;

        Ok(HistoryEntry {
            id: self.conn.last_insert_rowid(),
            sent_at,
            request: request.clone(),
            status,
            elapsed,
        })
    }

    /// Newest first. Rows whose request no longer deserializes are skipped.
    pub fn since(&self, cutoff: DateTime<Utc>) -> anyhow::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sent_at, status, elapsed_ms, request FROM history
             WHERE sent_at >= ?1
             ORDER BY sent_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([cutoff.timestamp_millis()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<u16>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let (id, sent_at, status, elapsed_ms, request) = row?;
            let Some(sent_at) = DateTime::from_timestamp_millis(sent_at) else {
                tracing::warn!(id, sent_at, "skipping history entry with invalid timestamp");
                continue;
            };
            let request = match serde_json::from_str(&request) {
                Ok(request) => request,
                Err(e) => {
                    tracing::warn!(id, "skipping unreadable history entry: {e}");
                    continue;
                }
            };
            entries.push(HistoryEntry {
                id,
                sent_at,
                request,
                status,
                elapsed: elapsed_ms.map(|ms| Duration::from_millis(ms.max(0) as u64)),
            });
        }
        Ok(entries)
    }

    pub fn prune(&self, cutoff: DateTime<Utc>) -> anyhow::Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM history WHERE sent_at < ?1",
            [cutoff.timestamp_millis()],
        )?)
    }
}

/// Start of the oldest day to keep: local midnight `RETENTION_DAYS - 1` days before `now`.
pub fn retention_cutoff<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Utc> {
    let first_day = (now.date_naive() - Days::new(RETENTION_DAYS - 1)).and_time(NaiveTime::MIN);
    first_day
        .and_local_timezone(now.timezone())
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| first_day.and_utc())
}

/// Expects `entries` newest first, as returned by [`HistoryStore::since`].
pub fn group_by_day<'a, Tz: TimeZone>(
    entries: &'a [HistoryEntry],
    tz: &Tz,
) -> Vec<(NaiveDate, &'a [HistoryEntry])> {
    let day = |e: &HistoryEntry| e.sent_at.with_timezone(tz).date_naive();
    entries
        .chunk_by(|a, b| day(a) == day(b))
        .map(|chunk| (day(&chunk[0]), chunk))
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeDelta};

    use super::*;
    use crate::domain::request::HttpMethod;

    fn request(url: &str) -> Request {
        Request {
            method: HttpMethod::Get,
            url: url.into(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Default::default(),
            auth: Default::default(),
        }
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    #[test]
    fn insert_and_read_back() {
        let store = HistoryStore::open_in_memory().unwrap();
        let sent_at = at("2026-11-10T12:00:00Z");
        let inserted = store
            .insert(
                sent_at,
                &request("https://a.test"),
                Some(200),
                Some(Duration::from_millis(42)),
            )
            .unwrap();

        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.id, inserted.id);
        assert_eq!(entry.sent_at, sent_at);
        assert_eq!(entry.request.url, "https://a.test");
        assert_eq!(entry.status, Some(200));
        assert_eq!(entry.elapsed, Some(Duration::from_millis(42)));
    }

    #[test]
    fn failed_request_has_no_status() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .insert(
                at("2026-11-10T12:00:00Z"),
                &request("https://a.test"),
                None,
                None,
            )
            .unwrap();

        let entry = &store.since(DateTime::UNIX_EPOCH).unwrap()[0];
        assert_eq!(entry.status, None);
        assert_eq!(entry.elapsed, None);
    }

    #[test]
    fn since_returns_newest_first_and_respects_cutoff() {
        let store = HistoryStore::open_in_memory().unwrap();
        for (time, url) in [
            ("2026-11-08T12:00:00Z", "old"),
            ("2026-11-10T09:00:00Z", "b"),
            ("2026-11-10T10:00:00Z", "c"),
            ("2026-11-09T12:00:00Z", "a"),
        ] {
            store
                .insert(at(time), &request(url), Some(200), None)
                .unwrap();
        }

        let urls: Vec<_> = store
            .since(at("2026-11-09T00:00:00Z"))
            .unwrap()
            .into_iter()
            .map(|e| e.request.url)
            .collect();
        assert_eq!(urls, ["c", "b", "a"]);
    }

    #[test]
    fn prune_removes_entries_before_cutoff() {
        let store = HistoryStore::open_in_memory().unwrap();
        let now = at("2026-11-10T12:00:00Z");
        store
            .insert(now - TimeDelta::days(13), &request("keep"), Some(200), None)
            .unwrap();
        store
            .insert(now - TimeDelta::days(15), &request("drop"), Some(200), None)
            .unwrap();

        assert_eq!(store.prune(retention_cutoff(&now)).unwrap(), 1);
        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request.url, "keep");
    }

    #[test]
    fn unreadable_rows_are_skipped() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .insert(at("2026-11-10T12:00:00Z"), &request("ok"), Some(200), None)
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO history (sent_at, method, url, request) VALUES (?1, 'GET', 'bad', '{')",
                [at("2026-11-10T13:00:00Z").timestamp_millis()],
            )
            .unwrap();

        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request.url, "ok");
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
        let store = HistoryStore::open_in_memory().unwrap();
        for time in [
            "2026-11-09T21:30:00Z", // 23:30 local, Nov 9
            "2026-11-09T22:30:00Z", // 00:30 local, Nov 10
            "2026-11-10T08:00:00Z",
        ] {
            store
                .insert(at(time), &request(time), Some(200), None)
                .unwrap();
        }
        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();

        let groups: Vec<_> = group_by_day(&entries, &tz)
            .into_iter()
            .map(|(day, entries)| (day, entries.len()))
            .collect();
        assert_eq!(
            groups,
            [
                (NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(), 2),
                (NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), 1),
            ]
        );
    }
}
