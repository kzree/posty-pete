use crate::domain::{collection::SavedRequest, request::Request};

/// The stored request the editor was loaded from, and the request the editor
/// built right after loading it. Inputs normalise what they're given (empty
/// key/value rows are dropped, single-line inputs drop newlines), so edits are
/// compared with the editor's own form of the stored request, not the stored
/// request itself.
pub(super) struct Baseline {
    stored: Request,
    loaded: Request,
}

impl Baseline {
    pub(super) fn new(stored: Request, loaded: Request) -> Self {
        Self { stored, loaded }
    }

    /// Whether saving `request` and `name` (the editor's trimmed name) would
    /// change `saved`. An empty name counts as unchanged, since saving keeps
    /// the stored one.
    pub(super) fn is_dirty(
        &self,
        saved: &SavedRequest,
        request: &Request,
        name: Option<&str>,
    ) -> bool {
        // If the stored request changed since loading, the baseline is stale.
        let reference = if saved.request == self.stored {
            &self.loaded
        } else {
            &saved.request
        };
        request != reference || name.is_some_and(|name| name != saved.name.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{Auth, Body, KeyValue, RawKind};

    fn row(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    fn stored() -> Request {
        Request {
            url: "example.com".into(),
            params: vec![row("q", "1"), row("", "")],
            body: Body::Raw {
                kind: RawKind::Json,
                text: "{}".into(),
            },
            auth: Auth::Bearer { token: "t".into() },
            ..Default::default()
        }
    }

    /// What the editor builds from `stored()`: the empty row is gone.
    fn loaded() -> Request {
        Request {
            params: vec![row("q", "1")],
            ..stored()
        }
    }

    fn saved() -> SavedRequest {
        SavedRequest::new("Get users", stored())
    }

    #[test]
    fn freshly_loaded_request_is_clean() {
        let baseline = Baseline::new(stored(), loaded());
        assert!(!baseline.is_dirty(&saved(), &loaded(), Some("Get users")));
    }

    #[test]
    fn edits_make_it_dirty_and_undoing_them_cleans_it() {
        let baseline = Baseline::new(stored(), loaded());
        let edited = Request {
            url: "example.com/users".into(),
            ..loaded()
        };
        assert!(baseline.is_dirty(&saved(), &edited, Some("Get users")));

        let auth = Request {
            auth: Auth::Bearer { token: "u".into() },
            ..loaded()
        };
        assert!(baseline.is_dirty(&saved(), &auth, Some("Get users")));

        assert!(!baseline.is_dirty(&saved(), &loaded(), Some("Get users")));
    }

    #[test]
    fn renaming_makes_it_dirty_but_clearing_the_name_doesnt() {
        let baseline = Baseline::new(stored(), loaded());
        assert!(baseline.is_dirty(&saved(), &loaded(), Some("List users")));
        assert!(!baseline.is_dirty(&saved(), &loaded(), None));
    }

    #[test]
    fn stored_name_is_compared_trimmed() {
        let baseline = Baseline::new(stored(), loaded());
        let saved = SavedRequest::new(" Get users ", stored());
        assert!(!baseline.is_dirty(&saved, &loaded(), Some("Get users")));
    }

    #[test]
    fn stale_baseline_falls_back_to_the_stored_request() {
        let baseline = Baseline::new(stored(), loaded());
        let changed = SavedRequest::new("Get users", Request::default());
        assert!(baseline.is_dirty(&changed, &loaded(), Some("Get users")));
        assert!(!baseline.is_dirty(&changed, &Request::default(), Some("Get users")));
    }

    #[test]
    fn saved_request_is_its_own_baseline() {
        let edited = Request {
            url: "example.com/users".into(),
            ..loaded()
        };
        let baseline = Baseline::new(edited.clone(), edited.clone());
        let saved = SavedRequest::new("Get users", edited.clone());
        assert!(!baseline.is_dirty(&saved, &edited, Some("Get users")));
    }
}
