use gpui::{Action, App, KeyBinding, actions};

use domain::command::Command;

actions!(
    posty_pete,
    [
        SendRequest,
        CancelRequest,
        SaveRequest,
        SaveRequestAs,
        RevertRequest,
        NewRequest,
        ImportOpenApi,
        GoBack,
        GoForward,
        ToggleSidebar,
        OpenLibrary,
        OpenHistory,
        OpenTests,
        OpenSettings,
        OpenSharedEnvironments,
        FocusUrl,
        QuickOpen,
        OpenCommandPalette,
        DeleteHistoryEntry,
        SendHistoryEntryAgain,
        SelectPreviousLibraryRow,
        SelectNextLibraryRow,
        FormatJsonBody,
        Quit,
        Hide,
        HideOthers,
        ShowAll
    ]
);

/// The action a command dispatches, or `None` for one Workspace runs itself.
pub fn action(command: Command) -> Option<Box<dyn Action>> {
    let action: Box<dyn Action> = match command {
        Command::NewRequest => Box::new(NewRequest),
        Command::Save => Box::new(SaveRequest),
        Command::SaveAs => Box::new(SaveRequestAs),
        Command::SendRequest => Box::new(SendRequest),
        Command::CancelRequest => Box::new(CancelRequest),
        Command::Revert => Box::new(RevertRequest),
        Command::Back => Box::new(GoBack),
        Command::Forward => Box::new(GoForward),
        Command::FocusUrl => Box::new(FocusUrl),
        Command::ToggleSidebar => Box::new(ToggleSidebar),
        Command::Library => Box::new(OpenLibrary),
        Command::History => Box::new(OpenHistory),
        Command::Tests => Box::new(OpenTests),
        Command::SharedEnvironments => Box::new(OpenSharedEnvironments),
        Command::Settings => Box::new(OpenSettings),
        Command::Quit => Box::new(Quit),
        Command::ImportOpenApi => Box::new(ImportOpenApi),
        Command::NewCollection | Command::NewEnvironment | Command::NewSharedEnvironment => {
            return None;
        }
    };
    Some(action)
}

pub const WORKSPACE_CONTEXT: &str = "Workspace";
pub const HISTORY_PANEL_CONTEXT: &str = "HistoryPanel";
pub const BODY_EDITOR_CONTEXT: &str = "BodyEditor";
/// Outranks the list's own cmd-enter (`Confirm { secondary: true }`).
const HISTORY_LIST_CONTEXT: &str = "HistoryPanel > List";
pub const LIBRARY_PANEL_CONTEXT: &str = "LibraryPanel";
/// Outranks the list's own up/down, which stop on placeholder rows.
const LIBRARY_LIST_CONTEXT: &str = "LibraryPanel > List";
const INPUT_CONTEXT: &str = "Input";

// gpui-pre 0.3.11's `KeyBinding::new` can't parse "secondary-".
#[cfg(target_os = "macos")]
pub(crate) mod keys {
    pub const SEND: &str = "cmd-enter";
    pub const SAVE: &str = "cmd-s";
    pub const SAVE_AS: &str = "cmd-shift-s";
    pub const NEW_REQUEST: &str = "cmd-n";
    pub const GO_BACK: &[&str] = &["cmd-["];
    pub const GO_FORWARD: &[&str] = &["cmd-]"];
    pub const TOGGLE_SIDEBAR: &str = "cmd-b";
    pub const LIBRARY: &str = "cmd-1";
    pub const HISTORY: &str = "cmd-2";
    pub const HISTORY_ALT: &str = "cmd-y";
    pub const TESTS: &str = "cmd-3";
    pub const SETTINGS: &str = "cmd-,";
    pub const FOCUS_URL: &str = "cmd-l";
    pub const QUICK_OPEN: &str = "cmd-p";
    pub const OPEN_COMMAND_PALETTE: &str = "cmd-k";
    pub const DELETE_HISTORY_ENTRY: &[&str] = &["backspace", "delete"];
    pub const FORMAT_JSON_BODY: &str = "alt-shift-f";
    pub const QUIT: &str = "cmd-q";
    pub const HIDE: &str = "cmd-h";
    pub const HIDE_OTHERS: &str = "alt-cmd-h";
}
#[cfg(not(target_os = "macos"))]
pub(crate) mod keys {
    pub const SEND: &str = "ctrl-enter";
    pub const SAVE: &str = "ctrl-s";
    pub const SAVE_AS: &str = "ctrl-shift-s";
    pub const NEW_REQUEST: &str = "ctrl-n";
    // Last one is shown in tooltips.
    pub const GO_BACK: &[&str] = &["ctrl-[", "alt-left"];
    pub const GO_FORWARD: &[&str] = &["ctrl-]", "alt-right"];
    pub const TOGGLE_SIDEBAR: &str = "ctrl-b";
    pub const LIBRARY: &str = "ctrl-1";
    pub const HISTORY: &str = "ctrl-2";
    pub const HISTORY_ALT: &str = "ctrl-h";
    pub const TESTS: &str = "ctrl-3";
    pub const SETTINGS: &str = "ctrl-,";
    pub const FOCUS_URL: &str = "ctrl-l";
    pub const QUICK_OPEN: &str = "ctrl-p";
    pub const OPEN_COMMAND_PALETTE: &str = "ctrl-k";
    pub const DELETE_HISTORY_ENTRY: &[&str] = &["delete"];
    pub const FORMAT_JSON_BODY: &str = "alt-shift-f";
    pub const QUIT: &str = "ctrl-q";
}

/// Must run after `gpui_component::init` so these override the input bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(keys::SEND, SendRequest, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SEND, SendRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::SAVE, SaveRequest, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SAVE, SaveRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::SAVE_AS, SaveRequestAs, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SAVE_AS, SaveRequestAs, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::NEW_REQUEST, NewRequest, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::NEW_REQUEST, NewRequest, Some(INPUT_CONTEXT)),
        KeyBinding::new(keys::TOGGLE_SIDEBAR, ToggleSidebar, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::LIBRARY, OpenLibrary, Some(WORKSPACE_CONTEXT)),
        // The menu shows the first binding.
        KeyBinding::new(keys::HISTORY, OpenHistory, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::HISTORY_ALT, OpenHistory, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::TESTS, OpenTests, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::SETTINGS, OpenSettings, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::FOCUS_URL, FocusUrl, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::QUICK_OPEN, QuickOpen, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new(keys::QUICK_OPEN, QuickOpen, Some(INPUT_CONTEXT)),
        KeyBinding::new(
            keys::OPEN_COMMAND_PALETTE,
            OpenCommandPalette,
            Some(WORKSPACE_CONTEXT),
        ),
        KeyBinding::new(
            keys::OPEN_COMMAND_PALETTE,
            OpenCommandPalette,
            Some(INPUT_CONTEXT),
        ),
        KeyBinding::new(
            keys::FORMAT_JSON_BODY,
            FormatJsonBody,
            Some(BODY_EDITOR_CONTEXT),
        ),
        KeyBinding::new(keys::QUIT, Quit, None),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new(keys::HIDE, Hide, None),
        KeyBinding::new(keys::HIDE_OTHERS, HideOthers, None),
    ]);
    // Workspace only: the code editor keeps cmd/ctrl-[ and -] for outdent/indent,
    // and single-line inputs pass them on to these.
    cx.bind_keys(
        keys::GO_BACK
            .iter()
            .map(|key| KeyBinding::new(key, GoBack, Some(WORKSPACE_CONTEXT)))
            .chain(
                keys::GO_FORWARD
                    .iter()
                    .map(|key| KeyBinding::new(key, GoForward, Some(WORKSPACE_CONTEXT))),
            ),
    );
    cx.bind_keys(
        keys::DELETE_HISTORY_ENTRY
            .iter()
            .map(|key| KeyBinding::new(key, DeleteHistoryEntry, Some(HISTORY_PANEL_CONTEXT))),
    );
    cx.bind_keys(
        [HISTORY_PANEL_CONTEXT, HISTORY_LIST_CONTEXT]
            .map(|context| KeyBinding::new(keys::SEND, SendHistoryEntryAgain, Some(context))),
    );
    cx.bind_keys([
        KeyBinding::new("up", SelectPreviousLibraryRow, Some(LIBRARY_LIST_CONTEXT)),
        KeyBinding::new("down", SelectNextLibraryRow, Some(LIBRARY_LIST_CONTEXT)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_workspace_calls_lack_an_action() {
        let without: Vec<_> = Command::ALL
            .into_iter()
            .filter(|command| action(*command).is_none())
            .collect();
        assert_eq!(
            without,
            [
                Command::NewCollection,
                Command::NewEnvironment,
                Command::NewSharedEnvironment
            ]
        );
    }

    #[test]
    fn commands_dispatch_distinct_actions() {
        let mut names: Vec<&str> = Command::ALL
            .into_iter()
            .filter_map(action)
            .map(|action| action.name())
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
