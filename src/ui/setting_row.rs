use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};

/// A setting's label and optional description, with its control on the right.
#[derive(IntoElement)]
pub struct SettingRow {
    label: SharedString,
    description: Option<SharedString>,
    warning: Option<SharedString>,
    control: Option<AnyElement>,
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            warning: None,
            control: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Shown under the description, in the warning color.
    pub fn warning(mut self, warning: impl Into<SharedString>) -> Self {
        self.warning = Some(warning.into());
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(div().text_sm().child(self.label))
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            })
            .when_some(self.warning, |this, warning| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(warning),
                )
            });
        h_flex()
            .w_full()
            .gap_4()
            .justify_between()
            .child(text)
            .when_some(self.control, |this, control| {
                this.child(div().flex_none().child(control))
            })
    }
}
