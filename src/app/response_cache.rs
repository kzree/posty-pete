use std::collections::VecDeque;

use crate::domain::response::Response;

const MAX_ENTRIES: usize = 20;
/// Larger display bodies aren't kept.
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct CachedResponse {
    pub(super) result: Result<Response, String>,
    pub(super) body_text: String,
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
    pub(super) fn insert(&mut self, key: K, result: &Result<Response, String>, body_text: &str) {
        self.remove(key);
        if body_text.len() > MAX_BODY_BYTES {
            return;
        }
        let entry = CachedResponse {
            result: result.clone(),
            body_text: body_text.to_owned(),
        };
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
        if let Some(entry) = self.position(from).map(|ix| self.entries[ix].1.clone()) {
            self.insert(to, &entry.result, &entry.body_text);
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

    fn ok(status: u16) -> Result<Response, String> {
        Ok(Response::new(status, vec![], vec![], Duration::ZERO))
    }

    fn status(cache: &mut ResponseCache<u32>, key: u32) -> Option<u16> {
        cache
            .get(key)
            .and_then(|entry| entry.result.as_ref().ok())
            .map(|response| response.status)
    }

    #[test]
    fn keeps_the_last_response_per_key() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(200), "first");
        cache.insert(1, &ok(404), "second");
        cache.insert(2, &Err("timed out".into()), "");
        assert_eq!(status(&mut cache, 1), Some(404));
        assert_eq!(cache.get(1).unwrap().body_text, "second");
        assert_eq!(
            cache.get(2).unwrap().result.as_ref().unwrap_err(),
            "timed out"
        );
        assert!(cache.get(3).is_none());
    }

    #[test]
    fn evicts_the_least_recently_used() {
        let mut cache = ResponseCache::new();
        for key in 0..MAX_ENTRIES as u32 {
            cache.insert(key, &ok(200), "");
        }
        cache.get(0);
        cache.insert(100, &ok(200), "");
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(cache.get(0).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.get(100).is_some());
    }

    #[test]
    fn skips_large_bodies_and_drops_the_stale_entry() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(200), "small");
        cache.insert(1, &ok(200), &"x".repeat(MAX_BODY_BYTES + 1));
        assert!(cache.get(1).is_none());
        cache.insert(2, &ok(200), &"x".repeat(MAX_BODY_BYTES));
        assert!(cache.get(2).is_some());
    }

    #[test]
    fn rekey_replaces_the_target() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(201), "");
        cache.insert(2, &ok(500), "");
        cache.rekey(1, 2);
        assert!(cache.get(1).is_none());
        assert_eq!(status(&mut cache, 2), Some(201));
        cache.rekey(3, 2);
        assert!(cache.get(2).is_none());
    }

    #[test]
    fn copy_keeps_the_source() {
        let mut cache = ResponseCache::new();
        cache.insert(1, &ok(201), "body");
        cache.insert(2, &ok(500), "");
        cache.copy(1, 2);
        assert_eq!(status(&mut cache, 1), Some(201));
        assert_eq!(status(&mut cache, 2), Some(201));
        cache.copy(3, 2);
        assert_eq!(status(&mut cache, 2), Some(201));
    }

    #[test]
    fn retain_and_remove_drop_entries() {
        let mut cache = ResponseCache::new();
        for key in 0..4 {
            cache.insert(key, &ok(200), "");
        }
        cache.retain(|key| key % 2 == 0);
        cache.remove(2);
        assert!(cache.get(0).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_none());
        assert!(cache.get(3).is_none());
    }
}
