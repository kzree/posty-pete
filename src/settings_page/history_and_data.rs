use std::path::PathBuf;

use domain::{
    settings::HistorySettings,
    settings_index::{Setting, SettingsGroup},
    store::data_dir,
};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, px,
};
use gpui_component::{
    Disableable,
    button::Button,
    input::{InputEvent, InputState, NumberInput, NumberInputEvent, NumberStep, StepAction},
};

use super::{Reveal, group, number_state, page, row};
use crate::settings::AppSettings;

const SHOW_DATA_FOLDER: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(target_os = "windows") {
    "Show in Explorer"
} else {
    "Open Folder"
};

/// Whether gpui can open a folder in the platform's file manager.
const CAN_SHOW_FOLDERS: bool = cfg!(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
));

/// The days to keep for the typed `text`, clamped into range, or `current`
/// when it isn't a number.
fn committed_days(text: &str, current: u32) -> u32 {
    let range = HistorySettings::RETENTION_DAYS;
    text.trim().parse::<u64>().map_or(current, |days| {
        days.clamp((*range.start()).into(), (*range.end()).into()) as u32
    })
}

fn stepped_days(text: &str, current: u32, action: StepAction) -> u32 {
    let days = committed_days(text, current);
    let days = match action {
        StepAction::Increment => days.saturating_add(1),
        StepAction::Decrement => days.saturating_sub(1),
    };
    committed_days(&days.to_string(), current)
}

pub enum HistoryAndDataEvent {
    /// The user asked to clear history; the confirmation is still to come.
    ClearHistory,
}

pub struct HistoryAndDataPage {
    reveal: Entity<Reveal>,
    retention_days: Entity<InputState>,
    data_dir: Result<PathBuf, String>,
    history_empty: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<HistoryAndDataEvent> for HistoryAndDataPage {}

impl HistoryAndDataPage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let days = AppSettings::get(cx).history.retention_days();
        let range = HistorySettings::RETENTION_DAYS;
        // Saved only when committed, since history is pruned as soon as it's
        // lowered. Without a step the input emits `Step` instead of stepping.
        let retention_days = cx.new(|cx| {
            let mut state = number_state(days, *range.start(), *range.end(), window, cx);
            state.set_step(None::<NumberStep>, window, cx);
            state
        });
        let _subscriptions = vec![
            cx.subscribe_in(
                &retention_days,
                window,
                |_, input, event: &InputEvent, window, cx| {
                    if let InputEvent::Blur | InputEvent::PressEnter { .. } = event {
                        commit_retention(input, committed_days, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &retention_days,
                window,
                |_, input, NumberInputEvent::Step(action): &NumberInputEvent, window, cx| {
                    let action = *action;
                    commit_retention(
                        input,
                        move |text, current| stepped_days(text, current, action),
                        window,
                        cx,
                    );
                },
            ),
        ];
        Self {
            reveal,
            retention_days,
            data_dir: data_dir().map_err(|e| format!("Couldn't find the data folder: {e}.")),
            history_empty: true,
            _subscriptions,
        }
    }

    pub fn set_history_empty(&mut self, empty: bool, cx: &mut Context<Self>) {
        if self.history_empty != empty {
            self.history_empty = empty;
            cx.notify();
        }
    }

    fn show_data_folder(&self, cx: &mut Context<Self>) {
        let Ok(dir) = self.data_dir.clone() else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let created = cx
                .background_spawn({
                    let dir = dir.clone();
                    async move { std::fs::create_dir_all(dir) }
                })
                .await;
            match created {
                Ok(()) => cx.update(|cx| cx.open_with_system(&dir)),
                Err(e) => tracing::warn!("couldn't create {}: {e}", dir.display()),
            }
        })
        .detach();
    }

    fn render_history(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .disabled(self.history_empty)
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(HistoryAndDataEvent::ClearHistory)
                            })),
                    ),
            )
    }

    fn render_data(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let description = match &self.data_dir {
            Ok(dir) => dir.display().to_string(),
            Err(error) => error.clone(),
        };
        group(SettingsGroup::Data).child(
            row(&self.reveal, Setting::DataFolder, cx)
                .description(description)
                .control(
                    Button::new("show-data-folder")
                        .outline()
                        .label(SHOW_DATA_FOLDER)
                        .disabled(!CAN_SHOW_FOLDERS || self.data_dir.is_err())
                        .on_click(cx.listener(|this, _, _, cx| this.show_data_folder(cx))),
                ),
        )
    }
}

/// Saves the days `days` makes from the field's text and the saved value,
/// and shows them in the field.
fn commit_retention(
    input: &Entity<InputState>,
    days: impl FnOnce(&str, u32) -> u32,
    window: &mut Window,
    cx: &mut App,
) {
    let current = AppSettings::get(cx).history.retention_days();
    let days = days(&input.read(cx).value(), current);
    input.update(cx, |input, cx| {
        if input.value() != days.to_string() {
            input.set_value(days.to_string(), window, cx);
        }
    });
    AppSettings::update(cx, |s| s.history.retention_days = days);
}

impl Render for HistoryAndDataPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_history(cx))
            .child(self.render_data(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_days_are_clamped_and_blanks_keep_the_saved_value() {
        assert_eq!(committed_days(" 30 ", 14), 30);
        assert_eq!(committed_days("365", 14), 365);
        assert_eq!(committed_days("0", 14), 1);
        assert_eq!(committed_days("366", 14), 365);
        assert_eq!(committed_days("99999999999", 14), 365);
        assert_eq!(committed_days("", 14), 14);
        assert_eq!(committed_days("-1", 14), 14);
    }

    #[test]
    fn steps_move_by_one_within_range() {
        assert_eq!(stepped_days("14", 14, StepAction::Increment), 15);
        assert_eq!(stepped_days("14", 14, StepAction::Decrement), 13);
        assert_eq!(stepped_days("1", 1, StepAction::Decrement), 1);
        assert_eq!(stepped_days("365", 365, StepAction::Increment), 365);
        assert_eq!(stepped_days("", 20, StepAction::Increment), 21);
        assert_eq!(stepped_days("900", 20, StepAction::Decrement), 364);
    }
}
