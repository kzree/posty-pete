//! Commands the palette can run: their titles, search keywords and when they apply.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    NewRequest,
    Save,
    SaveAs,
    SendRequest,
    Revert,
    NewCollection,
    NewEnvironment,
    NewSharedEnvironment,
    Back,
    Forward,
    FocusUrl,
    ToggleSidebar,
    Library,
    History,
    Tests,
    SharedEnvironments,
    Settings,
    Quit,
}

impl Command {
    /// In palette order.
    pub const ALL: [Self; 18] = [
        Self::NewRequest,
        Self::Save,
        Self::SaveAs,
        Self::SendRequest,
        Self::Revert,
        Self::NewCollection,
        Self::NewEnvironment,
        Self::NewSharedEnvironment,
        Self::Back,
        Self::Forward,
        Self::FocusUrl,
        Self::ToggleSidebar,
        Self::Library,
        Self::History,
        Self::Tests,
        Self::SharedEnvironments,
        Self::Settings,
        Self::Quit,
    ];

    /// The label Settings › Shortcuts uses, where it lists the command.
    pub fn title(self) -> &'static str {
        match self {
            Self::NewRequest => "New request",
            Self::Save => "Save",
            Self::SaveAs => "Save as",
            Self::SendRequest => "Send request",
            Self::Revert => "Revert changes",
            Self::NewCollection => "New collection",
            Self::NewEnvironment => "New environment",
            Self::NewSharedEnvironment => "New shared environment",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::FocusUrl => "Focus URL",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::Library => "Library",
            Self::History => "History",
            Self::Tests => "Tests",
            Self::SharedEnvironments => "Shared environments",
            Self::Settings => "Settings",
            Self::Quit => "Quit",
        }
    }

    /// Other words people search for it by.
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::NewRequest => &["create", "add", "blank", "draft"],
            Self::Save => &["request", "store"],
            Self::SaveAs => &["copy", "duplicate", "request"],
            Self::SendRequest => &["run", "execute"],
            Self::Revert => &["discard", "undo", "reset"],
            Self::NewCollection => &["create", "add"],
            Self::NewEnvironment => &["create", "add", "variables"],
            Self::NewSharedEnvironment => &["create", "add", "variables", "global"],
            Self::Back => &["go", "previous", "navigate"],
            Self::Forward => &["go", "next", "navigate"],
            Self::FocusUrl => &["address", "location"],
            Self::ToggleSidebar => &["panel", "show", "hide"],
            Self::Library => &["collections", "requests", "sidebar", "show"],
            Self::History => &["recent", "sidebar", "show"],
            Self::Tests => &["sidebar", "show"],
            Self::SharedEnvironments => &["variables", "global", "open"],
            Self::Settings => &["preferences", "options", "open"],
            Self::Quit => &["exit", "close"],
        }
    }

    /// Whether it runs a key-bound action rather than a Workspace call.
    pub fn has_action(self) -> bool {
        !matches!(
            self,
            Self::NewCollection | Self::NewEnvironment | Self::NewSharedEnvironment
        )
    }

    pub fn is_available(self, state: &CommandState) -> bool {
        match self {
            Self::Save => state.editor_shown && (state.draft || state.dirty),
            Self::SaveAs | Self::FocusUrl => state.editor_shown,
            Self::SendRequest => state.editor_shown && state.has_url && !state.sending,
            Self::Revert => state.editor_shown && state.dirty,
            Self::NewEnvironment => state.in_collection,
            Self::Back => state.can_go_back,
            Self::Forward => state.can_go_forward,
            Self::NewRequest
            | Self::NewCollection
            | Self::NewSharedEnvironment
            | Self::ToggleSidebar
            | Self::Library
            | Self::History
            | Self::Tests
            | Self::SharedEnvironments
            | Self::Settings
            | Self::Quit => true,
        }
    }
}

/// What decides which commands can run, read when the palette opens.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandState {
    /// The main pane shows the request editor.
    pub editor_shown: bool,
    /// The editor holds an unsaved draft rather than a saved request.
    pub draft: bool,
    /// The editor's saved request has unsaved changes.
    pub dirty: bool,
    pub has_url: bool,
    pub sending: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// The main pane shows a collection or one of its requests.
    pub in_collection: bool,
}

/// The commands that can run in `state`, in palette order.
pub fn available(state: &CommandState) -> impl Iterator<Item = Command> + '_ {
    Command::ALL
        .into_iter()
        .filter(|command| command.is_available(state))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn titles_are_unique() {
        let titles: HashSet<_> = Command::ALL.iter().map(|c| c.title()).collect();
        assert_eq!(titles.len(), Command::ALL.len());
    }

    #[test]
    fn all_lists_every_command_once() {
        let unique: HashSet<_> = Command::ALL.into_iter().collect();
        assert_eq!(unique.len(), Command::ALL.len());
    }

    #[test]
    fn keywords_are_lowercase_and_not_in_the_title() {
        for command in Command::ALL {
            let title = command.title().to_lowercase();
            for keyword in command.keywords() {
                assert_eq!(*keyword, keyword.to_lowercase(), "{command:?}");
                assert!(!title.contains(keyword), "{command:?} repeats {keyword:?}");
            }
        }
    }

    fn titles(state: &CommandState) -> Vec<&'static str> {
        available(state).map(Command::title).collect()
    }

    #[test]
    fn away_from_the_editor_only_general_commands_apply() {
        let state = CommandState {
            has_url: true,
            dirty: true,
            ..CommandState::default()
        };
        let titles = titles(&state);
        for hidden in [
            "Save",
            "Save as",
            "Send request",
            "Revert changes",
            "Focus URL",
        ] {
            assert!(!titles.contains(&hidden), "{hidden} shown");
        }
        assert!(titles.contains(&"New request"));
        assert!(titles.contains(&"Quit"));
    }

    #[test]
    fn save_needs_a_draft_or_changes() {
        let clean = CommandState {
            editor_shown: true,
            ..CommandState::default()
        };
        assert!(!Command::Save.is_available(&clean));
        assert!(!Command::Revert.is_available(&clean));
        assert!(Command::SaveAs.is_available(&clean));
        let dirty = CommandState {
            dirty: true,
            ..clean
        };
        assert!(Command::Save.is_available(&dirty));
        assert!(Command::Revert.is_available(&dirty));
        let draft = CommandState {
            draft: true,
            ..clean
        };
        assert!(Command::Save.is_available(&draft));
        assert!(!Command::Revert.is_available(&draft));
    }

    #[test]
    fn send_needs_a_url_and_no_request_in_flight() {
        let state = CommandState {
            editor_shown: true,
            ..CommandState::default()
        };
        assert!(!Command::SendRequest.is_available(&state));
        let ready = CommandState {
            has_url: true,
            ..state
        };
        assert!(Command::SendRequest.is_available(&ready));
        let sending = CommandState {
            sending: true,
            ..ready
        };
        assert!(!Command::SendRequest.is_available(&sending));
    }

    #[test]
    fn navigation_and_collection_commands_follow_state() {
        let none = CommandState::default();
        assert!(!Command::Back.is_available(&none));
        assert!(!Command::Forward.is_available(&none));
        assert!(!Command::NewEnvironment.is_available(&none));
        let all = CommandState {
            can_go_back: true,
            can_go_forward: true,
            in_collection: true,
            ..none
        };
        assert!(Command::Back.is_available(&all));
        assert!(Command::Forward.is_available(&all));
        assert!(Command::NewEnvironment.is_available(&all));
    }
}
