use gpui::{AnyElement, Context, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{ActiveTheme, h_flex, scroll::ScrollableElement, tab::TabBar, v_flex};

use crate::domain::http::Response;

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
}

impl ResponseView {
    pub fn new() -> Self {
        Self {
            response: None,
            active_tab: ResponseTab::Body,
        }
    }

    pub fn set_response(&mut self, response: Result<Response, String>, cx: &mut Context<Self>) {
        self.response = Some(response);
        cx.notify();
    }

    fn render_tab_content(&self, response: &Response, cx: &Context<Self>) -> AnyElement {
        match self.active_tab {
            ResponseTab::Body => div()
                .text_sm()
                .child(response.body.clone())
                .into_any_element(),
            ResponseTab::Headers => v_flex()
                .gap_1()
                .children(response.headers.iter().map(|(name, value)| {
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
                }))
                .into_any_element(),
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
                            div()
                                .px_3()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {} ms",
                                    response.status,
                                    response.elapsed.as_millis()
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_4()
                        .child(self.render_tab_content(response, cx)),
                )
                .into_any_element(),
        };

        div().size_full().child(content)
    }
}
