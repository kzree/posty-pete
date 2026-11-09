use gpui::{
    AnyElement, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    div, relative,
};
use gpui_component::{
    ActiveTheme, h_flex,
    input::{Editor, EditorState},
    scroll::ScrollableElement,
    tab::TabBar,
    v_flex,
};

mod status_tag;

use crate::domain::{body_format::BodyFormat, response::Response};
use status_tag::status_tag;

/// gpui-component highlighter language for a body format.
fn editor_language(format: BodyFormat) -> &'static str {
    match format {
        BodyFormat::Json => "json",
        BodyFormat::Html => "html",
        BodyFormat::Xml => "html", // gpui-component 0.9 has no XML grammar
        BodyFormat::Css => "css",
        BodyFormat::JavaScript => "javascript",
        BodyFormat::Yaml => "yaml",
        BodyFormat::Text => "text",
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ResponseTab {
    Body,
    Headers,
}

impl ResponseTab {
    const ALL: [ResponseTab; 2] = [Self::Body, Self::Headers];

    fn label(self) -> &'static str {
        match self {
            Self::Body => "Body",
            Self::Headers => "Headers",
        }
    }
}

pub struct ResponseView {
    response: Option<Result<Response, String>>,
    active_tab: ResponseTab,
    body: Entity<EditorState>,
}

impl ResponseView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let body = cx.new(|cx| EditorState::new(window, cx).language("json"));
        Self {
            response: None,
            active_tab: ResponseTab::Body,
            body,
        }
    }

    pub fn set_response(
        &mut self,
        response: Result<Response, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(resp) = &response {
            let language = editor_language(resp.body_format());
            let text = resp.pretty_body();
            self.body.update(cx, |state, cx| {
                state.set_highlighter(language, cx);
                state.set_value(text, window, cx);
            });
        }
        self.response = Some(response);
        cx.notify();
    }
    fn render_tab_content(&self, response: &Response, cx: &Context<Self>) -> AnyElement {
        match self.active_tab {
            ResponseTab::Body => div()
                .flex_1()
                .min_h_0()
                .p_2()
                .child(Editor::new(&self.body).readonly(true).h(relative(1.)))
                .into_any_element(),
            ResponseTab::Headers => {
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_4()
                    .child(v_flex().gap_1().children(response.headers.iter().map(
                        |(name, value)| {
                            h_flex()
                                .gap_4()
                                .items_start()
                                .text_sm()
                                .child(
                                    div()
                                        .w_48()
                                        .flex_none()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(name.clone()),
                                )
                                .child(div().flex_1().min_w_0().child(value.clone()))
                        },
                    )))
                    .into_any_element()
            }
        }
    }
}

impl Render for ResponseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.response {
            None => div()
                .p_4()
                .text_color(cx.theme().muted_foreground)
                .child("Send a request to see the response.")
                .into_any_element(),
            Some(Err(error)) => div()
                .p_4()
                .text_color(cx.theme().danger)
                .child(format!("Request failed: {error}"))
                .into_any_element(),
            Some(Ok(response)) => v_flex()
                .size_full()
                .child(
                    TabBar::new("response-tabs")
                        .px_4()
                        .underline()
                        .selected_index(
                            ResponseTab::ALL
                                .iter()
                                .position(|t| *t == self.active_tab)
                                .unwrap_or(0),
                        )
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.active_tab = ResponseTab::ALL[*ix];
                            cx.notify();
                        }))
                        .children(ResponseTab::ALL.map(ResponseTab::label))
                        .suffix(
                            h_flex().px_3().gap_2().child(status_tag(response)).child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} ms", response.elapsed.as_millis())),
                            ),
                        ),
                )
                .child(self.render_tab_content(response, cx))
                .into_any_element(),
        };

        div().size_full().child(content)
    }
}
