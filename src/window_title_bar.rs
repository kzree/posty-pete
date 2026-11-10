use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce,
    SharedString, Styled, Window, div,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Sizable, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use crate::actions::{GoBack, GoForward, NewRequest, WORKSPACE_CONTEXT};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct WindowTitleBar {
    title: SharedString,
    can_go_back: bool,
    can_go_forward: bool,
    on_back: Option<ClickHandler>,
    on_forward: Option<ClickHandler>,
    on_new_request: Option<ClickHandler>,
}

impl WindowTitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            can_go_back: false,
            can_go_forward: false,
            on_back: None,
            on_forward: None,
            on_new_request: None,
        }
    }

    pub fn can_go_back(mut self, can_go_back: bool) -> Self {
        self.can_go_back = can_go_back;
        self
    }

    pub fn can_go_forward(mut self, can_go_forward: bool) -> Self {
        self.can_go_forward = can_go_forward;
        self
    }

    pub fn on_back(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_back = Some(Box::new(handler));
        self
    }

    pub fn on_forward(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_forward = Some(Box::new(handler));
        self
    }

    pub fn on_new_request(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_new_request = Some(Box::new(handler));
        self
    }
}

fn with_click(button: Button, handler: Option<ClickHandler>) -> Button {
    match handler {
        Some(handler) => button.on_click(handler),
        None => button,
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let back = Button::new("go-back")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::ChevronLeft))
            .accessibility_label("Back")
            .tooltip_with_action("Back", &GoBack, Some(WORKSPACE_CONTEXT))
            .disabled(!self.can_go_back);
        let forward = Button::new("go-forward")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::ChevronRight))
            .accessibility_label("Forward")
            .tooltip_with_action("Forward", &GoForward, Some(WORKSPACE_CONTEXT))
            .disabled(!self.can_go_forward);
        let home = Button::new("new-request")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::House))
            .accessibility_label("New request")
            .tooltip_with_action("New request", &NewRequest, Some(WORKSPACE_CONTEXT));

        TitleBar::new().child(
            h_flex()
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
                        .child(with_click(back, self.on_back))
                        .child(with_click(forward, self.on_forward))
                        .child(with_click(home, self.on_new_request)),
                ),
        )
    }
}
