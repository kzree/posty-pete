use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window};
use gpui_component::Disableable;

use super::{Reveal, group, page, row, switch};
use crate::domain::settings_index::{Setting, SettingsGroup};

pub struct GeneralPage {
    reveal: Entity<Reveal>,
    restore_session: bool,
    confirm_discard: bool,
    check_for_updates: bool,
}

impl GeneralPage {
    pub fn new(reveal: Entity<Reveal>) -> Self {
        Self {
            reveal,
            restore_session: true,
            confirm_discard: true,
            check_for_updates: false,
        }
    }
}

impl Render for GeneralPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = &self.reveal;
        page()
            .child(
                group(SettingsGroup::Startup)
                    .child(
                        row(reveal, Setting::RestoreSession, cx)
                            .description("Reopen the page and request you had open.")
                            .control(switch(
                                Setting::RestoreSession,
                                self.restore_session,
                                |this, on| this.restore_session = on,
                                cx,
                            )),
                    )
                    .child(
                        row(reveal, Setting::CheckForUpdates, cx)
                            .description("Not available yet.")
                            .control(
                                switch(
                                    Setting::CheckForUpdates,
                                    self.check_for_updates,
                                    |this, on| this.check_for_updates = on,
                                    cx,
                                )
                                .disabled(true),
                            ),
                    ),
            )
            .child(
                group(SettingsGroup::Editing).child(
                    row(reveal, Setting::ConfirmDiscard, cx)
                        .description("Confirm before closing a request with unsaved edits.")
                        .control(switch(
                            Setting::ConfirmDiscard,
                            self.confirm_discard,
                            |this, on| this.confirm_discard = on,
                            cx,
                        )),
                ),
            )
    }
}
