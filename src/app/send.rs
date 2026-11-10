use chrono::Utc;
use gpui::{AppContext, Context, Window};
use tokio::task::JoinError;

use super::{EditorContent, Workspace, show_error};
use crate::{
    domain::{
        curl,
        history::{HistoryRecord, Outcome},
        http,
        request::Request,
        response::Response,
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

impl Workspace {
    pub(super) fn copy_as_curl(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn send_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
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
                    (result, text)
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
        body_text: String,
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
        self.responses.insert(content, &result, &body_text);
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
