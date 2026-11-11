use gpui::{Context, IntoElement, ParentElement, Render, Window};
use gpui_component::Disableable;

use super::{group, page, switch};
use crate::ui::setting_row::SettingRow;

pub struct GeneralPage {
    restore_session: bool,
    confirm_discard: bool,
    check_for_updates: bool,
}

impl GeneralPage {
    pub fn new() -> Self {
        Self {
            restore_session: true,
            confirm_discard: true,
            check_for_updates: false,
        }
    }
}

impl Render for GeneralPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(
                group("general-startup", "Startup")
                    .child(
                        SettingRow::new("Restore last session")
                            .description("Reopen the page and request you had open.")
                            .control(switch(
                                "restore-session",
                                "Restore last session",
                                self.restore_session,
                                |this, on| this.restore_session = on,
                                cx,
                            )),
                    )
                    .child(
                        SettingRow::new("Check for updates")
                            .description("Not available yet.")
                            .control(
                                switch(
                                    "check-for-updates",
                                    "Check for updates",
                                    self.check_for_updates,
                                    |this, on| this.check_for_updates = on,
                                    cx,
                                )
                                .disabled(true),
                            ),
                    ),
            )
            .child(
                group("general-editing", "Editing").child(
                    SettingRow::new("Ask before discarding changes")
                        .description("Confirm before closing a request with unsaved edits.")
                        .control(switch(
                            "confirm-discard",
                            "Ask before discarding changes",
                            self.confirm_discard,
                            |this, on| this.confirm_discard = on,
                            cx,
                        )),
                ),
            )
    }
}
