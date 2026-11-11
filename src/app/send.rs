use std::sync::Arc;

use chrono::Utc;
use gpui::{App, AppContext, Context, SharedString, Window};
use tokio::task::JoinError;

use super::{EditorContent, Workspace, show_error};
use crate::{
    domain::{
        collection::Collection,
        curl,
        environment::Environment,
        environment_selection::EnvironmentSelection,
        history::{HistoryRecord, Outcome},
        http,
        request::Request,
        response::Response,
        variables::Variables,
    },
    ui::clipboard::copy_with_notification,
};

fn flatten_send_result(
    joined: Result<anyhow::Result<Response>, JoinError>,
) -> Result<Response, String> {
    match joined {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(format!("task failed: {e}")),
    }
}

/// The collection the editor's request resolves against: a saved request's
/// own, the draft collection for the draft.
fn editor_collection<'a>(
    content: EditorContent,
    selection: &EnvironmentSelection,
    collections: &'a [Arc<Collection>],
) -> Option<&'a Arc<Collection>> {
    match content {
        EditorContent::Saved { collection_id, .. } => collections
            .iter()
            .find(|collection| collection.id == collection_id),
        EditorContent::Draft => selection.draft_collection(collections),
    }
}

/// The variables that apply to what the editor holds: its collection's, with
/// that collection's active environment and its base. `None` without a collection.
fn editor_variables(
    content: EditorContent,
    selection: &EnvironmentSelection,
    collections: &[Arc<Collection>],
    shared: &[Arc<Environment>],
) -> Option<Variables> {
    editor_collection(content, selection, collections).map(|c| selection.variables(c, shared))
}

fn resolve(request: Request, variables: Option<&Variables>) -> Request {
    match variables {
        Some(variables) => request.with_variables(variables),
        None => request,
    }
}

impl Workspace {
    /// The request the editor's `request` sends, with the variables as they are now.
    fn resolve_editor_request(&mut self, request: Request, cx: &mut Context<Self>) -> Request {
        self.flush_collection_variables(cx);
        let variables = self.editor_variables(cx);
        resolve(request, variables.as_ref())
    }

    fn editor_variables(&self, cx: &App) -> Option<Variables> {
        let collections = self.collections.read(cx).all();
        let shared = self.shared_environments.read(cx).all();
        editor_variables(self.editor_content, &self.environments, collections, shared)
    }

    /// Hands the editor the variables `request` references but that don't
    /// resolve. Without a collection to resolve against, all of them.
    pub(super) fn sync_unresolved_variables(&mut self, request: &Request, cx: &mut Context<Self>) {
        let variables = self.editor_variables(cx);
        let resolvable = variables.is_some();
        let names = request.unresolved_variables(&variables.unwrap_or_default());
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
                Ok(Some(request)) => {
                    copy_with_notification(curl::to_curl(&request), "Copied as cURL", window, cx)
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
        let request = self.resolve_editor_request(request, cx);
        self.set_sending(true, cx);

        let content = self.editor_content;
        let sent_at = Utc::now();
        let handle = http::spawn_send(self.client.clone(), request.clone());
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
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
        }));
    }

    /// `content` is what the editor held when the request was sent.
    fn finish_request(
        &mut self,
        content: EditorContent,
        result: Result<Response, String>,
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
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::domain::{
        collection::{add_environment, set_environment_variables},
        request::KeyValue,
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

    fn templated() -> Request {
        Request {
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

        let variables = editor_variables(saved_in(own.id), &none(), &collections, &[]);
        let sent = resolve(templated(), variables.as_ref());

        assert_eq!(sent.url, "https://api/users/{{id}}");
    }

    #[test]
    fn draft_and_missing_collection_are_sent_unchanged() {
        let collections = [collection(vec![row(true, "base", "https://api")])];
        for content in [EditorContent::Draft, saved_in(Uuid::new_v4())] {
            let variables = editor_variables(content, &none(), &collections, &[]);
            assert_eq!(variables, None);
            assert_eq!(resolve(templated(), variables.as_ref()), templated());
        }
    }

    #[test]
    fn key_resolving_to_empty_is_dropped_when_sent() {
        let collections = [collection(vec![row(true, "k", "")])];
        let variables = editor_variables(saved_in(collections[0].id), &none(), &collections, &[]);
        let sent = resolve(templated(), variables.as_ref());

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

        let variables = editor_variables(saved_in(own.id), &selection, &collections, &[]);
        assert_eq!(variables.unwrap().get("base"), Some("https://staging"));

        selection.set_active_environment(own.id, None);
        let variables = editor_variables(saved_in(own.id), &selection, &collections, &[]);
        assert_eq!(variables.unwrap().get("base"), Some("https://api"));
    }

    #[test]
    fn draft_uses_the_draft_collection_and_its_environment() {
        let base = collection(vec![row(true, "base", "https://api")]);
        let (own, staging) = with_environment(&base);
        let collections = [collection(Vec::new()), own.clone()];
        let mut selection = none();
        selection.set_draft_collection(Some(own.id), Some(staging));

        let variables = editor_variables(EditorContent::Draft, &selection, &collections, &[]);
        let sent = resolve(templated(), variables.as_ref());
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
}
