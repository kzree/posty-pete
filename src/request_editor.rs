use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, px,
};
use gpui_component::input::{Input, InputState};

pub struct RequestEditor {
    input: Entity<InputState>,
}

impl RequestEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Type here..."));
        Self { input }
    }
}

impl Render for RequestEditor {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::prelude::Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .p_4()
            .child(div().w(px(400.)).child(Input::new(&self.input)))
    }
}
