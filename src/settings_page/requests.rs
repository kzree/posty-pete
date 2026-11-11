use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable,
    input::{Input, InputState, NumberInput},
    radio::RadioGroup,
};

use super::{group, number_state, page, switch};
use crate::ui::setting_row::SettingRow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProxyMode {
    None,
    System,
    Custom,
}

impl ProxyMode {
    const ALL: [Self; 3] = [Self::None, Self::System, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::System => "System",
            Self::Custom => "Custom",
        }
    }
}

pub struct RequestsPage {
    timeout: Entity<InputState>,
    follow_redirects: bool,
    max_redirects: Entity<InputState>,
    verify_tls: bool,
    proxy: ProxyMode,
    proxy_url: Entity<InputState>,
    user_agent: Entity<InputState>,
}

impl RequestsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            timeout: cx.new(|cx| number_state(30, 0, 3600, window, cx)),
            follow_redirects: true,
            max_redirects: cx.new(|cx| number_state(10, 1, 50, window, cx)),
            verify_tls: true,
            proxy: ProxyMode::System,
            proxy_url: cx
                .new(|cx| InputState::new(window, cx).placeholder("http://proxy.example.com:8080")),
            // reqwest sends no User-Agent unless one is set.
            user_agent: cx.new(|cx| InputState::new(window, cx).placeholder("None")),
        }
    }

    fn render_proxy(&self, cx: &mut Context<Self>) -> RadioGroup {
        let selected = ProxyMode::ALL.iter().position(|m| *m == self.proxy);
        RadioGroup::horizontal("proxy")
            .children(ProxyMode::ALL.map(ProxyMode::label))
            .selected_index(selected)
            .on_change(cx.listener(|this, index: &usize, _, cx| {
                this.proxy = ProxyMode::ALL[*index];
                cx.notify();
            }))
    }

    fn render_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        group("requests-connection", "Connection")
            .child(
                SettingRow::new("Timeout")
                    .description("In seconds. 0 means no timeout.")
                    .control(
                        NumberInput::new(&self.timeout)
                            .accessibility_label("Timeout in seconds")
                            .w(px(120.)),
                    ),
            )
            .child(
                SettingRow::new("Verify TLS certificates")
                    .when(!self.verify_tls, |row| {
                        row.warning(
                            "Anyone on the network could read or change requests to HTTPS servers.",
                        )
                    })
                    .control(switch(
                        "verify-tls",
                        "Verify TLS certificates",
                        self.verify_tls,
                        |this, on| this.verify_tls = on,
                        cx,
                    )),
            )
            .child(SettingRow::new("Proxy").control(self.render_proxy(cx)))
            .when(self.proxy == ProxyMode::Custom, |group| {
                group.child(
                    SettingRow::new("Proxy URL").control(
                        Input::new(&self.proxy_url)
                            .aria_label("Proxy URL")
                            .w(px(240.)),
                    ),
                )
            })
    }

    fn render_redirects(&self, cx: &mut Context<Self>) -> impl IntoElement {
        group("requests-redirects", "Redirects")
            .child(
                SettingRow::new("Follow redirects")
                    .description(
                        "Only to the same origin; other redirects are shown as the response.",
                    )
                    .control(switch(
                        "follow-redirects",
                        "Follow redirects",
                        self.follow_redirects,
                        |this, on| this.follow_redirects = on,
                        cx,
                    )),
            )
            .child(
                SettingRow::new("Maximum redirects").control(
                    NumberInput::new(&self.max_redirects)
                        .accessibility_label("Maximum redirects")
                        .disabled(!self.follow_redirects)
                        .w(px(120.)),
                ),
            )
    }

    fn render_headers(&self) -> impl IntoElement {
        group("requests-headers", "Headers").child(
            SettingRow::new("User-Agent")
                .description("Sent when a request doesn't set its own.")
                .control(
                    Input::new(&self.user_agent)
                        .aria_label("User-Agent")
                        .w(px(240.)),
                ),
        )
    }
}

impl Render for RequestsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_connection(cx))
            .child(self.render_redirects(cx))
            .child(self.render_headers())
    }
}
