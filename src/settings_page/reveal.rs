use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Pixels, ScrollHandle, Task, Window, point,
    prelude::FluentBuilder, px,
};

use domain::settings_index::Setting;

use crate::ui::setting_row::{HIGHLIGHT_DURATION, SettingRow};

/// Space kept above a row scrolled into view.
const SCROLL_MARGIN: Pixels = px(24.);

/// The setting a palette result revealed: scrolled to, focused and briefly highlighted.
pub struct Reveal {
    scroll: ScrollHandle,
    current: Option<Revealed>,
    generation: usize,
    _clear: Task<()>,
}

struct Revealed {
    setting: Setting,
    generation: usize,
    /// Tracked by the row, so focus can move into it and stay there.
    focus: FocusHandle,
    highlighted: bool,
    scroll_pending: bool,
}

impl Reveal {
    /// `scroll` is the handle of the panel the rows scroll in.
    pub fn new(scroll: ScrollHandle) -> Self {
        Self {
            scroll,
            current: None,
            generation: 0,
            _clear: Task::ready(()),
        }
    }

    pub fn start(&mut self, setting: Setting, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.current = Some(Revealed {
            setting,
            generation,
            focus: cx.focus_handle(),
            highlighted: true,
            scroll_pending: true,
        });
        self._clear = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT_DURATION).await;
            this.update(cx, |this, cx| this.end_highlight(generation, cx))
                .ok();
        });
        cx.notify();
    }

    fn end_highlight(&mut self, generation: usize, cx: &mut Context<Self>) {
        if let Some(revealed) = self.current.as_mut().filter(|r| r.generation == generation) {
            revealed.highlighted = false;
            revealed.scroll_pending = false;
            cx.notify();
        }
    }

    /// Scrolls the revealed row into view, then moves focus into it.
    fn on_row_laid_out(&mut self, row: Bounds<Pixels>, window: &mut Window) {
        let Some(revealed) = self.current.as_mut().filter(|r| r.scroll_pending) else {
            return;
        };
        revealed.scroll_pending = false;
        let offset = self.scroll.offset();
        if let Some(y) = scroll_target(row, self.scroll.bounds(), offset.y) {
            self.scroll.set_offset(point(offset.x, y));
        }
        let focus = revealed.focus.clone();
        window.on_next_frame(move |window, cx| {
            focus_control(&focus, window, cx);
            window.refresh();
        });
    }
}

/// The scroll offset that puts `row` near the top of `viewport`, or `None`
/// when it's already in full view.
fn scroll_target(row: Bounds<Pixels>, viewport: Bounds<Pixels>, offset: Pixels) -> Option<Pixels> {
    if row.top() >= viewport.top() && row.bottom() <= viewport.bottom() {
        return None;
    }
    Some((offset - (row.top() - viewport.top()) + SCROLL_MARGIN).min(px(0.)))
}

/// Focuses the first control in the row, or the row itself when it has none
/// that takes focus.
fn focus_control(row: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(row, cx);
    window.focus_next(cx);
    if !row.contains_focused(window, cx) {
        window.focus(row, cx);
    }
}

/// The row for `setting`, labelled from the index.
pub fn row(reveal: &Entity<Reveal>, setting: Setting, cx: &App) -> SettingRow {
    row_standing_in(reveal, setting, None, cx)
}

/// Like [`row`], also revealed for `hidden`, a row not shown right now.
pub fn row_standing_in(
    reveal: &Entity<Reveal>,
    setting: Setting,
    hidden: Option<Setting>,
    cx: &App,
) -> SettingRow {
    let row = SettingRow::new(setting.label()).id(setting.id());
    let Some(revealed) = reveal
        .read(cx)
        .current
        .as_ref()
        .filter(|r| r.setting == setting || Some(r.setting) == hidden)
    else {
        return row;
    };
    let reveal = reveal.clone();
    row.track_focus(&revealed.focus)
        .when(revealed.highlighted, |row| {
            row.highlight(revealed.generation)
        })
        .when(revealed.scroll_pending, |row| {
            row.on_prepaint(move |bounds, window, cx| {
                reveal.update(cx, |reveal, _| reveal.on_row_laid_out(bounds, window));
            })
        })
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, size};

    use super::*;

    fn bounds(top: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(100.), px(height)))
    }

    #[test]
    fn rows_in_view_stay_put() {
        let viewport = bounds(100., 400.);
        assert_eq!(scroll_target(bounds(150., 40.), viewport, px(0.)), None);
        assert_eq!(scroll_target(bounds(460., 40.), viewport, px(-50.)), None);
    }

    #[test]
    fn rows_below_scroll_up_to_near_the_top() {
        let viewport = bounds(100., 400.);
        assert_eq!(
            scroll_target(bounds(700., 40.), viewport, px(0.)),
            Some(px(-600.) + SCROLL_MARGIN)
        );
        assert_eq!(
            scroll_target(bounds(480., 40.), viewport, px(-100.)),
            Some(px(-480.) + SCROLL_MARGIN)
        );
    }

    #[test]
    fn rows_above_scroll_down_but_not_past_the_start() {
        let viewport = bounds(100., 400.);
        assert_eq!(
            scroll_target(bounds(50., 40.), viewport, px(-300.)),
            Some(px(-250.) + SCROLL_MARGIN)
        );
        assert_eq!(
            scroll_target(bounds(90., 40.), viewport, px(-5.)),
            Some(px(0.))
        );
    }
}
