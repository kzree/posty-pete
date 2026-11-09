mod method_item;
mod params;
mod tabs;
mod url_bar;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};
use gpui_component::{
    IndexPath,
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    select::SelectState,
    v_flex,
};

use crate::domain::request::Request;
use method_item::MethodItem;
use params::ParamRow;
use tabs::RequestTab;

pub enum RequestEditorEvent {
    SendRequested(Request),
}

pub struct RequestEditor {
    method: Entity<SelectState<Vec<MethodItem>>>,
    url: Entity<InputState>,
    active_tab: RequestTab,
    params: Vec<ParamRow>,
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

        let mut this = Self {
            method,
            url,
            active_tab: RequestTab::Params,
            params: Vec::new(),
            _subscriptions: vec![sub],
        };
        this.add_param_row(window, cx);
        this
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
            params: self
                .params
                .iter()
                .filter(|row| !row.is_empty(cx))
                .map(|row| row.to_param(cx))
                .collect(),
        }
    }
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.active_tab {
            RequestTab::Params => self.render_params(cx).into_any_element(),
            // Other tabs come later.
            _ => div().into_any_element(),
        };

        v_flex()
            .size_full()
            .p_4()
            .gap_2()
            .child(self.render_url_bar(cx))
            .child(self.render_tab_bar(cx))
            .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(content))
    }
}
