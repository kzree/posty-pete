use gpui::{App, Div, ParentElement, Styled, div, px};
use gpui_component::{ActiveTheme, StyledExt};

use crate::domain::request::HttpMethod;

pub fn method_label(method: HttpMethod, cx: &App) -> Div {
    div()
        .w(px(52.))
        .flex_none()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(method.as_str())
}
