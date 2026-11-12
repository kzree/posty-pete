use domain::settings_index::{Setting, SettingsGroup};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, px,
};
use gpui_component::{
    Disableable, IndexPath,
    input::{Input, InputState},
    select::{Select, SelectState},
};

use super::{Reveal, group, mcp::McpSettings, page, row, switch};

const PROVIDERS: [&str; 1] = ["Anthropic"];
const MODELS: [&str; 3] = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];
const DEFAULT_MODEL: usize = 1;

type TextSelect = SelectState<Vec<&'static str>>;

pub struct AiPage {
    reveal: Entity<Reveal>,
    enabled: bool,
    provider: Entity<TextSelect>,
    api_key: Entity<InputState>,
    model: Entity<TextSelect>,
    explain_responses: bool,
    generate_requests: bool,
    mcp: Entity<McpSettings>,
}

impl AiPage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            mcp: cx.new(|cx| McpSettings::new(reveal.clone(), window, cx)),
            reveal,
            enabled: false,
            provider: cx.new(|cx| select_state(&PROVIDERS, 0, window, cx)),
            api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("sk-ant-…")
                    .masked(true)
            }),
            model: cx.new(|cx| select_state(&MODELS, DEFAULT_MODEL, window, cx)),
            explain_responses: true,
            generate_requests: true,
        }
    }

    fn render_master(&self, cx: &mut Context<Self>) -> impl IntoElement {
        group(SettingsGroup::Ai).child(
            row(&self.reveal, Setting::AiEnabled, cx)
                .description(
                    "Requests and responses you use them with are sent to the provider below.",
                )
                .control(switch(
                    Setting::AiEnabled,
                    self.enabled,
                    |this, on| this.enabled = on,
                    cx,
                )),
        )
    }

    fn render_provider(&self, cx: &App) -> impl IntoElement {
        let reveal = &self.reveal;
        group(SettingsGroup::Provider)
            .child(
                row(reveal, Setting::AiProvider, cx).control(
                    Select::new(&self.provider)
                        .accessibility_label("Provider")
                        .w(px(240.)),
                ),
            )
            .child(
                row(reveal, Setting::AiApiKey, cx)
                    .description("Not saved yet. It will be kept in the system keychain.")
                    .control(
                        Input::new(&self.api_key)
                            .aria_label("API key")
                            .mask_toggle()
                            .w(px(240.)),
                    ),
            )
            .child(
                row(reveal, Setting::AiModel, cx).control(
                    Select::new(&self.model)
                        .accessibility_label("Model")
                        .w(px(240.)),
                ),
            )
    }

    fn render_features(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = !self.enabled;
        let reveal = &self.reveal;
        group(SettingsGroup::Features)
            .footer("These features aren't available yet.")
            .child(
                row(reveal, Setting::ExplainResponses, cx)
                    .description("Summarize a response and point out errors.")
                    .control(
                        switch(
                            Setting::ExplainResponses,
                            self.explain_responses,
                            |this, on| this.explain_responses = on,
                            cx,
                        )
                        .disabled(disabled),
                    ),
            )
            .child(
                row(reveal, Setting::GenerateRequests, cx)
                    .description("Create a request from a description of what it should do.")
                    .control(
                        switch(
                            Setting::GenerateRequests,
                            self.generate_requests,
                            |this, on| this.generate_requests = on,
                            cx,
                        )
                        .disabled(disabled),
                    ),
            )
    }
}

fn select_state(
    items: &[&'static str],
    selected: usize,
    window: &mut Window,
    cx: &mut Context<TextSelect>,
) -> TextSelect {
    SelectState::new(items.to_vec(), Some(IndexPath::new(selected)), window, cx)
}

impl Render for AiPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_master(cx))
            .child(self.render_provider(cx))
            .child(self.render_features(cx))
            .child(self.mcp.clone())
    }
}
