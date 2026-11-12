use domain::{
    collection::SavedRequest,
    request::{Body, Request},
};

/// The stored request the editor was loaded from, and the request the editor
/// built right after loading it. Inputs normalise what they're given (empty
/// key/value rows are dropped, single-line inputs drop newlines), so edits are
/// compared with the editor's own form of the stored request, not the stored
/// request itself.
pub(super) struct Baseline {
    stored: Request,
    /// `None` when the editor built `stored` unchanged.
    loaded: Option<Request>,
}

impl Baseline {
    /// Equal raw body text is shared, so the baseline holds it once.
    pub(super) fn new(stored: Request, mut loaded: Request) -> Self {
        if let (
            Body::Raw { text, .. },
            Body::Raw {
                text: stored_text, ..
            },
        ) = (&mut loaded.body, &stored.body)
            && text == stored_text
        {
            *text = stored_text.clone();
        }
        if loaded == stored {
            return Self::saved(stored);
        }
        Self {
            stored,
            loaded: Some(loaded),
        }
    }

    /// The baseline of a request the editor just saved.
    pub(super) fn saved(request: Request) -> Self {
        Self {
            stored: request,
            loaded: None,
        }
    }

    fn loaded(&self) -> &Request {
        self.loaded.as_ref().unwrap_or(&self.stored)
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
            self.loaded()
        } else {
            &saved.request
        };
        request != reference || name.is_some_and(|name| name != saved.name.trim())
    }
}

/// Coalesces the dirty checks a burst of edits asks for into one.
#[derive(Default)]
pub(super) struct PendingCheck(bool);

impl PendingCheck {
    /// Asks for a check. Returns `true` if none was pending, so one needs scheduling.
    pub(super) fn request(&mut self) -> bool {
        !std::mem::replace(&mut self.0, true)
    }

    pub(super) fn is_pending(&self) -> bool {
        self.0
    }

    pub(super) fn clear(&mut self) {
        self.0 = false;
    }
}

#[cfg(test)]
mod tests {
    use domain::request::{Auth, KeyValue, RawKind, RawText};

    use super::*;

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

    #[test]
    fn body_text_is_compared_by_content_not_storage() {
        let baseline = Baseline::new(stored(), loaded());
        let rebuilt = Request {
            body: Body::Raw {
                kind: RawKind::Json,
                text: String::from("{}").into(),
            },
            ..loaded()
        };
        assert!(!baseline.is_dirty(&saved(), &rebuilt, Some("Get users")));

        let edited = Request {
            body: Body::Raw {
                kind: RawKind::Json,
                text: "{ }".into(),
            },
            ..loaded()
        };
        assert!(baseline.is_dirty(&saved(), &edited, Some("Get users")));
    }

    fn large_body() -> Body {
        Body::Raw {
            kind: RawKind::Json,
            text: format!("\"{}\"", "x".repeat(1024 * 1024)).into(),
        }
    }

    fn raw_text(body: &Body) -> &RawText {
        match body {
            Body::Raw { text, .. } => text,
            _ => panic!("not a raw body"),
        }
    }

    #[test]
    fn loading_shares_the_body_with_the_editor_snapshot() {
        let stored = Request {
            body: large_body(),
            ..stored()
        };
        // The editor's snapshot of a loaded raw body shares the stored text.
        let snapshot = stored.body.clone();
        let loaded = Request {
            params: vec![row("q", "1")],
            body: snapshot.clone(),
            ..stored.clone()
        };
        let baseline = Baseline::new(stored.clone(), loaded.clone());
        assert!(RawText::ptr_eq(
            raw_text(&baseline.stored.body),
            raw_text(&snapshot)
        ));
        assert!(RawText::ptr_eq(
            raw_text(&baseline.loaded().body),
            raw_text(&snapshot)
        ));
        let saved = SavedRequest::new("Get users", stored);
        assert!(!baseline.is_dirty(&saved, &loaded, Some("Get users")));
    }

    #[test]
    fn equal_body_text_is_held_once() {
        let stored = Request {
            body: large_body(),
            ..stored()
        };
        let loaded = Request {
            params: vec![row("q", "1")],
            body: Body::Raw {
                kind: RawKind::Json,
                text: raw_text(&stored.body).to_string().into(),
            },
            ..stored.clone()
        };
        let baseline = Baseline::new(stored.clone(), loaded.clone());
        assert!(RawText::ptr_eq(
            raw_text(&baseline.loaded().body),
            raw_text(&baseline.stored.body)
        ));

        let unchanged = Request {
            body: loaded.body.clone(),
            ..stored.clone()
        };
        let baseline = Baseline::new(stored, unchanged);
        assert!(baseline.loaded.is_none());
    }

    #[test]
    fn saving_shares_the_body_with_the_editor_snapshot() {
        let request = Request {
            body: large_body(),
            ..loaded()
        };
        let baseline = Baseline::saved(request.clone());
        assert!(baseline.loaded.is_none());
        assert!(RawText::ptr_eq(
            raw_text(&baseline.loaded().body),
            raw_text(&request.body)
        ));
        let saved = SavedRequest::new("Get users", request.clone());
        assert!(!baseline.is_dirty(&saved, &request, Some("Get users")));
    }

    #[test]
    fn a_burst_of_requests_schedules_one_check() {
        let mut check = PendingCheck::default();
        let scheduled = (0..100).filter(|_| check.request()).count();
        assert_eq!(scheduled, 1);
        assert!(check.is_pending());

        check.clear();
        assert!(!check.is_pending());
        assert!(check.request());
    }

    mod views {
        use std::{cell::Cell, rc::Rc};

        use gpui::{TestAppContext, VisualTestContext};

        use super::*;
        use crate::{
            request_editor::{RequestEditor, RequestEditorEvent},
            test_support::{open_view, type_into},
        };

        #[gpui::test]
        fn the_dirty_state_follows_edits_and_their_undoing(cx: &mut TestAppContext) {
            let (editor, cx) = open_view(cx, RequestEditor::new);
            let saved = saved();
            editor.update_in(cx, |editor, window, cx| {
                editor.load_request(&saved.request, Some(saved.name.clone()), window, cx)
            });
            let build = |cx: &mut VisualTestContext| {
                editor.read_with(cx, |editor, cx| (editor.build_request(cx), editor.name(cx)))
            };
            let baseline = Baseline::new(saved.request.clone(), build(cx).0);
            let edits = Rc::new(Cell::new(0));
            let counted = edits.clone();
            cx.update(|_, cx| {
                cx.subscribe(&editor, move |_, event, _| {
                    if let RequestEditorEvent::Edited = event {
                        counted.set(counted.get() + 1);
                    }
                })
                .detach()
            });
            let dirty = |cx: &mut VisualTestContext| {
                let (request, name) = build(cx);
                baseline.is_dirty(&saved, &request, name.as_deref())
            };
            assert!(!dirty(cx));

            let (name, url, headers) = editor.read_with(cx, |editor, _| {
                (
                    editor.name_input().clone(),
                    editor.url_input().clone(),
                    editor.headers_editor().clone(),
                )
            });
            type_into(&url, "example.com/users", cx);
            assert!(dirty(cx));
            type_into(&url, "example.com", cx);
            assert!(!dirty(cx));

            type_into(&name, "List users", cx);
            assert!(dirty(cx));
            type_into(&name, " Get users ", cx);
            assert!(!dirty(cx));

            let (key, _) = headers.read_with(cx, |headers, _| headers.row_inputs(0));
            type_into(&key, "Accept", cx);
            assert!(dirty(cx));
            type_into(&key, "", cx);
            assert!(!dirty(cx));

            assert_eq!(edits.get(), 6);
        }
    }
}
