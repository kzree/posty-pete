use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, px};
use gpui_component::{
    Disableable,
    button::Button,
    input::{InputState, NumberInput},
};

use super::{group, number_state, page};
use crate::{
    domain::{history::RETENTION_DAYS, store::data_dir},
    ui::setting_row::SettingRow,
};

const NOT_AVAILABLE: &str = "Not available yet";

pub struct HistoryAndDataPage {
    retention_days: Entity<InputState>,
    /// The data folder's path, or why it couldn't be found.
    data_dir: String,
}

impl HistoryAndDataPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let retention_days = RETENTION_DAYS as u32;
        Self {
            retention_days: cx.new(|cx| number_state(retention_days, 1, 365, window, cx)),
            data_dir: data_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|e| format!("Couldn't find the data folder: {e}.")),
        }
    }

    fn render_history(&self) -> impl IntoElement {
        group("history", "History")
            .footer(
                "History stores each sent request with its variables filled in, \
                 including tokens and passwords.",
            )
            .child(
                SettingRow::new("Keep history for")
                    .description("Number of days. Older requests are removed.")
                    .control(
                        NumberInput::new(&self.retention_days)
                            .accessibility_label("Days to keep history")
                            .w(px(120.)),
                    ),
            )
            .child(
                SettingRow::new("Clear history")
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

    fn render_data(&self) -> impl IntoElement {
        group("data", "Data").child(
            SettingRow::new("Data folder")
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_history())
            .child(self.render_data())
    }
}
