use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable,
    button::Button,
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState, NumberInput},
    v_flex,
};

use super::{Reveal, group, number_state, page, row, switch};
use crate::{
    domain::{
        mcp::{self, DEFAULT_PORT, PLACEHOLDER_TOKEN, PORT_RANGE},
        settings_index::{Setting, SettingsGroup},
    },
    ui::setting_row::SettingRow,
};

const NOT_AVAILABLE: &str = "Not available yet";

/// The MCP server's settings, shown on the AI page.
pub struct McpSettings {
    reveal: Entity<Reveal>,
    enabled: bool,
    port: Entity<InputState>,
    /// `None` while the port input doesn't hold a valid port.
    valid_port: Option<u16>,
    url: Entity<InputState>,
    token: Entity<InputState>,
    snippet: Entity<EditorState>,
    read_collections: bool,
    send_requests: bool,
    ask_before_sending: bool,
    edit_collections: bool,
    read_history: bool,
    _port_sub: Subscription,
}

impl McpSettings {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (min, max) = (*PORT_RANGE.start(), *PORT_RANGE.end());
        let port =
            cx.new(|cx| number_state(DEFAULT_PORT.into(), min.into(), max.into(), window, cx));
        let port_sub = cx.subscribe_in(&port, window, |this, port, event, window, cx| {
            if let InputEvent::Change = event {
                let text = port.read(cx).value();
                this.set_port(mcp::parse_port(&text), window, cx);
            }
        });
        Self {
            reveal,
            enabled: false,
            port,
            valid_port: Some(DEFAULT_PORT),
            url: cx
                .new(|cx| InputState::new(window, cx).default_value(mcp::server_url(DEFAULT_PORT))),
            token: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(PLACEHOLDER_TOKEN)
                    .masked(true)
            }),
            snippet: cx.new(|cx| snippet_state(window, cx)),
            read_collections: true,
            send_requests: true,
            ask_before_sending: true,
            edit_collections: false,
            read_history: false,
            _port_sub: port_sub,
        }
    }

    /// Keeps the URL and snippet at the last valid port.
    fn set_port(&mut self, port: Option<u16>, window: &mut Window, cx: &mut Context<Self>) {
        self.valid_port = port;
        if let Some(port) = port {
            self.url.update(cx, |state, cx| {
                state.set_value(mcp::server_url(port), window, cx)
            });
            self.snippet.update(cx, |state, cx| {
                state.set_value(mcp::client_config(port, PLACEHOLDER_TOKEN), window, cx)
            });
        }
        cx.notify();
    }

    fn render_server(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = !self.enabled;
        group(SettingsGroup::McpServer)
            .child(
                row(&self.reveal, Setting::McpEnabled, cx)
                    .description(
                        "Let AI assistants such as Claude Code use your collections, \
                         environments and history. Works without the AI features above.",
                    )
                    .control(switch(
                        Setting::McpEnabled,
                        self.enabled,
                        |this, on| this.enabled = on,
                        cx,
                    )),
            )
            .child(self.render_port(disabled, cx))
            .child(
                row(&self.reveal, Setting::McpUrl, cx).control(
                    Input::new(&self.url)
                        .aria_label("MCP server URL")
                        .readonly(true)
                        .disabled(disabled)
                        .w(px(240.)),
                ),
            )
            .child(self.render_token(disabled, cx))
    }

    fn render_port(&self, disabled: bool, cx: &App) -> SettingRow {
        let row = row(&self.reveal, Setting::McpPort, cx).control(
            NumberInput::new(&self.port)
                .accessibility_label("MCP server port")
                .disabled(disabled)
                .w(px(120.)),
        );
        if self.valid_port.is_some() {
            row.description("Only reachable from this computer.")
        } else {
            let (min, max) = (PORT_RANGE.start(), PORT_RANGE.end());
            row.warning(format!("Enter a port from {min} to {max}."))
        }
    }

    fn render_token(&self, disabled: bool, cx: &App) -> SettingRow {
        row(&self.reveal, Setting::McpToken, cx)
            .description("Assistants send it with every request.")
            .control(
                h_flex()
                    .gap_2()
                    .child(
                        Input::new(&self.token)
                            .aria_label("Access token")
                            .readonly(true)
                            .disabled(disabled)
                            .w(px(140.)),
                    )
                    .child(unavailable_button("mcp-copy-token", "Copy"))
                    .child(unavailable_button("mcp-regenerate-token", "Regenerate")),
            )
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = !self.enabled;
        let reveal = &self.reveal;
        group(SettingsGroup::McpPermissions)
            .child(
                row(reveal, Setting::McpReadCollections, cx).control(
                    switch(
                        Setting::McpReadCollections,
                        self.read_collections,
                        |this, on| this.read_collections = on,
                        cx,
                    )
                    .disabled(disabled),
                ),
            )
            .child(
                row(reveal, Setting::McpSendRequests, cx).control(
                    switch(
                        Setting::McpSendRequests,
                        self.send_requests,
                        |this, on| this.send_requests = on,
                        cx,
                    )
                    .disabled(disabled),
                ),
            )
            .child(
                row(reveal, Setting::McpAskBeforeSending, cx)
                    .description("Confirm each request an assistant wants to send.")
                    .control(
                        switch(
                            Setting::McpAskBeforeSending,
                            self.ask_before_sending,
                            |this, on| this.ask_before_sending = on,
                            cx,
                        )
                        .disabled(disabled || !self.send_requests),
                    ),
            )
            .child(
                row(reveal, Setting::McpEditCollections, cx)
                    .description("Create, change and delete saved requests.")
                    .control(
                        switch(
                            Setting::McpEditCollections,
                            self.edit_collections,
                            |this, on| this.edit_collections = on,
                            cx,
                        )
                        .disabled(disabled),
                    ),
            )
            .child(self.render_read_history(disabled, cx))
    }

    fn render_read_history(&self, disabled: bool, cx: &mut Context<Self>) -> SettingRow {
        row(&self.reveal, Setting::McpReadHistory, cx)
            .description("History stores sent requests with their variables filled in.")
            .when(self.read_history, |row| {
                row.warning("Assistants can see tokens and passwords from sent requests.")
            })
            .control(
                switch(
                    Setting::McpReadHistory,
                    self.read_history,
                    |this, on| this.read_history = on,
                    cx,
                )
                .disabled(disabled),
            )
    }

    fn render_client_setup(&self, cx: &App) -> impl IntoElement {
        let disabled = !self.enabled;
        group(SettingsGroup::McpClientSetup)
            .footer("The MCP server isn't available yet.")
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        row(&self.reveal, Setting::McpClientConfig, cx)
                            .description(
                                "Add this to your assistant's MCP settings, \
                                 such as .mcp.json for Claude Code.",
                            )
                            .control(unavailable_button("mcp-copy-config", "Copy")),
                    )
                    .child(
                        Editor::new(&self.snippet)
                            .aria_label("Client configuration")
                            .readonly(true)
                            .disabled(disabled)
                            .h(px(240.)),
                    ),
            )
    }
}

fn snippet_state(window: &mut Window, cx: &mut Context<EditorState>) -> EditorState {
    EditorState::new(window, cx)
        .language("json")
        .line_number(false)
        .folding(false)
        .searchable(false)
        .default_value(mcp::client_config(DEFAULT_PORT, PLACEHOLDER_TOKEN))
}

fn unavailable_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id)
        .outline()
        .label(label)
        .tooltip(NOT_AVAILABLE)
        .disabled(true)
}

impl Render for McpSettings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_server(cx))
            .child(self.render_permissions(cx))
            .child(self.render_client_setup(cx))
    }
}
