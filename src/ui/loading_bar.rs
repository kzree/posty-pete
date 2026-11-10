use std::time::Duration;

use gpui::{
    Animation, AnimationExt, Context, IntoElement, ParentElement, Render, Styled, Window, div,
    ease_in_out, px, relative,
};
use gpui_component::ActiveTheme;

/// Render as the last child of a `relative()` container.
#[derive(Default)]
pub struct LoadingBar {
    active: bool,
}

impl LoadingBar {
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.active = active;
        cx.notify();
    }
}

impl Render for LoadingBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(2.))
            .overflow_hidden();

        if !self.active {
            return bar;
        }

        let primary = cx.theme().primary;
        if cx.reduce_motion() {
            return bar.bg(primary.opacity(0.6));
        }

        bar.bg(primary.opacity(0.2)).child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(0.3))
                .bg(primary)
                .with_animation(
                    "loading-bar-sweep",
                    Animation::new(Duration::from_millis(1200))
                        .repeat()
                        .with_easing(ease_in_out),
                    |segment, delta| segment.left(relative(delta * 1.3 - 0.3)),
                ),
        )
    }
}
