use gpui::{Context, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpui_component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    select::Select,
};

use super::{RequestEditor, RequestEditorEvent};
use crate::actions::{SaveRequest, SendRequest, WORKSPACE_CONTEXT};

impl RequestEditor {
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
                Button::new("save")
                    .outline()
                    .label("Save")
                    // The dot's slot is always there so the button keeps its width.
                    .child(
                        div()
                            .flex_none()
                            .size_1p5()
                            .rounded_full()
                            .when(self.dirty, |dot| dot.bg(cx.theme().primary)),
                    )
                    .when(self.dirty, |button| {
                        button.accessibility_label("Save, unsaved changes")
                    })
                    .disabled(!self.can_save())
                    .tooltip_with_action("Save request", &SaveRequest, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(RequestEditorEvent::Save))),
            )
            .child(
                Button::new("send")
                    .primary()
                    .label("Send")
                    .loading(self.loading)
                    .tooltip_with_action("Send request", &SendRequest, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
            )
    }
}
