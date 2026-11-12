use gpui::{ParentElement, SharedString};
use gpui_component::{ColorName, Sizable, tag::Tag};

use domain::response::StatusClass;

pub fn status_tag(status: u16, label: impl Into<SharedString>) -> Tag {
    let color = match StatusClass::from_status(status) {
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
    .child(label.into())
}
