use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable,
    button::Button,
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState, NumberInput},
    v_flex,
};

use super::{group, number_state, page, switch};
use crate::{
    domain::mcp::{self, DEFAULT_PORT, PLACEHOLDER_TOKEN, PORT_RANGE},
    ui::setting_row::SettingRow,
};

const NOT_AVAILABLE: &str = "Not available yet";

/// The MCP server's settings, shown on the AI page.
pub struct McpSettings {
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
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        group("mcp-server", "MCP server")
            .child(
                SettingRow::new("Enable MCP server")
                    .description(
                        "Let AI assistants such as Claude Code use your collections, \
                         environments and history. Works without the AI features above.",
                    )
                    .control(switch(
                        "mcp-enabled",
                        "Enable MCP server",
                        self.enabled,
                        |this, on| this.enabled = on,
                        cx,
                    )),
            )
            .child(self.render_port(disabled))
            .child(
                SettingRow::new("URL").control(
                    Input::new(&self.url)
                        .aria_label("MCP server URL")
                        .readonly(true)
                        .disabled(disabled)
                        .w(px(240.)),
                ),
            )
            .child(self.render_token(disabled))
    }

    fn render_port(&self, disabled: bool) -> SettingRow {
        let row = SettingRow::new("Port").control(
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

    fn render_token(&self, disabled: bool) -> SettingRow {
        SettingRow::new("Access token")
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
        group("mcp-permissions", "MCP permissions")
            .child(
                SettingRow::new("Read collections and environments").control(
                    switch(
                        "mcp-read-collections",
                        "Read collections and environments",
                        self.read_collections,
                        |this, on| this.read_collections = on,
                        cx,
                    )
                    .disabled(disabled),
                ),
            )
            .child(
                SettingRow::new("Send requests").control(
                    switch(
                        "mcp-send-requests",
                        "Send requests",
                        self.send_requests,
                        |this, on| this.send_requests = on,
                        cx,
                    )
                    .disabled(disabled),
                ),
            )
            .child(
                SettingRow::new("Ask before sending")
                    .description("Confirm each request an assistant wants to send.")
                    .control(
                        switch(
                            "mcp-ask-before-sending",
                            "Ask before sending",
                            self.ask_before_sending,
                            |this, on| this.ask_before_sending = on,
                            cx,
                        )
                        .disabled(disabled || !self.send_requests),
                    ),
            )
            .child(
                SettingRow::new("Edit collections")
                    .description("Create, change and delete saved requests.")
                    .control(
                        switch(
                            "mcp-edit-collections",
                            "Edit collections",
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
        SettingRow::new("Read history")
            .description("History stores sent requests with their variables filled in.")
            .when(self.read_history, |row| {
                row.warning("Assistants can see tokens and passwords from sent requests.")
            })
            .control(
                switch(
                    "mcp-read-history",
                    "Read history",
                    self.read_history,
                    |this, on| this.read_history = on,
                    cx,
                )
                .disabled(disabled),
            )
    }

    fn render_client_setup(&self) -> impl IntoElement {
        let disabled = !self.enabled;
        group("mcp-client-setup", "Client setup")
            .footer("The MCP server isn't available yet.")
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        SettingRow::new("Client configuration")
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
            .child(self.render_client_setup())
    }
}
