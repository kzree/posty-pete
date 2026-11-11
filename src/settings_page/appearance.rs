use gpui::{
    App, AppContext, Context, Entity, Hsla, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, px, rgb,
};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState, NumberInput},
    radio::RadioGroup,
};

use super::{Reveal, group, number_state, page, row, switch};
use crate::domain::{
    accent::Accent,
    settings::ThemePreference,
    settings_index::{Setting, SettingsGroup},
};
use crate::ui::color_swatches::ColorSwatches;
use crate::{settings::AppSettings, theme};

const THEMES: [ThemePreference; 3] = [
    ThemePreference::System,
    ThemePreference::Light,
    ThemePreference::Dark,
];

fn theme_label(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::System => "System",
        ThemePreference::Light => "Light",
        ThemePreference::Dark => "Dark",
    }
}

pub struct AppearancePage {
    reveal: Entity<Reveal>,
    ui_font_size: Entity<InputState>,
    editor_font_family: Entity<InputState>,
    editor_font_size: Entity<InputState>,
    wrap_responses: bool,
    _settings: Subscription,
}

impl AppearancePage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = cx.theme();
        let ui_font_size = theme.font_size.as_f32() as u32;
        let editor_font_size = theme.mono_font_size.as_f32() as u32;
        let editor_font_family = theme.mono_font_family.clone();
        Self {
            reveal,
            ui_font_size: cx.new(|cx| number_state(ui_font_size, 10, 24, window, cx)),
            editor_font_family: cx
                .new(|cx| InputState::new(window, cx).default_value(editor_font_family)),
            editor_font_size: cx.new(|cx| number_state(editor_font_size, 8, 32, window, cx)),
            wrap_responses: true,
            _settings: cx.observe_global::<AppSettings>(|_, cx| cx.notify()),
        }
    }

    fn render_theme(&self, cx: &App) -> RadioGroup {
        let current = AppSettings::get(cx).appearance.theme;
        RadioGroup::horizontal("theme")
            .children(THEMES.map(theme_label))
            .selected_index(THEMES.iter().position(|t| *t == current))
            .on_change(|index: &usize, _, cx| {
                let theme = THEMES[*index];
                AppSettings::update(cx, |s| s.appearance.theme = theme);
            })
    }

    fn render_accent(&self, cx: &App) -> ColorSwatches {
        let current = AppSettings::get(cx).appearance.accent;
        let selected = Accent::ALL.iter().position(|a| *a == current);
        Accent::ALL
            .into_iter()
            .fold(
                ColorSwatches::new("accent", Setting::Accent.label()),
                |swatches, accent| swatches.swatch(accent.name(), accent_color(accent, cx)),
            )
            .selected_index(selected)
            .on_change(|index: &usize, _, cx| {
                let accent = Accent::ALL[*index];
                AppSettings::update(cx, |s| s.appearance.accent = accent);
            })
    }
}

/// Monochrome shows the theme's own primary, near-black in light mode and near-white in dark.
fn accent_color(accent: Accent, cx: &App) -> Hsla {
    accent
        .hex()
        .map_or_else(|| theme::neutral_primary(cx), |hex| rgb(hex).into())
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = &self.reveal;
        page()
            .child(
                group(SettingsGroup::Interface)
                    .child(row(reveal, Setting::Theme, cx).control(self.render_theme(cx)))
                    .child(row(reveal, Setting::Accent, cx).control(self.render_accent(cx)))
                    .child(
                        row(reveal, Setting::UiFontSize, cx).control(
                            NumberInput::new(&self.ui_font_size)
                                .accessibility_label("Font size")
                                .w(px(120.)),
                        ),
                    ),
            )
            .child(
                group(SettingsGroup::Editors)
                    .child(
                        row(reveal, Setting::EditorFont, cx)
                            .description("Used by the URL, body and response editors.")
                            .control(
                                Input::new(&self.editor_font_family)
                                    .aria_label("Editor font")
                                    .w(px(200.)),
                            ),
                    )
                    .child(
                        row(reveal, Setting::EditorFontSize, cx).control(
                            NumberInput::new(&self.editor_font_size)
                                .accessibility_label("Editor font size")
                                .w(px(120.)),
                        ),
                    )
                    .child(row(reveal, Setting::WrapResponses, cx).control(switch(
                        Setting::WrapResponses,
                        self.wrap_responses,
                        |this, on| this.wrap_responses = on,
                        cx,
                    ))),
            )
    }
}
