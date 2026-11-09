use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
};

use crate::actions::{OpenSettings, ToggleSidebar, WORKSPACE_CONTEXT};

pub struct StatusBar {
    left_panel_open: bool,
}

impl StatusBar {
    pub fn new(left_panel_open: bool) -> Self {
        Self { left_panel_open }
    }

    pub fn set_left_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.left_panel_open = open;
        cx.notify();
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toggle_label = if self.left_panel_open {
            "Close left panel"
        } else {
            "Open left panel"
        };
        div()
            .flex()
            .flex_none()
            .h_6()
            .px_2()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .child(
                Button::new("l-panel-toggle")
                    .ghost()
                    .xsmall()
                    .icon(IconName::PanelLeft)
                    .accessibility_label(toggle_label)
                    .tooltip_with_action(toggle_label, &ToggleSidebar, Some(WORKSPACE_CONTEXT))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleSidebar), cx)),
            )
            .child(
                Button::new("settings")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings)
                    .accessibility_label("Settings")
                    .tooltip_with_action("Settings", &OpenSettings, Some(WORKSPACE_CONTEXT))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
    }
}
