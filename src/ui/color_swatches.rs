use std::rc::Rc;

use gpui::{
    App, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Toggled,
    Window, accesskit::Orientation, div, prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, ThemeStyled, h_flex, tooltip::Tooltip};

type ChangeHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// A row of colour circles that behaves like a radio group: one Tab stop, arrow keys move the
/// selection.
#[derive(IntoElement)]
pub struct ColorSwatches {
    id: ElementId,
    label: SharedString,
    swatches: Vec<(SharedString, Hsla)>,
    selected: Option<usize>,
    on_change: Option<ChangeHandler>,
}

impl ColorSwatches {
    /// `label` names the group for assistive technology.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            swatches: Vec::new(),
            selected: None,
            on_change: None,
        }
    }

    pub fn swatch(mut self, name: impl Into<SharedString>, color: impl Into<Hsla>) -> Self {
        self.swatches.push((name.into(), color.into()));
        self
    }

    pub fn selected_index(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }

    /// Called with the requested index; the owner stores it and re-renders.
    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    fn focus_handles(&self, window: &mut Window, cx: &mut App) -> Vec<FocusHandle> {
        let len = self.swatches.len();
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| {
            (0..len).map(|_| cx.focus_handle()).collect::<Vec<_>>()
        });
        if state.read(cx).len() != len {
            state.update(cx, |handles, cx| {
                handles.resize_with(len, || cx.focus_handle());
            });
        }
        state.read(cx).clone()
    }

    fn on_key_down(
        handles: Rc<Vec<FocusHandle>>,
        selected: Option<usize>,
        on_change: Option<ChangeHandler>,
    ) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        move |event, window, cx| {
            if event.keystroke.modifiers.modified() {
                return;
            }
            let current = handles
                .iter()
                .position(|handle| handle.is_focused(window))
                .or(selected)
                .unwrap_or(0);
            let Some(next) = next_index(&event.keystroke.key, current, handles.len()) else {
                return;
            };
            cx.stop_propagation();
            window.focus(&handles[next], cx);
            if selected != Some(next)
                && let Some(on_change) = &on_change
            {
                on_change(&next, window, cx);
            }
        }
    }
}

impl RenderOnce for ColorSwatches {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handles = Rc::new(self.focus_handles(window, cx));
        let len = self.swatches.len();
        let tab_stop = self.selected.filter(|ix| *ix < len).unwrap_or(0);
        let swatches = self
            .swatches
            .into_iter()
            .enumerate()
            .map(|(ix, (name, color))| Swatch {
                ix,
                len,
                name,
                color,
                selected: self.selected == Some(ix),
                focus: handles[ix].clone().tab_stop(ix == tab_stop),
                on_change: self.on_change.clone(),
            })
            .collect::<Vec<_>>();
        h_flex()
            .id(self.id)
            .role(Role::RadioGroup)
            .aria_label(self.label)
            .aria_orientation(Orientation::Horizontal)
            .gap_2()
            .on_key_down(Self::on_key_down(handles, self.selected, self.on_change))
            .children(swatches)
    }
}

#[derive(IntoElement)]
struct Swatch {
    ix: usize,
    len: usize,
    name: SharedString,
    color: Hsla,
    selected: bool,
    focus: FocusHandle,
    on_change: Option<ChangeHandler>,
}

impl RenderOnce for Swatch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let ring = if self.selected {
            theme.foreground
        } else {
            gpui::transparent_black()
        };
        let hover_ring = theme.muted_foreground.opacity(0.5);
        let show_focus = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let ix = self.ix;
        let name = self.name.clone();
        div()
            .id(ix)
            .role(Role::RadioButton)
            .aria_label(self.name)
            .aria_selected(self.selected)
            .aria_toggled(if self.selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .aria_position_in_set(ix + 1)
            .aria_size_of_set(self.len)
            .track_focus(&self.focus)
            .relative()
            .size(px(24.))
            .p(px(2.))
            .rounded_full()
            .border_2()
            .border_color(ring)
            .cursor_pointer()
            .when(!self.selected, |this| {
                this.hover(move |style| style.border_color(hover_ring))
            })
            .when(show_focus, |this| this.focus_ring_style(window, cx))
            .child(div().size_full().rounded_full().bg(self.color))
            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
            .when_some(
                self.on_change.filter(|_| !self.selected),
                |this, on_change| this.on_click(move |_, window, cx| on_change(&ix, window, cx)),
            )
    }
}

/// The swatch an arrow, Home or End key moves to from `current`, wrapping at the ends.
fn next_index(key: &str, current: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match key {
        "left" | "up" => Some((current + len - 1) % len),
        "right" | "down" => Some((current + 1) % len),
        "home" => Some(0),
        "end" => Some(len - 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_and_wrap() {
        assert_eq!(next_index("right", 0, 3), Some(1));
        assert_eq!(next_index("down", 2, 3), Some(0));
        assert_eq!(next_index("left", 0, 3), Some(2));
        assert_eq!(next_index("up", 2, 3), Some(1));
    }

    #[test]
    fn home_and_end_jump_to_the_ends() {
        assert_eq!(next_index("home", 2, 3), Some(0));
        assert_eq!(next_index("end", 0, 3), Some(2));
    }

    #[test]
    fn other_keys_and_empty_rows_are_ignored() {
        assert_eq!(next_index("a", 0, 3), None);
        assert_eq!(next_index("right", 0, 0), None);
    }
}
