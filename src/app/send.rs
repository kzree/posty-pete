use std::sync::Arc;

use chrono::Utc;
use gpui::{App, AppContext, Context, SharedString, Task, Window};
use tokio::task::{AbortHandle, JoinError};

use super::{EditorContent, Workspace, show_error};
use crate::{
    domain::{
        collection::Collection,
        curl,
        environment::Environment,
        environment_selection::EnvironmentSelection,
        history::{HistoryRecord, Outcome},
        http,
        layers::Layers,
        request::Request,
        response::Response,
        send_error::SendError,
    },
    settings::AppSettings,
    ui::clipboard::copy_with_notification,
};

/// A send in flight. Dropping it aborts the request on tokio and stops
/// `finish_request` from running.
pub(super) struct PendingSend {
    _task: Task<()>,
    abort: AbortHandle,
}

impl Drop for PendingSend {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

fn flatten_send_result(
    joined: Result<Result<Response, SendError>, JoinError>,
) -> Result<Response, SendError> {
    joined.unwrap_or_else(|e| Err(SendError::new(format!("task failed: {e}"))))
}

/// What the editor's request resolves against: a saved request's collection,
/// with that collection's active environment and its base; the draft's source
/// for the draft. `None` with nothing to resolve against.
fn editor_layers<'a>(
    content: EditorContent,
    selection: &EnvironmentSelection,
    collections: &'a [Arc<Collection>],
    shared: &'a [Arc<Environment>],
) -> Option<Layers<'a>> {
    match content {
        EditorContent::Saved { collection_id, .. } => collections
            .iter()
            .find(|collection| collection.id == collection_id)
            .map(|collection| selection.layers(collection, shared)),
        EditorContent::Draft => selection.draft_layers(collections, shared),
    }
}

impl Workspace {
    /// Requests in flight keep the client they started with.
    pub(super) fn request_settings_changed(&mut self, cx: &mut Context<Self>) {
        self.client.apply(&AppSettings::get(cx).requests);
    }

    /// The request the editor's `request` sends, with the variables and
    /// inherited auth as they are now.
    fn resolve_editor_request(&mut self, request: Request, cx: &mut Context<Self>) -> Request {
        self.flush_pending_variables(cx);
        request.resolved_with(self.editor_layers(cx).as_ref())
    }

    fn editor_layers<'a>(&self, cx: &'a App) -> Option<Layers<'a>> {
        let collections = self.collections.read(cx).all();
        let shared = self.shared_environments.read(cx).all();
        editor_layers(self.editor_content, &self.environments, collections, shared)
    }

    /// Hands the editor the variables `request` references but that don't
    /// resolve, including those in inherited auth. Without a collection to
    /// resolve against, all of them.
    pub(super) fn sync_unresolved_variables(&mut self, request: &Request, cx: &mut Context<Self>) {
        let layers = self.editor_layers(cx);
        let resolvable = layers.is_some();
        let names = request.unresolved_variables_with(layers.as_ref());
        self.request_editor.update(cx, |editor, cx| {
            editor.set_unresolved_variables(&names, resolvable, cx)
        });
    }

    pub(super) fn copy_editor_as_curl(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = self.resolve_editor_request(request, cx);
        copy_with_notification(curl::to_curl(&request), "Copied as cURL", window, cx);
    }

    pub(super) fn copy_history_as_curl(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = self.history.read(cx).request(id, cx);
        cx.spawn_in(window, async move |this, cx| {
            let request = request.await;
            this.update_in(cx, |this, window, cx| match request {
                Ok(Some(entry)) => {
                    let message = match entry.omitted_body_size {
                        Some(_) => "Copied as cURL, without the body",
                        None => "Copied as cURL",
                    };
                    copy_with_notification(curl::to_curl(&entry.request), message, window, cx)
                }
                Ok(None) => this
                    .history_panel
                    .update(cx, |panel, cx| panel.remove(id, window, cx)),
                Err(error) => show_error("Couldn't copy as cURL", &error, window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Sends `request` from the editor and records what was sent in history.
    pub(super) fn send_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let client = match self.client.get() {
            Ok(client) => client,
            Err(error) => return show_error("Couldn't send the request", error, window, cx),
        };
        let request = self.resolve_editor_request(request, cx);
        self.set_sending(true, cx);

        let content = self.editor_content;
        let sent_at = Utc::now();
        let handle = http::spawn_send(client, request.clone());
        let abort = handle.abort_handle();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = flatten_send_result(handle.await);
            let outcome = result.as_ref().ok().map(|r| Outcome {
                status: r.status,
                elapsed: r.elapsed,
            });
            let (result, body_text) = cx
                .background_spawn(async move {
                    let mut result = result;
                    let text = result
                        .as_mut()
                        .map(Response::take_display_body)
                        .unwrap_or_default();
                    (result, SharedString::from(text))
                })
                .await;
            let record = HistoryRecord {
                sent_at,
                request,
                outcome,
            };
            this.update_in(cx, |this, window, cx| {
                this.finish_request(content, result, body_text, record, window, cx)
            })
            .ok();
        });
        self.pending = Some(PendingSend { _task: task, abort });
    }

    /// Sends the editor's request, or cancels the one in flight.
    pub(super) fn send_or_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            self.cancel_request(window, cx);
        } else {
            self.request_editor.update(cx, |editor, cx| editor.send(cx));
        }
    }

    /// Aborts the request in flight. Nothing is recorded in history; the
    /// editor's content remembers the cancel instead of its last response.
    pub(super) fn cancel_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.take().is_none() {
            return;
        }
        self.set_sending(false, cx);
        self.responses.insert_cancelled(self.editor_content);
        self.response_view
            .update(cx, |view, cx| view.show_cancelled(window, cx));
    }

    /// `content` is what the editor held when the request was sent.
    fn finish_request(
        &mut self,
        content: EditorContent,
        result: Result<Response, SendError>,
        body_text: SharedString,
        record: HistoryRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        self.set_sending(false, cx);
        // A saved request deleted in flight became the draft.
        let content = if self.route_exists(content.route(), cx) {
            content
        } else {
            self.editor_content
        };
        self.responses.insert(content, &result, body_text.clone());
        if content == self.editor_content {
            self.response_view.update(cx, |view, cx| {
                view.set_response(result, body_text, window, cx)
            });
        }
        self.history
            .update(cx, |history, cx| history.record(record, cx));
    }

    /// `pending` is the source of truth; this mirrors it in the UI.
    fn set_sending(&mut self, sending: bool, cx: &mut Context<Self>) {
        self.request_editor
            .update(cx, |editor, cx| editor.set_loading(sending, cx));
        self.loading_bar
            .update(cx, |bar, cx| bar.set_active(sending, cx));
        // Which actions are registered depends on it.
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::domain::{
        collection::{add_environment, set_environment_base, set_environment_variables},
        request::{Auth, KeyValue},
        variables::Variables,
    };

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn collection(variables: Vec<KeyValue>) -> Arc<Collection> {
        let mut collection = Collection::new("API");
        collection.variables = variables;
        Arc::new(collection)
    }

    fn none() -> EnvironmentSelection {
        EnvironmentSelection::default()
    }

    /// `collection` with an environment overriding `base`, and its id.
    fn with_environment(collection: &Arc<Collection>) -> (Arc<Collection>, Uuid) {
        let mut collections = [collection.clone()];
        let id = add_environment(&mut collections, collection.id).unwrap();
        assert!(set_environment_variables(
            &mut collections,
            collection.id,
            id,
            vec![row(true, "base", "https://staging")],
        ));
        let [collection] = collections;
        (collection, id)
    }

    fn saved_in(collection_id: Uuid) -> EditorContent {
        EditorContent::Saved {
            collection_id,
            request_id: Uuid::new_v4(),
        }
    }

    fn editor_variables(
        content: EditorContent,
        selection: &EnvironmentSelection,
        collections: &[Arc<Collection>],
        shared: &[Arc<Environment>],
    ) -> Option<Variables> {
        editor_layers(content, selection, collections, shared)
            .as_ref()
            .map(Layers::variables)
    }

    fn templated() -> Request {
        Request {
            auth: Auth::None,
            url: "{{base}}/users/{{id}}".into(),
            headers: vec![row(true, "{{k}}", "v")],
            ..Request::default()
        }
    }

    #[test]
    fn saved_request_uses_its_collections_variables() {
        let other = collection(vec![row(true, "base", "https://other")]);
        let own = collection(vec![
            row(true, "base", "https://api"),
            row(false, "id", "7"),
        ]);
        let collections = [other, own.clone()];

        let layers = editor_layers(saved_in(own.id), &none(), &collections, &[]);
        let sent = templated().resolved_with(layers.as_ref());

        assert_eq!(sent.url, "https://api/users/{{id}}");
    }

    #[test]
    fn draft_and_missing_collection_are_sent_unchanged() {
        let collections = [collection(vec![row(true, "base", "https://api")])];
        for content in [EditorContent::Draft, saved_in(Uuid::new_v4())] {
            let layers = editor_layers(content, &none(), &collections, &[]);
            let variables = layers.as_ref().map(Layers::variables);
            assert_eq!(variables, None);
            assert_eq!(templated().resolved_with(layers.as_ref()), templated());
        }
    }

    #[test]
    fn key_resolving_to_empty_is_dropped_when_sent() {
        let collections = [collection(vec![row(true, "k", "")])];
        let layers = editor_layers(saved_in(collections[0].id), &none(), &collections, &[]);
        let sent = templated().resolved_with(layers.as_ref());

        assert_eq!(sent.headers, [row(true, "", "v")]);
        assert!(sent.resolve().headers.is_empty());
    }

    #[test]
    fn saved_request_uses_its_collections_active_environment() {
        let base = collection(vec![row(true, "base", "https://api")]);
        let (own, staging) = with_environment(&base);
        let collections = [own.clone()];
        let mut selection = none();
        selection.set_active_environment(own.id, Some(staging));

        let layers = editor_layers(saved_in(own.id), &selection, &collections, &[]);
        let variables = layers.as_ref().map(Layers::variables);
        assert_eq!(variables.unwrap().get("base"), Some("https://staging"));

        selection.set_active_environment(own.id, None);
        let layers = editor_layers(saved_in(own.id), &selection, &collections, &[]);
        let variables = layers.as_ref().map(Layers::variables);
        assert_eq!(variables.unwrap().get("base"), Some("https://api"));
    }

    #[test]
    fn draft_uses_the_draft_collection_and_its_environment() {
        let base = collection(vec![row(true, "base", "https://api")]);
        let (own, staging) = with_environment(&base);
        let collections = [collection(Vec::new()), own.clone()];
        let mut selection = none();
        selection.set_draft_collection(Some(own.id), Some(staging));

        let layers = editor_layers(EditorContent::Draft, &selection, &collections, &[]);
        let sent = templated().resolved_with(layers.as_ref());
        assert_eq!(sent.url, "https://staging/users/{{id}}");

        selection.set_draft_collection(None, None);
        assert_eq!(
            editor_variables(EditorContent::Draft, &selection, &collections, &[]),
            None
        );
    }

    #[test]
    fn deleted_draft_collection_means_no_variables() {
        let collections = [collection(vec![row(true, "base", "https://api")])];
        let mut selection = none();
        selection.set_draft_collection(Some(Uuid::new_v4()), None);
        assert_eq!(
            editor_variables(EditorContent::Draft, &selection, &collections, &[]),
            None
        );
    }

    fn shared(variables: Vec<KeyValue>) -> Arc<Environment> {
        let mut environment = Environment::new("Shared");
        environment.variables = variables;
        Arc::new(environment)
    }

    #[test]
    fn saved_request_uses_the_base_of_its_active_environment() {
        let base = collection(vec![row(true, "id", "1")]);
        let (own, staging) = with_environment(&base);
        let dev = shared(vec![row(true, "base", "https://dev"), row(true, "id", "2")]);
        let mut collections = [own.clone()];
        assert!(set_environment_base(
            &mut collections,
            own.id,
            staging,
            Some(dev.id)
        ));
        let mut selection = none();
        selection.set_active_environment(own.id, Some(staging));

        let variables =
            editor_variables(saved_in(own.id), &selection, &collections, &[dev]).unwrap();
        assert_eq!(variables.get("base"), Some("https://staging"));
        assert_eq!(variables.get("id"), Some("2"));
    }

    #[test]
    fn draft_with_a_shared_environment_uses_only_its_variables() {
        let collections = [collection(vec![row(true, "id", "1")])];
        let dev = shared(vec![row(true, "base", "https://dev")]);
        let mut selection = none();
        selection.set_draft_shared_environment(dev.id);

        let layers = editor_layers(
            EditorContent::Draft,
            &selection,
            &collections,
            std::slice::from_ref(&dev),
        );
        let sent = templated().resolved_with(layers.as_ref());
        assert_eq!(sent.url, "https://dev/users/{{id}}");

        assert_eq!(
            editor_variables(EditorContent::Draft, &selection, &collections, &[]),
            None
        );
    }

    #[test]
    fn inherit_sends_the_active_environments_auth_with_its_variables() {
        let mut own = Collection::new("API");
        own.variables = vec![row(true, "token", "from-collection")];
        own.auth = Auth::Bearer {
            token: "collection".into(),
        };
        let mut staging = Environment::new("Staging");
        staging.auth = Auth::Bearer {
            token: "{{token}}".into(),
        };
        let staging_id = staging.id;
        own.environments.push(staging);
        let collections = [Arc::new(own)];
        let id = collections[0].id;
        let mut selection = none();
        let inherit = Request::default();

        let layers = editor_layers(saved_in(id), &selection, &collections, &[]);
        assert_eq!(
            inherit.resolved_with(layers.as_ref()).auth,
            Auth::Bearer {
                token: "collection".into()
            }
        );

        selection.set_active_environment(id, Some(staging_id));
        let layers = editor_layers(saved_in(id), &selection, &collections, &[]);
        assert_eq!(
            inherit.resolved_with(layers.as_ref()).auth,
            Auth::Bearer {
                token: "from-collection".into()
            }
        );

        let layers = editor_layers(EditorContent::Draft, &selection, &collections, &[]);
        assert_eq!(inherit.resolved_with(layers.as_ref()).auth, Auth::None);
    }
}
