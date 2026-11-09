use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use gpui_component::{ActiveTheme, scroll::ScrollableElement, v_flex};

use crate::domain::http::Response;

#[derive(IntoElement)]
pub struct ResponseView {
    last_response: Option<Result<Response, String>>,
}

impl ResponseView {
    pub fn new(last_response: Option<Result<Response, String>>) -> Self {
        Self { last_response }
    }
}

impl RenderOnce for ResponseView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .size_full()
            .overflow_y_scrollbar()
            .p_4()
            .child(match self.last_response {
                None => div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Send a request to see the response.")
                    .into_any_element(),
                Some(Err(error)) => div()
                    .text_color(cx.theme().danger)
                    .child(format!("Request failed: {error}"))
                    .into_any_element(),
                Some(Ok(response)) => v_flex()
                    .gap_2()
                    .child(format!(
                        "{} · {} ms",
                        response.status,
                        response.elapsed.as_millis()
                    ))
                    .child(response.body)
                    .into_any_element(),
            })
    }
}
