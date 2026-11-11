use gpui::{
    AnyElement, Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    NavigationDirection, ParentElement, Render, Role, Stateful, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, h_resizable, input::Escape, resizable_panel};

use super::{LeftPanelState, Workspace, navigation::Direction, routes::AppRoute};
use crate::{
    actions::{
        FocusUrl, GoBack, GoForward, NewRequest, OpenHistory, OpenLibrary, OpenSettings, OpenTests,
        QuickOpen, RevertRequest, SaveRequest, SaveRequestAs, SendRequest, ToggleSidebar,
        WORKSPACE_CONTEXT,
    },
    sidebar::SidebarTab,
    window_title_bar::WindowTitleBar,
};

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .id("workspace")
            .role(Role::Group)
            .aria_label("Workspace")
            .track_focus(&self.focus_handle)
            .key_context(WORKSPACE_CONTEXT);
        let root = self.register_request_actions(root, cx);
        self.register_navigation_actions(root, cx)
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_body(cx)),
            )
            .child(self.status_bar.clone())
    }
}

impl Workspace {
    fn register_request_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &SendRequest, _, cx| {
            this.request_editor.update(cx, |editor, cx| editor.send(cx));
        }))
        .on_action(cx.listener(|this, _: &SaveRequest, window, cx| {
            this.save_request(window, cx);
        }))
        // Save As and Revert are only registered where they act, so their menu items grey out elsewhere.
        .when(self.current_route() == self.editor_route(), |this| {
            this.on_action(cx.listener(|this, _: &SaveRequestAs, window, cx| {
                this.open_save_as(window, cx);
            }))
        })
        .when(self.shows_dirty_request(), |this| {
            this.on_action(cx.listener(|this, _: &RevertRequest, window, cx| {
                this.revert_request(window, cx);
            }))
        })
        .on_action(cx.listener(|this, _: &NewRequest, window, cx| {
            this.new_request(window, cx);
        }))
        .on_action(cx.listener(|this, _: &FocusUrl, window, cx| {
            this.request_editor
                .update(cx, |editor, cx| editor.focus_url(window, cx));
        }))
    }

    fn register_navigation_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &GoBack, window, cx| {
            this.go(Direction::Back, window, cx);
        }))
        .on_action(cx.listener(|this, _: &GoForward, window, cx| {
            this.go(Direction::Forward, window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
            this.toggle_left_panel(cx);
        }))
        .on_action(cx.listener(|this, _: &OpenLibrary, _, cx| {
            this.open_sidebar_tab(SidebarTab::Library, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenHistory, _, cx| {
            this.open_sidebar_tab(SidebarTab::History, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenTests, _, cx| {
            this.open_sidebar_tab(SidebarTab::Tests, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
            this.navigate(AppRoute::Settings, window, cx);
        }))
        .on_action(cx.listener(|this, _: &QuickOpen, window, cx| {
            this.open_quick_open(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Escape, window, cx| {
            window.focus(&this.focus_handle, cx);
        }))
        // Capture phase: disabled inputs and scrollbars stop mouse-downs before they bubble.
        .capture_any_mouse_down(cx.listener(
            |this, event: &MouseDownEvent, window, cx| {
                let direction = match event.button {
                    MouseButton::Navigate(NavigationDirection::Back) => Direction::Back,
                    MouseButton::Navigate(NavigationDirection::Forward) => Direction::Forward,
                    _ => return,
                };
                this.go(direction, window, cx);
            },
        ))
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> WindowTitleBar {
        let title_collection = match self.current_route() {
            AppRoute::SavedRequest { collection_id, .. } => Some(collection_id),
            _ => None,
        };
        WindowTitleBar::new(self.title(cx))
            .edited(self.shows_dirty_request())
            .can_go_back(self.can_go(Direction::Back, cx))
            .can_go_forward(self.can_go(Direction::Forward, cx))
            .on_back(cx.listener(|this, _, window, cx| this.go(Direction::Back, window, cx)))
            .on_forward(cx.listener(|this, _, window, cx| this.go(Direction::Forward, window, cx)))
            .on_new_request(cx.listener(|this, _, window, cx| this.new_request(window, cx)))
            .when_some(title_collection, |this, id| {
                this.on_context_click(cx.listener(move |this, _, window, cx| {
                    this.navigate(AppRoute::Collection(id), window, cx)
                }))
            })
            .when_some(self.environment_picker(cx), WindowTitleBar::trailing)
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let main_content = self.routes(cx);
        // A hidden panel would still leave its resize handle behind.
        if self.left_panel != LeftPanelState::Open {
            return main_content.into_any_element();
        }
        h_resizable("main-layout")
            .with_state(&self.main_layout)
            .child(
                resizable_panel()
                    .size(px(280.))
                    .size_range(px(200.)..px(400.))
                    .fixed()
                    .child(self.sidebar.clone()),
            )
            .child(resizable_panel().child(main_content))
            .into_any_element()
    }
}
