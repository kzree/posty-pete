use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, Context, Global, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, ease_in_out, px, relative,
};
use gpui_component::ActiveTheme;

#[derive(Default)]
pub struct GlobalLoading {
    active: usize,
}

impl Global for GlobalLoading {}

impl GlobalLoading {
    pub fn begin(cx: &mut App) {
        cx.default_global::<Self>().active += 1;
    }

    pub fn end(cx: &mut App) {
        let loading = cx.default_global::<Self>();
        loading.active = loading.active.saturating_sub(1);
    }

    pub fn is_active(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|loading| loading.active > 0)
    }
}

/// Render as the last child of a `relative()` container.
pub struct LoadingBar {
    _observe: Subscription,
}

impl LoadingBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _observe: cx.observe_global::<GlobalLoading>(|_, cx| cx.notify()),
        }
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

        if !GlobalLoading::is_active(cx) {
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
