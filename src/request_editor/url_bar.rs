use gpui::{Context, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    select::Select,
};

use super::{RequestEditor, RequestEditorEvent};
use crate::{
    actions::{SaveRequest, SendRequest, WORKSPACE_CONTEXT},
    ui::unresolved_hint::UnresolvedHint,
};

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
                div()
                    .relative()
                    .flex_none()
                    .child(
                        Button::new("save")
                            .outline()
                            .label("Save")
                            .when(self.dirty, |button| {
                                button.accessibility_label("Save, unsaved changes")
                            })
                            .tooltip_with_action(
                                "Save request",
                                &SaveRequest,
                                Some(WORKSPACE_CONTEXT),
                            )
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(RequestEditorEvent::Save))),
                    )
                    // Overlaid so the label stays centred.
                    .when(self.dirty, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .right(px(4.))
                                .size_1p5()
                                .rounded_full()
                                .bg(cx.theme().primary),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(80.))
                    .child(self.send_button(cx).w_full()),
            )
    }

    /// Becomes Cancel while sending; the fixed-width slot keeps the layout still.
    fn send_button(&self, cx: &mut Context<Self>) -> Button {
        if self.loading {
            return Button::new("cancel")
                .label("Cancel")
                .tooltip_with_action("Cancel request", &SendRequest, Some(WORKSPACE_CONTEXT))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(RequestEditorEvent::CancelRequested)));
        }
        Button::new("send")
            .primary()
            .label("Send")
            .tooltip_with_action("Send request", &SendRequest, Some(WORKSPACE_CONTEXT))
            .on_click(cx.listener(|this, _, _, cx| this.send(cx)))
    }

    pub(super) fn render_unresolved_hint(&self) -> impl IntoElement {
        UnresolvedHint::new("unresolved-variables", self.unresolved_hint.clone())
    }
}
