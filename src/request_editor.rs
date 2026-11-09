use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    IndexPath,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableListItem,
    select::{Select, SelectState},
    v_flex,
};

use crate::domain::request::{HttpMethod, Request};

pub enum RequestEditorEvent {
    SendRequested(Request),
}

#[derive(Clone)]
pub struct MethodItem(HttpMethod);

impl SearchableListItem for MethodItem {
    type Value = HttpMethod;

    fn title(&self) -> SharedString {
        self.0.as_str().into()
    }

    fn value(&self) -> &HttpMethod {
        &self.0
    }
}

pub struct RequestEditor {
    method: Entity<SelectState<Vec<MethodItem>>>,
    url: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestEditorEvent> for RequestEditor {}

impl RequestEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items: Vec<MethodItem> = HttpMethod::ALL.into_iter().map(MethodItem).collect();
        let method = cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx));
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
        v_flex().size_full().p_4().gap_2().child(
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(px(110.))
                        .child(Select::new(&self.method)),
                )
                .child(div().flex_1().child(Input::new(&self.url)))
                .child(
                    Button::new("send")
                        .primary()
                        .label("Send")
                        .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                ),
        )
    }
}
