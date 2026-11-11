use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    select::Select,
    tooltip::Tooltip,
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
                Button::new("send")
                    .primary()
                    .label("Send")
                    .loading(self.loading)
                    .tooltip_with_action("Send request", &SendRequest, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
            )
    }

    /// Always takes its line, so the tabs don't move when the hint comes and goes.
    pub(super) fn render_unresolved_hint(&self, cx: &Context<Self>) -> impl IntoElement {
        let row = h_flex()
            .id("unresolved-variables")
            .h_4()
            .gap_1p5()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        let Some(hint) = self.unresolved_hint.clone() else {
            return row;
        };
        row.child(
            Icon::new(IconName::TriangleAlert)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().warning),
        )
        .child(div().min_w_0().truncate().child(hint.clone()))
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
    }
}
