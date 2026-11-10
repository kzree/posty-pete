use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce,
    SharedString, Styled, Window, div,
};
use gpui_component::{
    ActiveTheme, Icon, Sizable, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use crate::actions::{NewRequest, WORKSPACE_CONTEXT};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct WindowTitleBar {
    title: SharedString,
    on_new_request: Option<ClickHandler>,
}

impl WindowTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            on_new_request: None,
        }
    }

    pub fn on_new_request(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_new_request = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let home = Button::new("new-request")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::House))
            .accessibility_label("New request")
            .tooltip_with_action("New request", &NewRequest, Some(WORKSPACE_CONTEXT));
        let home = match self.on_new_request {
            Some(handler) => home.on_click(handler),
            None => home,
        };

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
                .child(
                    h_flex()
                        .id("title-bar-nav")
                        .h_full()
                        .gap_1()
                        // Keeps a click on a button from starting a window drag.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(home),
                ),
        )
    }
}
