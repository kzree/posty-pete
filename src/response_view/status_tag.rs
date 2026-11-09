use gpui::ParentElement;
use gpui_component::{ColorName, Sizable, tag::Tag};

use crate::domain::response::{Response, StatusClass};

/// Status code + reason phrase, colored by status class.
/// Tuned for the dark theme: opaque dark tint, bright text.
pub(super) fn status_tag(response: &Response) -> Tag {
    let color = match response.status_class() {
        StatusClass::Success => ColorName::Green,
        StatusClass::Redirect => ColorName::Blue,
        StatusClass::ClientError => ColorName::Amber,
        StatusClass::ServerError => ColorName::Red,
        StatusClass::Informational => ColorName::Gray,
    };
    Tag::custom(
        color.scale(950),
        color.scale(200),
        color.scale(800).opacity(0.6),
    )
    .small()
    .child(response.status_text())
}
