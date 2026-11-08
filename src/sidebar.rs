use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use gpui_component::{ActiveTheme, Colorize};

/// The left-hand sidebar (collections, history).
#[derive(IntoElement)]
pub struct Sidebar;

impl Sidebar {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().sidebar.lighten(1.0))
            .child("Sidebar")
    }
}
