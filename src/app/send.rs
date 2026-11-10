use std::sync::Arc;

use chrono::Utc;
use gpui::{AppContext, Context, SharedString, Window};
use tokio::task::JoinError;

use super::{EditorContent, Workspace, show_error};
use crate::{
    domain::{
        collection::Collection,
        curl,
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

/// The variables that apply to what the editor holds: its collection's for a
/// saved request, none for the draft.
fn editor_variables(content: EditorContent, collections: &[Arc<Collection>]) -> Option<Variables> {
    let EditorContent::Saved { collection_id, .. } = content else {
        return None;
    };
    collections
        .iter()
        .find(|collection| collection.id == collection_id)
        .map(|collection| Variables::from_rows(&collection.variables))
}

fn resolve(request: Request, variables: Option<&Variables>) -> Request {
    match variables {
        Some(variables) => request.with_variables(variables).0,
        None => request,
    }
}

impl Workspace {
    /// The request the editor's `request` sends, with the variables as they are now.
    fn resolve_editor_request(&mut self, request: Request, cx: &mut Context<Self>) -> Request {
        self.flush_collection_variables(cx);
        let variables = editor_variables(self.editor_content, self.collections.read(cx).all());
        resolve(request, variables.as_ref())
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
    use crate::domain::request::KeyValue;

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

        let variables = editor_variables(saved_in(own.id), &collections);
        let sent = resolve(templated(), variables.as_ref());

        assert_eq!(sent.url, "https://api/users/{{id}}");
    }

    #[test]
    fn draft_and_missing_collection_are_sent_unchanged() {
        let collections = [collection(vec![row(true, "base", "https://api")])];
        for content in [EditorContent::Draft, saved_in(Uuid::new_v4())] {
            let variables = editor_variables(content, &collections);
            assert_eq!(variables, None);
            assert_eq!(resolve(templated(), variables.as_ref()), templated());
        }
    }

    #[test]
    fn key_resolving_to_empty_is_dropped_when_sent() {
        let collections = [collection(vec![row(true, "k", "")])];
        let variables = editor_variables(saved_in(collections[0].id), &collections);
        let sent = resolve(templated(), variables.as_ref());

        assert_eq!(sent.headers, [row(true, "", "v")]);
        assert!(sent.resolve().headers.is_empty());
    }
}
