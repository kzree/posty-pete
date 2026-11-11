use gpui::{Context, Entity, Window, px};
use gpui_component::ResizableState;

use super::{LeftPanelState, Workspace};
use crate::domain::layout::Layout;

#[derive(Default)]
pub(super) struct OwnResizes {
    sidebar: usize,
    split: usize,
}

/// Resizes the first panel of `state` to `size` unless it already is. Returns
/// whether it did, which emits one `Resized`.
fn resize(
    state: &Entity<ResizableState>,
    size: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let current = state.read(cx).sizes().first().map(|size| size.as_f32());
    if current.is_none_or(|current| (current - size).abs() < 0.5) {
        return false;
    }
    state.update(cx, |state, cx| state.resize_panel(0, px(size), window, cx));
    true
}

impl Workspace {
    pub(super) fn sidebar_resized(&mut self, cx: &mut Context<Self>) {
        if self.own_resizes.sidebar > 0 {
            self.own_resizes.sidebar -= 1;
            return;
        }
        let Some(width) = self.main_layout.read(cx).sizes().first() else {
            return;
        };
        self.layout.sidebar_width = width.as_f32();
        self.schedule_navigation_save(cx);
    }

    pub(super) fn split_resized(&mut self, cx: &mut Context<Self>) {
        if self.own_resizes.split > 0 {
            self.own_resizes.split -= 1;
            return;
        }
        let &[request, response] = self.request_response_layout.read(cx).sizes().as_slice() else {
            return;
        };
        if let Some(split) = Layout::split_of(request.as_f32(), response.as_f32()) {
            self.layout.request_split = Some(split);
            self.schedule_navigation_save(cx);
        }
    }

    pub(super) fn current_layout(&self) -> Layout {
        Layout {
            sidebar_collapsed: self.left_panel == LeftPanelState::Closed,
            ..self.layout
        }
    }

    /// Applies `saved`, clamped to the window, while keeping it as stored
    /// until the user changes it. The split waits until it has been laid out,
    /// since it is restored relative to its height.
    pub(super) fn restore_layout(
        &mut self,
        saved: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.layout = saved;
        let width = saved.sidebar_width_for(window.viewport_size().width.as_f32());
        self.sidebar_initial_width = width;
        if resize(&self.main_layout.clone(), width, window, cx) {
            self.own_resizes.sidebar += 1;
        }
        self.split_pending = true;
        self.apply_pending_split(window, cx);
        let panel = if saved.sidebar_collapsed {
            LeftPanelState::Closed
        } else {
            LeftPanelState::Open
        };
        if self.left_panel != panel {
            self.left_panel = panel;
            self.left_panel_changed(cx);
        }
    }

    pub(super) fn apply_pending_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.split_pending {
            return;
        }
        let state = self.request_response_layout.read(cx);
        let container = state.container_size().as_f32();
        if state.sizes().len() != 2 || container <= 0. {
            return;
        }
        self.split_pending = false;
        if let Some(height) = self.layout.request_height_for(container)
            && resize(&self.request_response_layout.clone(), height, window, cx)
        {
            self.own_resizes.split += 1;
        }
    }
}
