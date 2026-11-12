//! What a request resolves against: the layers its variables and inherited
//! auth come from, first one winning.

use std::sync::Arc;

use uuid::Uuid;

use super::{
    auth::effective_auth,
    collection::Collection,
    environment::{self, Environment},
    request::{Auth, KeyValue, Request},
    variables::{Lookup, Variables, first_value},
};

#[derive(Clone, Copy, Debug)]
pub enum Layers<'a> {
    /// The environment, then the shared environment it's based on, then the
    /// collection.
    Collection {
        collection: &'a Collection,
        environment: Option<&'a Environment>,
        base: Option<&'a Environment>,
    },
    /// A shared environment alone, which a draft can resolve against.
    Shared(&'a Environment),
}

impl<'a> Layers<'a> {
    /// An unknown environment or base is left out.
    pub fn collection(
        collection: &'a Collection,
        environment_id: Option<Uuid>,
        shared: &'a [Arc<Environment>],
    ) -> Self {
        let environment = environment_id.and_then(|id| collection.environment(id));
        let base = environment
            .and_then(|env| env.base)
            .and_then(|id| environment::find(shared, id));
        Self::Collection {
            collection,
            environment,
            base,
        }
    }

    /// The variable rows of each layer, first one winning.
    fn sources(&self) -> impl Iterator<Item = &'a [KeyValue]> + use<'a> {
        let layers = match *self {
            Self::Collection {
                collection,
                environment,
                base,
            } => [
                environment.map(|env| env.variables.as_slice()),
                base.map(|env| env.variables.as_slice()),
                Some(collection.variables.as_slice()),
            ],
            Self::Shared(environment) => [Some(environment.variables.as_slice()), None, None],
        };
        layers.into_iter().flatten()
    }

    pub fn variables(&self) -> Variables<'a> {
        Variables::layered(self.sources())
    }
}

/// Looks names up layer by layer, without building a map.
impl Lookup for Layers<'_> {
    fn value(&self, name: &str) -> Option<&str> {
        first_value(self.sources(), name)
    }
}

impl Request {
    /// What gets sent: Inherit replaced by the effective auth, then variables
    /// substituted. Without layers nothing is inherited or substituted.
    pub fn resolved_with(mut self, layers: Option<&Layers>) -> Request {
        if self.auth.is_inherit() {
            self.auth = effective_auth(&Auth::Inherit, layers).auth.clone();
        }
        match layers {
            Some(layers) => self.with_variables_substituted(&layers.variables()),
            None => self,
        }
    }

    /// The names that don't resolve in what [`Request::resolved_with`] sends,
    /// in first-seen order; without layers, every name.
    pub fn unresolved_variables_with(&self, layers: Option<&Layers>) -> Vec<String> {
        let auth = effective_auth(&self.auth, layers).auth;
        self.unresolved_variables_with_auth(auth, &layers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Body, RawKind, RawText};

    fn row(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    fn bearer(token: &str) -> Auth {
        Auth::Bearer {
            token: token.into(),
        }
    }

    fn raw(text: &str) -> Body {
        Body::Raw {
            kind: RawKind::Json,
            text: text.into(),
        }
    }

    fn templated() -> Request {
        Request {
            url: "{{base}}/items".into(),
            params: vec![row("page", "{{page}}")],
            headers: vec![row("X-Env", "{{name}}")],
            body: raw(r#"{"id": "{{id}}"}"#),
            ..Request::default()
        }
    }

    #[test]
    fn each_layer_combination_resolves_the_same() {
        let mut collection = Collection::new("API");
        collection.variables = vec![
            row("base", "https://collection"),
            row("page", "1"),
            row("name", "collection"),
            row("id", "c"),
        ];
        collection.auth = bearer("{{name}}");
        let mut shared = Environment::new("Shared");
        shared.variables = vec![row("base", "https://shared"), row("name", "shared")];
        let mut staging = Environment::new("Staging");
        staging.variables = vec![row("name", "staging")];

        let sent = |layers: Option<Layers>| templated().resolved_with(layers.as_ref());
        let expected = |url: &str, page: &str, name: &str, body: &str, auth: Auth| Request {
            url: url.into(),
            params: vec![row("page", page)],
            headers: vec![row("X-Env", name)],
            body: raw(body),
            auth,
            ..Request::default()
        };

        assert_eq!(
            sent(None),
            Request {
                auth: Auth::None,
                ..templated()
            }
        );
        assert_eq!(
            sent(Some(Layers::Shared(&shared))),
            expected(
                "https://shared/items",
                "{{page}}",
                "shared",
                r#"{"id": "{{id}}"}"#,
                Auth::None
            )
        );
        let in_collection = |environment, base| Layers::Collection {
            collection: &collection,
            environment,
            base,
        };
        assert_eq!(
            sent(Some(in_collection(None, None))),
            expected(
                "https://collection/items",
                "1",
                "collection",
                r#"{"id": "c"}"#,
                bearer("collection")
            )
        );
        assert_eq!(
            sent(Some(in_collection(Some(&staging), None))),
            expected(
                "https://collection/items",
                "1",
                "staging",
                r#"{"id": "c"}"#,
                bearer("staging")
            )
        );
        assert_eq!(
            sent(Some(in_collection(Some(&staging), Some(&shared)))),
            expected(
                "https://shared/items",
                "1",
                "staging",
                r#"{"id": "c"}"#,
                bearer("staging")
            )
        );

        let own = Request {
            auth: bearer("{{name}}"),
            ..templated()
        };
        assert_eq!(own.clone().resolved_with(None), own);
        assert_eq!(
            own.resolved_with(Some(&in_collection(Some(&staging), None)))
                .auth,
            bearer("staging")
        );
    }

    #[test]
    fn text_without_references_is_kept_not_copied() {
        let mut environment = Environment::new("Env");
        environment.variables = vec![row("id", "7")];
        let text = RawText::from(r#"{"a": 1}"#);
        for layers in [None, Some(Layers::Shared(&environment))] {
            let request = Request {
                url: "https://a.test".into(),
                headers: vec![row("X-A", "1")],
                body: Body::Raw {
                    kind: RawKind::Json,
                    text: text.clone(),
                },
                auth: bearer("t"),
                ..Request::default()
            };
            let url = request.url.as_ptr();
            let header = request.headers[0].value.as_ptr();

            let sent = request.resolved_with(layers.as_ref());
            assert_eq!(sent.url.as_ptr(), url);
            assert_eq!(sent.headers[0].value.as_ptr(), header);
            let Body::Raw {
                text: sent_text, ..
            } = &sent.body
            else {
                panic!("not a raw body");
            };
            assert!(RawText::ptr_eq(sent_text, &text));
        }
    }

    fn with(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            ..row(key, value)
        }
    }

    #[test]
    fn lookup_matches_the_merged_variables() {
        let mut collection = Collection::new("API");
        collection.variables = vec![
            row("a", "collection"),
            row("b", "collection"),
            row("c", "collection"),
            row(" d ", "collection"),
            with(false, "e", "disabled"),
        ];
        let mut base = Environment::new("Shared");
        base.variables = vec![
            row("b", "base"),
            with(false, "c", "disabled"),
            row("  ", "blank"),
        ];
        let mut environment = Environment::new("Staging");
        environment.variables = vec![
            with(false, "a", "disabled"),
            row(" a", "environment"),
            row("a", "second"),
        ];
        let layers = Layers::Collection {
            collection: &collection,
            environment: Some(&environment),
            base: Some(&base),
        };
        let variables = layers.variables();
        for name in ["a", "b", "c", "d", "e", " d ", "", "missing"] {
            assert_eq!(layers.value(name), variables.get(name), "{name:?}");
        }
        assert_eq!(layers.value("a"), Some("environment"));
        assert_eq!(layers.value("b"), Some("base"));
        assert_eq!(layers.value("c"), Some("collection"));
        assert_eq!(layers.value("d"), Some("collection"));
        assert_eq!(layers.value("e"), None);
        assert_eq!(Layers::Shared(&base).value("b"), Some("base"));
        assert_eq!(Layers::Shared(&base).value("a"), None);
    }

    #[test]
    fn unresolved_names_keep_first_seen_order_across_layers() {
        let mut collection = Collection::new("API");
        collection.variables = vec![row(" known ", "k"), with(false, "off", "x")];
        let mut environment = Environment::new("Staging");
        environment.variables = vec![with(false, "known", "x"), row("env", "e")];
        let request = Request {
            url: "{{z}}/{{known}}/{{off}}".into(),
            headers: vec![row("X-A", "{{env}} {{a}} {{z}}")],
            body: raw("{{off}} {{b}} {{a}}"),
            auth: bearer("{{c}} {{env}}"),
            ..Request::default()
        };
        let layers = Layers::Collection {
            collection: &collection,
            environment: Some(&environment),
            base: None,
        };
        assert_eq!(
            request.unresolved_variables_with(Some(&layers)),
            ["z", "off", "a", "b", "c"]
        );
        assert_eq!(
            request.unresolved_variables_with(Some(&layers)),
            request.unresolved_variables(&layers.variables())
        );
        assert_eq!(
            request.unresolved_variables_with(None),
            ["z", "known", "off", "env", "a", "b", "c"]
        );
    }
}
