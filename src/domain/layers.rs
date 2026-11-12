//! What a request resolves against: the layers its variables and inherited
//! auth come from, first one winning.

use std::sync::Arc;

use uuid::Uuid;

use super::{
    auth::effective_auth,
    collection::Collection,
    environment::{self, Environment},
    request::Request,
    variables::Variables,
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

    pub fn variables(&self) -> Variables {
        match *self {
            Self::Collection {
                collection,
                environment,
                base,
            } => Variables::layered(
                environment
                    .into_iter()
                    .chain(base)
                    .map(|env| env.variables.as_slice())
                    .chain([collection.variables.as_slice()]),
            ),
            Self::Shared(environment) => Variables::layered([environment.variables.as_slice()]),
        }
    }
}

impl Request {
    /// What gets sent: Inherit replaced by the effective auth, then variables
    /// substituted. Without layers nothing is inherited or substituted.
    pub fn resolved_with(&self, layers: Option<&Layers>) -> Request {
        let auth = effective_auth(&self.auth, layers).auth;
        match layers {
            Some(layers) => self.with_auth_and_variables(auth, &layers.variables()),
            None => Request {
                auth: auth.clone(),
                ..self.clone()
            },
        }
    }

    /// The names that don't resolve in what [`Request::resolved_with`] sends,
    /// in first-seen order; without layers, every name.
    pub fn unresolved_variables_with(&self, layers: Option<&Layers>) -> Vec<String> {
        let auth = effective_auth(&self.auth, layers).auth;
        let variables = layers.map(Layers::variables).unwrap_or_default();
        self.unresolved_variables_with_auth(auth, &variables)
    }
}
