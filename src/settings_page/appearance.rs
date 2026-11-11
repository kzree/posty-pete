use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, px};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState, NumberInput},
    radio::RadioGroup,
};

use super::{group, number_state, page, switch};
use crate::ui::setting_row::SettingRow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

pub struct AppearancePage {
    theme: ThemeChoice,
    ui_font_size: Entity<InputState>,
    editor_font_family: Entity<InputState>,
    editor_font_size: Entity<InputState>,
    wrap_responses: bool,
}

impl AppearancePage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = cx.theme();
        let ui_font_size = theme.font_size.as_f32() as u32;
        let editor_font_size = theme.mono_font_size.as_f32() as u32;
        let editor_font_family = theme.mono_font_family.clone();
        Self {
            // The app always starts in dark mode for now.
            theme: ThemeChoice::Dark,
            ui_font_size: cx.new(|cx| number_state(ui_font_size, 10, 24, window, cx)),
            editor_font_family: cx
                .new(|cx| InputState::new(window, cx).default_value(editor_font_family)),
            editor_font_size: cx.new(|cx| number_state(editor_font_size, 8, 32, window, cx)),
            wrap_responses: true,
        }
    }

    fn render_theme(&self, cx: &mut Context<Self>) -> RadioGroup {
        let selected = ThemeChoice::ALL.iter().position(|t| *t == self.theme);
        RadioGroup::horizontal("theme")
            .children(ThemeChoice::ALL.map(ThemeChoice::label))
            .selected_index(selected)
            .on_change(cx.listener(|this, index: &usize, _, cx| {
                this.theme = ThemeChoice::ALL[*index];
                cx.notify();
            }))
    }
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(
                group("appearance-interface", "Interface")
                    .child(SettingRow::new("Theme").control(self.render_theme(cx)))
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
