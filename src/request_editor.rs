use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use gpui_component::{
    IndexPath, h_flex,
    input::{Input, InputState},
    searchable_list::SearchableListItem,
    select::{Select, SelectState},
    v_flex,
};

use crate::domain::request::HttpMethod;

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
}

impl RequestEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items: Vec<MethodItem> = HttpMethod::ALL.into_iter().map(MethodItem).collect();
        let method = cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx));
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://..."));

        Self { method, url }
    }
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(div().flex_1().child(Input::new(&self.url))),
        )
    }
}
