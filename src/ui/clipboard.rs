use gpui::{App, ClipboardItem, Window};
use gpui_component::{WindowExt, notification::Notification};

struct CopiedNotification;

/// Copies `text` and confirms it with a toast that replaces any earlier one.
pub fn copy_with_notification(
    text: impl Into<String>,
    message: &'static str,
    window: &mut Window,
    cx: &mut App,
) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
    window.push_notification(
        Notification::success(message).id::<CopiedNotification>(),
        cx,
    );
}
