use std::collections::VecDeque;

use gpui::SharedString;

use domain::{response::Response, send_error::SendError};

const MAX_ENTRIES: usize = 20;
/// Larger display bodies aren't kept.
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) enum CachedResponse {
    Received {
        result: Result<Response, SendError>,
        body_text: SharedString,
    },
    Cancelled,
}

/// The last response per key, least recently used first.
#[derive(Debug)]
pub(super) struct ResponseCache<K> {
    entries: VecDeque<(K, CachedResponse)>,
}

impl<K: Copy + Eq> ResponseCache<K> {
    pub(super) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// Replaces the entry for `key`. A body over the size limit isn't kept,
    /// and the older entry is dropped anyway since it's stale.
    pub(super) fn insert(
        &mut self,
        key: K,
        result: &Result<Response, SendError>,
        body_text: SharedString,
    ) {
        self.remove(key);
        if body_text.len() > MAX_BODY_BYTES {
            return;
        }
        let entry = CachedResponse::Received {
            result: result.clone(),
            body_text,
        };
        self.push(key, entry);
    }

    /// Replaces the entry for `key` with a note that its send was cancelled.
    pub(super) fn insert_cancelled(&mut self, key: K) {
        self.remove(key);
        self.push(key, CachedResponse::Cancelled);
    }

    fn push(&mut self, key: K, entry: CachedResponse) {
        self.entries.push_back((key, entry));
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    /// Returns the entry for `key` and marks it as recently used.
    pub(super) fn get(&mut self, key: K) -> Option<&CachedResponse> {
        let index = self.position(key)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, entry)| entry)
    }

    pub(super) fn remove(&mut self, key: K) {
        if let Some(index) = self.position(key) {
            self.entries.remove(index);
        }
    }

    /// Moves the entry for `from` to `to`, replacing what `to` had.
    pub(super) fn rekey(&mut self, from: K, to: K) {
        if from == to {
            return;
        }
        self.remove(to);
        if let Some(index) = self.position(from) {
            self.entries[index].0 = to;
        }
    }

    /// Copies the entry for `from` to `to`, replacing what `to` had.
    pub(super) fn copy(&mut self, from: K, to: K) {
        if from == to {
            return;
        }
        if let Some(entry) = self.position(from).map(|ix| self.entries[ix].1.clone()) {
            self.remove(to);
            self.push(to, entry);
        }
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(K) -> bool) {
        self.entries.retain(|(key, _)| keep(*key));
    }

    fn position(&self, key: K) -> Option<usize> {
        self.entries.iter().position(|(k, _)| *k == key)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn ok(status: u16) -> Result<Response, SendError> {
        Ok(Response::new(status, vec![], vec![], Duration::ZERO))
    }

    fn received(cache: &mut ResponseCache<u32>, key: u32) -> Option<&Result<Response, SendError>> {
        match cache.get(key)? {
            CachedResponse::Received { result, .. } => Some(result),
            CachedResponse::Cancelled => None,
        }
    }

    fn body(cache: &mut ResponseCache<u32>, key: u32) -> Option<SharedString> {
        match cache.get(key)? {
            CachedResponse::Received { body_text, .. } => Some(body_text.clone()),
            CachedResponse::Cancelled => None,
        }
    }

    fn status(cache: &mut ResponseCache<u32>, key: u32) -> Option<u16> {
        received(cache, key)
            .and_then(|result| result.as_ref().ok())
            .map(|response| response.status)
    }

    fn is_cancelled(cache: &mut ResponseCache<u32>, key: u32) -> bool {
        matches!(cache.get(key), Some(CachedResponse::Cancelled))
    }

    #[test]
    fn keeps_the_last_response_per_key() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(200), "first".into());
        cache.insert(1, &ok(404), "second".into());
        cache.insert(2, &Err(SendError::new("timed out")), "".into());
        assert_eq!(status(&mut cache, 1), Some(404));
        assert_eq!(body(&mut cache, 1).unwrap(), "second");
        assert_eq!(
            received(&mut cache, 2)
                .unwrap()
                .as_ref()
                .unwrap_err()
                .message,
            "timed out"
        );
        assert!(cache.get(3).is_none());
    }

    #[test]
    fn evicts_the_least_recently_used() {
        let mut cache = ResponseCache::new();
        for key in 0..MAX_ENTRIES as u32 {
            cache.insert(key, &ok(200), "".into());
        }
        cache.get(0);
        cache.insert(100, &ok(200), "".into());
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(cache.get(0).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.get(100).is_some());
    }

    #[test]
    fn skips_large_bodies_and_drops_the_stale_entry() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(200), "small".into());
        cache.insert(1, &ok(200), "x".repeat(MAX_BODY_BYTES + 1).into());
        assert!(cache.get(1).is_none());
        cache.insert(2, &ok(200), "x".repeat(MAX_BODY_BYTES).into());
        assert!(cache.get(2).is_some());
    }

    #[test]
    fn rekey_replaces_the_target() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(201), "".into());
        cache.insert(2, &ok(500), "".into());
        cache.rekey(1, 2);
        assert!(cache.get(1).is_none());
        assert_eq!(status(&mut cache, 2), Some(201));
        cache.rekey(3, 2);
        assert!(cache.get(2).is_none());
    }

    #[test]
    fn copy_keeps_the_source() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(201), "body".into());
        cache.insert(2, &ok(500), "".into());
        cache.copy(1, 2);
        assert_eq!(status(&mut cache, 1), Some(201));
        assert_eq!(status(&mut cache, 2), Some(201));
        cache.copy(3, 2);
        assert_eq!(status(&mut cache, 2), Some(201));
    }

    #[test]
    fn shares_the_body_instead_of_copying() {
        let mut cache = ResponseCache::new();
        let shared = SharedString::from("x".repeat(100));
        cache.insert(1, &ok(200), shared.clone());
        cache.copy(1, 2);
        assert_eq!(body(&mut cache, 1).unwrap().as_ptr(), shared.as_ptr());
        assert_eq!(body(&mut cache, 2).unwrap().as_ptr(), shared.as_ptr());
    }

    #[test]
    fn retain_and_remove_drop_entries() {
        let mut cache = ResponseCache::new();
        for key in 0..4 {
            cache.insert(key, &ok(200), "".into());
        }
        cache.retain(|key| key % 2 == 0);
        cache.remove(2);
        assert!(cache.get(0).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_none());
        assert!(cache.get(3).is_none());
    }

    #[test]
    fn cancelled_replaces_the_response_until_the_next_send() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(200), "body".into());
        cache.insert_cancelled(1);
        assert!(is_cancelled(&mut cache, 1));
        cache.copy(1, 2);
        assert!(is_cancelled(&mut cache, 2));
        cache.insert(1, &ok(201), "".into());
        assert_eq!(status(&mut cache, 1), Some(201));
        assert!(!is_cancelled(&mut cache, 1));
    }
}
