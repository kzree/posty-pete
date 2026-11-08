use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use gpui_component::ActiveTheme;

/// The bar along the bottom of the window.
#[derive(IntoElement)]
pub struct StatusBar;

impl StatusBar {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .h_6()
            .px_2()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .child("Status bar")
    }
}
