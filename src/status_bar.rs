use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
};

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
            .flex_none()
            .h_6()
            .px_2()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .child("Status bar")
            .child(div().flex_1())
            .child(
                Button::new("settings")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings)
                    .tooltip("Settings")
                    .on_click(|_, _, _| {
                        println!("open settings");
                    }),
            )
    }
}
