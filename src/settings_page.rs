use std::rc::Rc;

use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    v_flex,
};

type BackHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SettingsPage {
    on_back: BackHandler,
}

impl SettingsPage {
    pub fn new(on_back: BackHandler) -> Self {
        Self { on_back }
    }
}

impl RenderOnce for SettingsPage {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_back = self.on_back;
        v_flex()
            .id("settings-page")
            .role(Role::Group)
            .aria_label("Settings")
            .size_full()
            .items_start()
            .gap_2()
            .p_4()
            .child(
                Button::new("settings-back")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .label("Back")
                    .on_click(move |event, window, cx| on_back(event, window, cx)),
            )
            .child(
                div()
                    .id("settings-heading")
                    .role(Role::Heading)
                    .aria_level(1)
                    .text_lg()
                    .font_semibold()
                    .child("Settings"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Nothing to configure yet."),
            )
    }
}
