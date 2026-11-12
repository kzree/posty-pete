//! Resolving [`Auth::Inherit`]: a request's auth comes from the first layer
//! that sets it, in the order variables use.

use uuid::Uuid;

use super::{layers::Layers, request::Auth};

/// Where a request's effective auth comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthSource<'a> {
    /// The request's own, including an explicit "No auth".
    Request,
    /// The collection's active environment.
    Environment {
        collection_id: Uuid,
        id: Uuid,
        name: &'a str,
    },
    /// A shared environment: the base of the collection environment named
    /// `via`, or a shared draft source when `via` is `None`.
    SharedEnvironment {
        id: Uuid,
        name: &'a str,
        via: Option<&'a str>,
    },
    Collection {
        id: Uuid,
        name: &'a str,
    },
    /// No layer sets any auth.
    Nothing,
    /// Nothing to inherit from: a draft without a source, or a request whose
    /// collection is gone.
    NoSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveAuth<'a> {
    /// Never [`Auth::Inherit`].
    pub auth: &'a Auth,
    pub source: AuthSource<'a>,
}

static NO_AUTH: Auth = Auth::None;

/// The auth a request with `auth` sends, and where it comes from. Variables
/// in it aren't substituted yet.
pub fn effective_auth<'a>(auth: &'a Auth, layers: Option<&Layers<'a>>) -> EffectiveAuth<'a> {
    if !auth.is_inherit() {
        return EffectiveAuth {
            auth,
            source: AuthSource::Request,
        };
    }
    let found =
        |auth: &'a Auth, source| (!auth.is_inherit()).then_some(EffectiveAuth { auth, source });
    let inherited = match layers {
        None => {
            return EffectiveAuth {
                auth: &NO_AUTH,
                source: AuthSource::NoSource,
            };
        }
        Some(&Layers::Collection {
            collection,
            environment,
            base,
        }) => environment
            .and_then(|env| {
                found(
                    &env.auth,
                    AuthSource::Environment {
                        collection_id: collection.id,
                        id: env.id,
                        name: &env.name,
                    },
                )
            })
            .or_else(|| {
                base.and_then(|base| {
                    found(
                        &base.auth,
                        AuthSource::SharedEnvironment {
                            id: base.id,
                            name: &base.name,
                            via: environment.map(|env| env.name.as_str()),
                        },
                    )
                })
            })
            .or_else(|| {
                (!matches!(collection.auth, Auth::None | Auth::Inherit)).then_some(EffectiveAuth {
                    auth: &collection.auth,
                    source: AuthSource::Collection {
                        id: collection.id,
                        name: &collection.name,
                    },
                })
            }),
        Some(&Layers::Shared(env)) => found(
            &env.auth,
            AuthSource::SharedEnvironment {
                id: env.id,
                name: &env.name,
                via: None,
            },
        ),
    };
    inherited.unwrap_or(EffectiveAuth {
        auth: &NO_AUTH,
        source: AuthSource::Nothing,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::{
        collection::Collection,
        environment::Environment,
        request::{ApiKeyLocation, KeyValue, Request},
    };

    fn bearer(token: &str) -> Auth {
        Auth::Bearer {
            token: token.into(),
        }
    }

    fn row(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    struct Fixture {
        collection: Collection,
        staging: Uuid,
        shared: Vec<Arc<Environment>>,
    }

    /// Pets, with its environment Staging based on the shared Prod; every
    /// layer set to Inherit (the collection to None).
    fn fixture() -> Fixture {
        let prod = Environment::new("Prod");
        let mut staging = Environment::new("Staging");
        staging.base = Some(prod.id);
        let mut collection = Collection::new("Pets");
        let staging_id = staging.id;
        collection.environments.push(staging);
        Fixture {
            collection,
            staging: staging_id,
            shared: vec![Arc::new(prod)],
        }
    }

    impl Fixture {
        fn staging(&mut self) -> &mut Environment {
            let id = self.staging;
            self.collection
                .environments
                .iter_mut()
                .find(|env| env.id == id)
                .unwrap()
        }

        fn prod(&mut self) -> &mut Environment {
            Arc::make_mut(&mut self.shared[0])
        }

        fn layers(&self) -> Layers<'_> {
            Layers::collection(&self.collection, Some(self.staging), &self.shared)
        }
    }

    #[test]
    fn the_requests_own_auth_wins() {
        let mut f = fixture();
        f.staging().auth = bearer("env");
        for auth in [bearer("own"), Auth::None] {
            let effective = effective_auth(&auth, Some(&f.layers()));
            assert_eq!(effective.auth, &auth);
            assert_eq!(effective.source, AuthSource::Request);
            assert_eq!(effective_auth(&auth, None).source, AuthSource::Request);
        }
    }

    #[test]
    fn each_layer_wins_over_the_ones_after_it() {
        let mut f = fixture();
        f.collection.auth = bearer("collection");
        let collection_id = f.collection.id;
        let effective = effective_auth(&Auth::Inherit, Some(&f.layers()));
        assert_eq!(effective.auth, &bearer("collection"));
        assert_eq!(
            effective.source,
            AuthSource::Collection {
                id: collection_id,
                name: "Pets"
            }
        );

        f.prod().auth = bearer("prod");
        let prod_id = f.shared[0].id;
        let effective = effective_auth(&Auth::Inherit, Some(&f.layers()));
        assert_eq!(effective.auth, &bearer("prod"));
        assert_eq!(
            effective.source,
            AuthSource::SharedEnvironment {
                id: prod_id,
                name: "Prod",
                via: Some("Staging")
            }
        );

        f.staging().auth = Auth::None;
        let staging_id = f.staging;
        let effective = effective_auth(&Auth::Inherit, Some(&f.layers()));
        assert_eq!(effective.auth, &Auth::None);
        assert_eq!(
            effective.source,
            AuthSource::Environment {
                collection_id,
                id: staging_id,
                name: "Staging"
            }
        );
    }

    #[test]
    fn inherit_everywhere_means_nothing() {
        let mut f = fixture();
        let effective = effective_auth(&Auth::Inherit, Some(&f.layers()));
        assert_eq!(effective.auth, &Auth::None);
        assert_eq!(effective.source, AuthSource::Nothing);

        f.collection.auth = Auth::Inherit;
        let effective = effective_auth(&Auth::Inherit, Some(&f.layers()));
        assert_eq!(effective.auth, &Auth::None);
        assert_eq!(effective.source, AuthSource::Nothing);
    }

    #[test]
    fn no_active_environment_uses_the_collection() {
        let mut f = fixture();
        f.staging().auth = bearer("env");
        f.collection.auth = bearer("collection");
        let layers = Layers::collection(&f.collection, None, &f.shared);
        let effective = effective_auth(&Auth::Inherit, Some(&layers));
        assert_eq!(effective.auth, &bearer("collection"));
    }

    #[test]
    fn deleted_environment_and_base_are_skipped() {
        let mut f = fixture();
        f.staging().auth = bearer("env");
        f.collection.auth = bearer("collection");
        let layers = Layers::collection(&f.collection, Some(Uuid::new_v4()), &f.shared);
        assert_eq!(
            effective_auth(&Auth::Inherit, Some(&layers)).auth,
            &bearer("collection")
        );

        f.staging().auth = Auth::Inherit;
        f.prod().auth = bearer("prod");
        let layers = Layers::collection(&f.collection, Some(f.staging), &[]);
        assert_eq!(
            effective_auth(&Auth::Inherit, Some(&layers)).auth,
            &bearer("collection")
        );
    }

    #[test]
    fn shared_draft_uses_only_the_shared_environment() {
        let mut f = fixture();
        f.collection.auth = bearer("collection");
        let layers = Layers::Shared(&f.shared[0]);
        let effective = effective_auth(&Auth::Inherit, Some(&layers));
        assert_eq!(effective.auth, &Auth::None);
        assert_eq!(effective.source, AuthSource::Nothing);

        f.prod().auth = bearer("prod");
        let prod_id = f.shared[0].id;
        let layers = Layers::Shared(&f.shared[0]);
        let effective = effective_auth(&Auth::Inherit, Some(&layers));
        assert_eq!(effective.auth, &bearer("prod"));
        assert_eq!(
            effective.source,
            AuthSource::SharedEnvironment {
                id: prod_id,
                name: "Prod",
                via: None
            }
        );
    }

    #[test]
    fn no_source_inherits_nothing() {
        let effective = effective_auth(&Auth::Inherit, None);
        assert_eq!(effective.auth, &Auth::None);
        assert_eq!(effective.source, AuthSource::NoSource);

        let request = Request {
            url: "{{base}}".into(),
            ..Request::default()
        };
        let sent = request.resolved_with(None);
        assert_eq!(sent.auth, Auth::None);
        assert_eq!(sent.url, "{{base}}");
        assert_eq!(request.unresolved_variables_with(None), ["base"]);
    }

    #[test]
    fn inherited_values_use_the_requests_variables() {
        let mut f = fixture();
        f.collection.variables = vec![row("token", "from-collection"), row("key", "X-Key")];
        f.staging().variables = vec![row("token", "from-staging")];
        f.collection.auth = Auth::Bearer {
            token: "{{token}}".into(),
        };
        let request = Request::default();

        let sent = request.resolved_with(Some(&f.layers()));
        assert_eq!(sent.auth, bearer("from-staging"));
        assert!(
            request
                .unresolved_variables_with(Some(&f.layers()))
                .is_empty()
        );

        f.collection.auth = Auth::ApiKey {
            key: "{{key}}".into(),
            value: "{{missing}}".into(),
            location: ApiKeyLocation::Header,
        };
        assert_eq!(
            request.unresolved_variables_with(Some(&f.layers())),
            ["missing"]
        );
        let sent = request.resolved_with(Some(&f.layers()));
        assert_eq!(sent.resolve().headers, [("X-Key", "{{missing}}".into())]);
    }

    #[test]
    fn resolved_request_is_never_inherit() {
        let mut f = fixture();
        let request = Request::default();
        assert_eq!(request.resolved_with(Some(&f.layers())).auth, Auth::None);
        f.prod().auth = bearer("prod");
        assert_eq!(
            request.resolved_with(Some(&f.layers())).auth,
            bearer("prod")
        );
    }

    #[test]
    fn a_header_set_by_hand_wins_over_inherited_auth() {
        let mut f = fixture();
        let request = Request {
            headers: vec![row("Authorization", "Custom x")],
            ..Request::default()
        };

        f.collection.auth = bearer("t");
        let sent = request.resolved_with(Some(&f.layers()));
        assert_eq!(
            sent.resolve().headers,
            [("Authorization", "Custom x".into())]
        );
        assert!(
            request
                .unresolved_variables_with(Some(&f.layers()))
                .is_empty()
        );

        f.collection.auth = Auth::Basic {
            username: "{{user}}".into(),
            password: "p".into(),
        };
        let sent = request.resolved_with(Some(&f.layers()));
        assert_eq!(sent.resolve().basic_auth, None);
        assert!(
            request
                .unresolved_variables_with(Some(&f.layers()))
                .is_empty(),
            "auth that isn't sent doesn't need its variables"
        );
    }
}
