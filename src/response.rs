use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

#[derive(IntoElement)]
pub struct Response;

impl Response {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for Response {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div().size_full().child("Response")
    }
}
