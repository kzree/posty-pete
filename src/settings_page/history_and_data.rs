use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, px,
};
use gpui_component::{
    Disableable,
    button::Button,
    input::{InputState, NumberInput},
};

use super::{Reveal, group, number_state, page, row};
use crate::domain::{
    history::RETENTION_DAYS,
    settings_index::{Setting, SettingsGroup},
    store::data_dir,
};

const NOT_AVAILABLE: &str = "Not available yet";

pub struct HistoryAndDataPage {
    reveal: Entity<Reveal>,
    retention_days: Entity<InputState>,
    /// The data folder's path, or why it couldn't be found.
    data_dir: String,
}

impl HistoryAndDataPage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let retention_days = RETENTION_DAYS as u32;
        Self {
            reveal,
            retention_days: cx.new(|cx| number_state(retention_days, 1, 365, window, cx)),
            data_dir: data_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|e| format!("Couldn't find the data folder: {e}.")),
        }
    }

    fn render_history(&self, cx: &App) -> impl IntoElement {
        group(SettingsGroup::History)
            .footer(
                "History stores each sent request with its variables filled in, \
                 including tokens and passwords.",
            )
            .child(
                row(&self.reveal, Setting::HistoryRetention, cx)
                    .description("Number of days. Older requests are removed.")
                    .control(
                        NumberInput::new(&self.retention_days)
                            .accessibility_label("Days to keep history")
                            .w(px(120.)),
                    ),
            )
            .child(
                row(&self.reveal, Setting::ClearHistory, cx)
                    .description("Remove every request from history.")
                    .control(
                        Button::new("clear-history")
                            .outline()
                            .label("Clear…")
                            .tooltip(NOT_AVAILABLE)
                            .disabled(true),
                    ),
            )
    }

    fn render_data(&self, cx: &App) -> impl IntoElement {
        group(SettingsGroup::Data).child(
            row(&self.reveal, Setting::DataFolder, cx)
                .description(self.data_dir.clone())
                .control(
                    Button::new("show-data-folder")
                        .outline()
                        .label("Show in Finder")
                        .tooltip(NOT_AVAILABLE)
                        .disabled(true),
                ),
        )
    }
}

impl Render for HistoryAndDataPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_history(cx))
            .child(self.render_data(cx))
    }
}
