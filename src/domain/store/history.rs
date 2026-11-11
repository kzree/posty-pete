use std::{path::Path, time::Duration};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};

use crate::domain::{
    history::{HistoryRecord, HistorySummary, Outcome},
    request::{HttpMethod, Request},
};

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

    #[cfg(test)]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
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

    pub fn get(&self, id: i64) -> anyhow::Result<Option<Request>> {
        let mut stmt = self
            .conn
            .prepare("SELECT request FROM history WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            let request_json: String = row.get(0)?;
            let request: Request = serde_json::from_str(&request_json)?;
            Ok(Some(request))
        } else {
            Ok(None)
        }
    }

    pub fn insert(&self, record: &HistoryRecord) -> anyhow::Result<HistorySummary> {
        let request = &record.request;
        let (status, elapsed_ms) = match record.outcome {
            Some(outcome) => (
                Some(outcome.status),
                Some(i64::try_from(outcome.elapsed.as_millis()).unwrap_or(i64::MAX)),
            ),
            None => (None, None),
        };
        self.conn.execute(
            "INSERT INTO history (sent_at, method, url, status, elapsed_ms, request)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.sent_at.timestamp_millis(),
                request.method.as_str(),
                request.url,
                status,
                elapsed_ms,
                serde_json::to_string(request)?,
            ],
        )?;

        Ok(HistorySummary {
            id: self.conn.last_insert_rowid(),
            sent_at: record.sent_at,
            method: request.method,
            url: request.url.clone(),
            outcome: record.outcome,
        })
    }

    /// Newest first. Rows that can't be decoded are skipped with a warning.
    pub fn since(&self, cutoff: DateTime<Utc>) -> anyhow::Result<Vec<HistorySummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sent_at, method, url, status, elapsed_ms FROM history
             WHERE sent_at >= ?1
             ORDER BY sent_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([cutoff.timestamp_millis()], RawRow::read)?;

        Ok(rows
            .filter_map(|row| match row {
                Ok(row) => row.into_summary(),
                Err(e) => {
                    tracing::warn!("skipping unreadable history row: {e}");
                    None
                }
            })
            .collect())
    }

    /// Returns `false` if there was no entry with this id.
    pub fn delete(&self, id: i64) -> anyhow::Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM history WHERE id = ?1", [id])?
            > 0)
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        Ok(self.conn.execute("DELETE FROM history", [])?)
    }

    /// Drops entries older than `cutoff` and loads the rest. A failed prune is
    /// only logged.
    pub fn prune_and_load(&self, cutoff: DateTime<Utc>) -> anyhow::Result<Vec<HistorySummary>> {
        if let Err(e) = self.prune(cutoff) {
            tracing::warn!("failed to prune history: {e:#}");
        }
        self.since(cutoff)
    }

    pub fn prune(&self, cutoff: DateTime<Utc>) -> anyhow::Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM history WHERE sent_at < ?1",
            [cutoff.timestamp_millis()],
        )?)
    }
}

struct RawRow {
    id: i64,
    sent_at: i64,
    method: String,
    url: String,
    status: Option<i64>,
    elapsed_ms: Option<i64>,
}

impl RawRow {
    fn read(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            sent_at: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            elapsed_ms: row.get(5)?,
        })
    }

    fn into_summary(self) -> Option<HistorySummary> {
        let id = self.id;
        let Some(sent_at) = DateTime::from_timestamp_millis(self.sent_at) else {
            tracing::warn!(id, "skipping history row with invalid timestamp");
            return None;
        };
        let Some(method) = HttpMethod::parse(&self.method) else {
            tracing::warn!(
                id,
                method = self.method,
                "skipping history row with unknown method"
            );
            return None;
        };
        let outcome = match (self.status, self.elapsed_ms) {
            (None, _) => None,
            (Some(status), elapsed_ms) => {
                let Ok(status) = u16::try_from(status) else {
                    tracing::warn!(id, status, "skipping history row with invalid status");
                    return None;
                };
                let elapsed_ms = elapsed_ms.unwrap_or(0).max(0) as u64;
                Some(Outcome {
                    status,
                    elapsed: Duration::from_millis(elapsed_ms),
                })
            }
        };
        Some(HistorySummary {
            id,
            sent_at,
            method,
            url: self.url,
            outcome,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::domain::{
        history::retention_cutoff,
        request::{Auth, Body, KeyValue, RawKind},
        store::test_support::TempDir,
    };

    fn record(sent_at: DateTime<Utc>, url: &str, outcome: Option<Outcome>) -> HistoryRecord {
        HistoryRecord {
            sent_at,
            request: Request {
                method: HttpMethod::Get,
                url: url.into(),
                params: Vec::new(),
                headers: Vec::new(),
                body: Default::default(),
                auth: Default::default(),
            },
            outcome,
        }
    }

    fn ok(sent_at: DateTime<Utc>, url: &str) -> HistoryRecord {
        let outcome = Outcome {
            status: 200,
            elapsed: Duration::ZERO,
        };
        record(sent_at, url, Some(outcome))
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    fn insert_raw(store: &HistoryStore, sent_at: i64, method: &str, status: &str) {
        store
            .conn
            .execute(
                &format!(
                    "INSERT INTO history (sent_at, method, url, status, request)
                     VALUES (?1, '{method}', 'bad', {status}, '{{}}')"
                ),
                [sent_at],
            )
            .unwrap();
    }

    #[test]
    fn insert_and_read_back() {
        let store = HistoryStore::open_in_memory().unwrap();
        let sent_at = at("2026-11-10T12:00:00Z");
        let outcome = Outcome {
            status: 201,
            elapsed: Duration::from_millis(42),
        };
        let inserted = store
            .insert(&record(sent_at, "https://a.test", Some(outcome)))
            .unwrap();

        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.id, inserted.id);
        assert_eq!(entry.sent_at, sent_at);
        assert_eq!(entry.method, HttpMethod::Get);
        assert_eq!(entry.url, "https://a.test");
        assert_eq!(entry.outcome, Some(outcome));
    }

    #[test]
    fn get_returns_the_full_request() {
        let store = HistoryStore::open_in_memory().unwrap();
        let mut sent = ok(at("2026-11-10T12:00:00Z"), "https://a.test");
        sent.request.method = HttpMethod::Post;
        sent.request.headers = vec![KeyValue {
            enabled: false,
            key: "X-Trace".into(),
            value: "1".into(),
        }];
        sent.request.body = Body::Raw {
            kind: RawKind::Json,
            text: r#"{"a":1}"#.into(),
        };
        sent.request.auth = Auth::Bearer {
            token: "secret".into(),
        };
        let id = store.insert(&sent).unwrap().id;

        let request = store.get(id).unwrap().unwrap();
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, "https://a.test");
        assert!(matches!(
            request.headers.as_slice(),
            [KeyValue { enabled: false, key, value }] if key == "X-Trace" && value == "1"
        ));
        assert!(matches!(
            &request.body,
            Body::Raw { kind: RawKind::Json, text } if text == r#"{"a":1}"#
        ));
        assert!(matches!(&request.auth, Auth::Bearer { token } if token == "secret"));
    }

    #[test]
    fn get_unknown_id_is_none() {
        let store = HistoryStore::open_in_memory().unwrap();
        assert!(store.get(42).unwrap().is_none());
    }

    #[test]
    fn failed_request_has_no_outcome() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .insert(&record(at("2026-11-10T12:00:00Z"), "https://a.test", None))
            .unwrap();

        let entry = &store.since(DateTime::UNIX_EPOCH).unwrap()[0];
        assert_eq!(entry.outcome, None);
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
            store.insert(&ok(at(time), url)).unwrap();
        }

        let urls: Vec<_> = store
            .since(at("2026-11-09T00:00:00Z"))
            .unwrap()
            .into_iter()
            .map(|e| e.url)
            .collect();
        assert_eq!(urls, ["c", "b", "a"]);
    }

    #[test]
    fn prune_removes_entries_before_cutoff() {
        let store = HistoryStore::open_in_memory().unwrap();
        let now = at("2026-11-10T12:00:00Z");
        store
            .insert(&ok(now - TimeDelta::days(13), "keep"))
            .unwrap();
        store
            .insert(&ok(now - TimeDelta::days(15), "drop"))
            .unwrap();

        assert_eq!(store.prune(retention_cutoff(&now)).unwrap(), 1);
        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "keep");
    }

    #[test]
    fn prune_and_load_prunes_then_loads() {
        let store = HistoryStore::open_in_memory().unwrap();
        let now = at("2026-11-10T12:00:00Z");
        store.insert(&ok(now - TimeDelta::days(1), "keep")).unwrap();
        store
            .insert(&ok(now - TimeDelta::days(15), "drop"))
            .unwrap();

        let entries = store.prune_and_load(retention_cutoff(&now)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "keep");
        assert_eq!(store.since(DateTime::UNIX_EPOCH).unwrap().len(), 1);
    }

    #[test]
    fn failed_load_is_reported_and_recording_still_works() {
        let dir = TempDir::new();
        let path = dir.path().join("history.db");
        let now = at("2026-11-10T12:00:00Z");
        HistoryStore::open(&path)
            .unwrap()
            .insert(&ok(now, "older"))
            .unwrap();
        // Loading selects `id`, recording doesn't.
        Connection::open(&path)
            .unwrap()
            .execute_batch("ALTER TABLE history RENAME COLUMN id TO entry_id")
            .unwrap();

        let store = HistoryStore::open(&path).unwrap();
        assert!(store.prune_and_load(retention_cutoff(&now)).is_err());
        store.insert(&ok(now, "new")).unwrap();
    }

    #[test]
    fn delete_removes_only_that_entry() {
        let store = HistoryStore::open_in_memory().unwrap();
        let keep = store
            .insert(&ok(at("2026-11-10T11:00:00Z"), "keep"))
            .unwrap();
        let drop = store
            .insert(&ok(at("2026-11-10T12:00:00Z"), "drop"))
            .unwrap();

        assert!(store.delete(drop.id).unwrap());
        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, keep.id);
    }

    #[test]
    fn delete_unknown_id_is_false() {
        let store = HistoryStore::open_in_memory().unwrap();
        assert!(!store.delete(42).unwrap());
    }

    #[test]
    fn clear_removes_everything() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.insert(&ok(at("2026-11-09T12:00:00Z"), "a")).unwrap();
        store.insert(&ok(at("2026-11-10T12:00:00Z"), "b")).unwrap();

        assert_eq!(store.clear().unwrap(), 2);
        assert!(store.since(DateTime::UNIX_EPOCH).unwrap().is_empty());
    }

    #[test]
    fn undecodable_rows_are_skipped() {
        let store = HistoryStore::open_in_memory().unwrap();
        let sent_at = at("2026-11-10T12:00:00Z");
        store.insert(&ok(sent_at, "ok")).unwrap();
        let later = sent_at.timestamp_millis() + 1;
        insert_raw(&store, later, "GET", "70000");
        insert_raw(&store, later, "GET", "'not a number'");
        insert_raw(&store, later, "BREW", "200");

        let entries = store.since(DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "ok");
    }
}
