use std::ops::RangeInclusive;

use gpui::{
    App, AppContext, Context, Entity, Hsla, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, prelude::FluentBuilder, px, rgb,
};
use gpui_component::{
    input::{Input, InputEvent, InputState, NumberInput},
    radio::RadioGroup,
    switch::Switch,
};

use super::{Reveal, group, number_state, page, row};
use crate::domain::{
    accent::Accent,
    settings::{AppearanceSettings, ThemePreference},
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

/// A typed font size, or `None` while it isn't a number in `range`, so
/// partly typed sizes don't resize the app on the way.
fn parse_font_size(text: &str, range: &RangeInclusive<u32>) -> Option<u32> {
    text.trim().parse().ok().filter(|size| range.contains(size))
}

fn appearance(cx: &App) -> &AppearanceSettings {
    &AppSettings::get(cx).appearance
}

pub struct AppearancePage {
    reveal: Entity<Reveal>,
    ui_font_size: Entity<InputState>,
    editor_font_family: Entity<InputState>,
    editor_font_size: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl AppearancePage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = appearance(cx).clone();
        let (ui_range, editor_range) = (
            AppearanceSettings::UI_FONT_SIZE,
            AppearanceSettings::EDITOR_FONT_SIZE,
        );
        let ui_font_size = cx.new(|cx| {
            number_state(
                settings.ui_font_size(),
                *ui_range.start(),
                *ui_range.end(),
                window,
                cx,
            )
        });
        let editor_font_family = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(theme::default_mono_font_family())
                .default_value(settings.editor_font_family.clone())
        });
        let editor_font_size = cx.new(|cx| {
            number_state(
                settings.editor_font_size(),
                *editor_range.start(),
                *editor_range.end(),
                window,
                cx,
            )
        });
        let _subscriptions = vec![
            on_change(&ui_font_size, cx, |s, text| {
                if let Some(size) = parse_font_size(text, &AppearanceSettings::UI_FONT_SIZE) {
                    s.ui_font_size = size;
                }
            }),
            on_change(&editor_font_family, cx, |s, text| {
                s.editor_font_family = text.to_owned()
            }),
            on_change(&editor_font_size, cx, |s, text| {
                if let Some(size) = parse_font_size(text, &AppearanceSettings::EDITOR_FONT_SIZE) {
                    s.editor_font_size = size;
                }
            }),
            cx.observe_global::<AppSettings>(|_, cx| cx.notify()),
        ];
        Self {
            reveal,
            ui_font_size,
            editor_font_family,
            editor_font_size,
            _subscriptions,
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

/// Applies `set` to the appearance settings with the input's text on every edit.
fn on_change(
    input: &Entity<InputState>,
    cx: &mut Context<AppearancePage>,
    set: fn(&mut AppearanceSettings, &str),
) -> Subscription {
    cx.subscribe(input, move |_, input, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
            let text = input.read(cx).value();
            AppSettings::update(cx, |s| set(&mut s.appearance, &text));
        }
    })
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
        let settings = appearance(cx);
        let wrap_responses = settings.wrap_responses;
        let missing_font = settings
            .editor_font_family()
            .filter(|family| !theme::is_font_installed(family, cx))
            .map(|family| {
                format!(
                    "\u{201c}{family}\u{201d} isn't installed, so {} is used.",
                    theme::default_mono_font_family()
                )
            });
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
                            .when_some(missing_font, |row, error| row.error(error))
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
                    .child(
                        row(reveal, Setting::WrapResponses, cx).control(
                            Switch::new(Setting::WrapResponses.id())
                                .checked(wrap_responses)
                                .accessibility_label(Setting::WrapResponses.label())
                                .on_change(|on: &bool, _, cx| {
                                    let on = *on;
                                    AppSettings::update(cx, |s| s.appearance.wrap_responses = on);
                                }),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_sizes_out_of_range_are_ignored_while_typing() {
        let range = AppearanceSettings::UI_FONT_SIZE;
        assert_eq!(parse_font_size(" 14 ", &range), Some(14));
        assert_eq!(parse_font_size("1", &range), None);
        assert_eq!(parse_font_size("240", &range), None);
        assert_eq!(parse_font_size("", &range), None);
        assert_eq!(parse_font_size("-12", &range), None);
    }
}
