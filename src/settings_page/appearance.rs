use gpui::{
    App, AppContext, Context, Entity, Hsla, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, px, rgb,
};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState, NumberInput},
    radio::RadioGroup,
};

use super::{group, number_state, page, switch};
use crate::domain::{accent::Accent, settings::ThemePreference};
use crate::settings::AppSettings;
use crate::ui::{color_swatches::ColorSwatches, setting_row::SettingRow};

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
    accent: Accent,
    ui_font_size: Entity<InputState>,
    editor_font_family: Entity<InputState>,
    editor_font_size: Entity<InputState>,
    wrap_responses: bool,
    _settings: Subscription,
}

impl AppearancePage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = cx.theme();
        let ui_font_size = theme.font_size.as_f32() as u32;
        let editor_font_size = theme.mono_font_size.as_f32() as u32;
        let editor_font_family = theme.mono_font_family.clone();
        Self {
            accent: Accent::default(),
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

    fn render_accent(&self, cx: &mut Context<Self>) -> ColorSwatches {
        let selected = Accent::ALL.iter().position(|a| *a == self.accent);
        Accent::ALL
            .into_iter()
            .fold(
                ColorSwatches::new("accent", "Accent colour"),
                |swatches, accent| swatches.swatch(accent.name(), accent_color(accent, cx)),
            )
            .selected_index(selected)
            .on_change(cx.listener(|this, index: &usize, _, cx| {
                this.accent = Accent::ALL[*index];
                cx.notify();
            }))
    }
}

/// Monochrome follows the theme's primary, which is near-black in light mode and near-white in dark.
fn accent_color(accent: Accent, cx: &Context<AppearancePage>) -> Hsla {
    accent
        .hex()
        .map_or(cx.theme().primary, |hex| rgb(hex).into())
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(
                group("appearance-interface", "Interface")
                    .child(SettingRow::new("Theme").control(self.render_theme(cx)))
                    .child(SettingRow::new("Accent colour").control(self.render_accent(cx)))
                    .child(
                        SettingRow::new("Font size").control(
                            NumberInput::new(&self.ui_font_size)
                                .accessibility_label("Font size")
                                .w(px(120.)),
                        ),
                    ),
            )
            .child(
                group("appearance-editors", "Editors")
                    .child(
                        SettingRow::new("Font")
                            .description("Used by the URL, body and response editors.")
                            .control(
                                Input::new(&self.editor_font_family)
                                    .aria_label("Editor font")
                                    .w(px(200.)),
                            ),
                    )
                    .child(
                        SettingRow::new("Font size").control(
                            NumberInput::new(&self.editor_font_size)
                                .accessibility_label("Editor font size")
                                .w(px(120.)),
                        ),
                    )
                    .child(
                        SettingRow::new("Wrap long lines in responses").control(switch(
                            "wrap-responses",
                            "Wrap long lines in responses",
                            self.wrap_responses,
                            |this, on| this.wrap_responses = on,
                            cx,
                        )),
                    ),
            )
    }
}
