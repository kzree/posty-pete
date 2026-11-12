use gpui::{AppContext as _, Context, Entity, Render, TestAppContext, VisualTestContext, Window};
use gpui_component::{Root, input::InputState};

use crate::{settings::AppSettings, theme};

/// Sets up `cx` the way `main` does, with default settings that aren't saved,
/// and opens a window showing the view `build` makes.
pub fn open_view<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> (Entity<V>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        AppSettings::init_for_tests(cx);
        theme::init(cx);
    });
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let built = cx.new(|cx| build(window, cx));
        view = Some(built.clone());
        Root::new(built, window, cx)
    });
    (view.expect("window built"), cx)
}

/// Replaces the input's text as an edit, so it emits `InputEvent::Change`
/// like typing does.
pub fn type_into(input: &Entity<InputState>, text: &str, cx: &mut VisualTestContext) {
    input.update_in(cx, |input, window, cx| {
        input.replace_all(text.to_string(), window, cx)
    });
}
