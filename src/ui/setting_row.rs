use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, ElementId, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, canvas, div,
    ease_in_out, prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};

const HIGHLIGHT_HOLD: Duration = Duration::from_millis(1000);
const HIGHLIGHT_FADE: Duration = Duration::from_millis(500);
/// How long a highlight shows, fade included.
pub const HIGHLIGHT_DURATION: Duration = HIGHLIGHT_HOLD.saturating_add(HIGHLIGHT_FADE);
/// How far the highlight reaches past the row's content.
const HIGHLIGHT_INSET: Pixels = px(8.);

type PrepaintCallback = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App)>;

/// A setting's label and optional description, with its control on the right.
#[derive(IntoElement)]
pub struct SettingRow {
    id: Option<ElementId>,
    label: SharedString,
    description: Option<SharedString>,
    warning: Option<SharedString>,
    control: Option<AnyElement>,
    highlight: Option<usize>,
    focus: Option<FocusHandle>,
    on_prepaint: Option<PrepaintCallback>,
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            id: None,
            label: label.into(),
            description: None,
            warning: None,
            control: None,
            highlight: None,
            focus: None,
            on_prepaint: None,
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Shown under the description, in the warning color.
    pub fn warning(mut self, warning: impl Into<SharedString>) -> Self {
        self.warning = Some(warning.into());
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// Highlights the row for [`HIGHLIGHT_DURATION`]; a new `key` starts it again.
    pub fn highlight(mut self, key: usize) -> Self {
        self.highlight = Some(key);
        self
    }

    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    /// Called with the row's bounds once it's laid out.
    pub fn on_prepaint(
        mut self,
        callback: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_prepaint = Some(Box::new(callback));
        self
    }
}

/// Drawn behind the row's content, so the row's layout doesn't change.
fn highlight_layer(key: usize, cx: &App) -> AnyElement {
    let color = cx.theme().primary.opacity(0.12);
    let layer = div()
        .absolute()
        .top(-px(4.))
        .bottom(-px(4.))
        .left(-HIGHLIGHT_INSET)
        .right(-HIGHLIGHT_INSET)
        .rounded_md();
    if cx.reduce_motion() {
        return layer.bg(color).into_any_element();
    }
    layer
        .with_animations(
            ("setting-highlight", key),
            vec![
                Animation::new(HIGHLIGHT_HOLD),
                Animation::new(HIGHLIGHT_FADE).with_easing(ease_in_out),
            ],
            move |layer, step, delta| {
                let fade = if step == 0 { 0. } else { delta };
                layer.bg(color.opacity(color.a * (1. - fade)))
            },
        )
        .into_any_element()
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(div().text_sm().child(self.label.clone()))
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            })
            .when_some(self.warning, |this, warning| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(warning),
                )
            });
        h_flex()
            .id(self.id.unwrap_or(ElementId::Name(self.label)))
            .relative()
            .w_full()
            .gap_4()
            .justify_between()
            .when_some(self.highlight, |this, key| {
                this.child(highlight_layer(key, cx))
            })
            .child(text)
            .when_some(self.control, |this, control| {
                this.child(div().flex_none().child(control))
            })
            .when_some(self.focus, |this, focus| this.track_focus(&focus))
            .when_some(self.on_prepaint, |this, callback| {
                this.child(
                    canvas(callback, |_, _, _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                )
            })
    }
}
