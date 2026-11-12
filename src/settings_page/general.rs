use domain::{
    settings::GeneralSettings,
    settings_index::{Setting, SettingsGroup},
};
use gpui::{Context, Entity, IntoElement, ParentElement, Render, Subscription, Window};
use gpui_component::{Disableable, switch::Switch};

use super::{Reveal, group, page, row};
use crate::settings::AppSettings;

pub struct GeneralPage {
    reveal: Entity<Reveal>,
    _settings: Subscription,
}

impl GeneralPage {
    pub fn new(reveal: Entity<Reveal>, cx: &mut Context<Self>) -> Self {
        Self {
            reveal,
            _settings: cx.observe_global::<AppSettings>(|_, cx| cx.notify()),
        }
    }
}

/// A switch for a general setting, named after `setting`.
fn switch(setting: Setting, checked: bool, set: fn(&mut GeneralSettings, bool)) -> Switch {
    Switch::new(setting.id())
        .checked(checked)
        .accessibility_label(setting.label())
        .on_change(move |checked: &bool, _, cx| {
            let checked = *checked;
            AppSettings::update(cx, |s| set(&mut s.general, checked));
        })
}

impl Render for GeneralPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = &self.reveal;
        let settings = AppSettings::get(cx).general.clone();
        page()
            .child(
                group(SettingsGroup::Startup)
                    .child(
                        row(reveal, Setting::RestoreSession, cx)
                            .description("Reopen the page and request you had open.")
                            .control(switch(
                                Setting::RestoreSession,
                                settings.restore_last_session,
                                |s, on| s.restore_last_session = on,
                            )),
                    )
                    .child(
                        row(reveal, Setting::CheckForUpdates, cx)
                            .description("Not available yet.")
                            .control(
                                switch(Setting::CheckForUpdates, false, |_, _| {}).disabled(true),
                            ),
                    ),
            )
            .child(
                group(SettingsGroup::Editing).child(
                    row(reveal, Setting::ConfirmDiscard, cx)
                        .description("Confirm before closing a request with unsaved edits.")
                        .control(switch(
                            Setting::ConfirmDiscard,
                            settings.ask_before_discarding,
                            |s, on| s.ask_before_discarding = on,
                        )),
                ),
            )
    }
}
