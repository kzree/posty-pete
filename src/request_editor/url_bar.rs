use gpui::{Context, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    select::Select,
};

use super::RequestEditor;

impl RequestEditor {
    /// Method dropdown, URL input and Send button.
    pub(super) fn render_url_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .w(px(110.))
                    .child(Select::new(&self.method)),
            )
            .child(div().flex_1().child(Input::new(&self.url)))
            .child(
                Button::new("send")
                    .primary()
                    .label("Send")
                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
            )
    }
}
