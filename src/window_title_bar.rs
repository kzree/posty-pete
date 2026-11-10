use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};
use gpui_component::{ActiveTheme, TitleBar, h_flex};

#[derive(IntoElement)]
pub struct WindowTitleBar {
    title: SharedString,
}

impl WindowTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        TitleBar::new().child(
            div()
                .relative()
                .flex_1()
                .h_full()
                .min_w_0()
                .child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .justify_center()
                        .px_24()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.title),
                        ),
                )
                .child(h_flex().h_full().gap_1()),
        )
    }
}
