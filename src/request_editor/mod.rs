mod method_item;
mod tabs;
mod url_bar;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window,
};
use gpui_component::{
    IndexPath,
    input::{InputEvent, InputState},
    select::SelectState,
    v_flex,
};

use crate::domain::request::Request;
use method_item::MethodItem;
use tabs::RequestTab;

pub enum RequestEditorEvent {
    SendRequested(Request),
}

pub struct RequestEditor {
    method: Entity<SelectState<Vec<MethodItem>>>,
    url: Entity<InputState>,
    active_tab: RequestTab,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestEditorEvent> for RequestEditor {}

impl RequestEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let method = cx.new(|cx| {
            SelectState::new(MethodItem::all(), Some(IndexPath::new(0)), window, cx)
        });
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://..."));

        let sub = cx.subscribe_in(
            &url,
            window,
            |this, _url, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.send(cx);
                }
            },
        );

        Self {
            method,
            url,
            active_tab: RequestTab::Params,
            _subscriptions: vec![sub],
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        if self.input_is_empty(cx) {
            return;
        }
        let req = self.build_request(cx);
        cx.emit(RequestEditorEvent::SendRequested(req));
    }

    fn input_is_empty(&self, cx: &App) -> bool {
        self.url.read(cx).value().to_string().is_empty()
    }

    fn build_request(&self, cx: &App) -> Request {
        Request {
            method: self
                .method
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            url: self.url.read(cx).value().into(),
        }
    }
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_2()
            .child(self.render_url_bar(cx))
            .child(self.render_tab_bar(cx))
    }
}
