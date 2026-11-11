use gpui::{
    AnyElement, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, relative,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Editor, EditorState},
    scroll::ScrollableElement,
    tab::TabBar,
    v_flex,
};

use crate::{
    domain::{body_format::BodyFormat, response::Response, send_error::SendError},
    ui::{clipboard::copy_with_notification, status_tag::status_tag},
};
use gpui_kit_assets::IconName as LucideIcon;

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

pub enum ResponseViewEvent {
    /// The user cleared the response.
    Cleared,
}

pub struct ResponseView {
    response: Option<Result<Response, SendError>>,
    /// The last send was cancelled; only meaningful while `response` is `None`.
    cancelled: bool,
    active_tab: ResponseTab,
    body: Entity<EditorState>,
}

impl EventEmitter<ResponseViewEvent> for ResponseView {}

impl ResponseView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let body = cx.new(|cx| EditorState::new(window, cx).language("json"));
        Self {
            response: None,
            cancelled: false,
            active_tab: ResponseTab::Body,
            body,
        }
    }

    pub fn set_response(
        &mut self,
        response: Result<Response, SendError>,
        body_text: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = response
            .as_ref()
            .map_or(BodyFormat::Text, |resp| resp.body_format());
        self.body.update(cx, |state, cx| {
            state.set_highlighter(editor_language(language), cx);
            state.set_value(body_text, window, cx);
        });
        self.response = Some(response);
        self.cancelled = false;
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.response = None;
        self.cancelled = false;
        self.body
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// Replaces the response with a note that the request was cancelled.
    pub fn show_cancelled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(window, cx);
        self.cancelled = true;
    }

    fn copy_button(&self, response: &Response, cx: &mut Context<Self>) -> Button {
        let label = if response.is_binary() {
            "Binary body can't be copied"
        } else {
            "Copy response body"
        };
        Button::new("copy-response")
            .ghost()
            .xsmall()
            .icon(Icon::new(IconName::Copy))
            .tooltip(label)
            .accessibility_label(label)
            .disabled(response.is_binary())
            .on_click(cx.listener(|this, _, window, cx| {
                let body = this.body.read(cx).value().to_string();
                copy_with_notification(body, "Response body copied", window, cx);
            }))
    }

    fn clear_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("clear-response")
            .ghost()
            .xsmall()
            .icon(Icon::new(LucideIcon::Trash))
            .tooltip("Clear response")
            .accessibility_label("Clear response")
            .on_click(cx.listener(|this, _, window, cx| {
                this.clear(window, cx);
                cx.emit(ResponseViewEvent::Cleared);
            }))
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
                .text_sm()
                .child(if self.cancelled {
                    "Request cancelled"
                } else {
                    "Send a request to see the response."
                })
                .into_any_element(),
            Some(Err(error)) => h_flex()
                .items_start()
                .gap_2()
                .p_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_color(cx.theme().danger)
                                .child(format!("Request failed: {}", error.message)),
                        )
                        .children(error.hint.clone().map(|hint| {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(hint)
                        })),
                )
                .child(self.clear_button(cx))
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
                        .suffix({
                            let separator = || {
                                Icon::new(LucideIcon::Dot)
                                    .xsmall()
                                    .text_color(cx.theme().muted_foreground)
                            };
                            h_flex()
                                .gap_1()
                                .child(status_tag(response.status, response.status_text()))
                                .child(separator())
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} ms", response.elapsed.as_millis())),
                                )
                                .child(separator())
                                .child(self.copy_button(response, cx))
                                .child(self.clear_button(cx))
                        }),
                )
                .child(self.render_tab_content(response, cx))
                .into_any_element(),
        };

        div().size_full().child(content)
    }
}
