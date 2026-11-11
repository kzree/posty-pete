use gpui::{App, Context, FocusHandle, Window};
use uuid::Uuid;

use super::{EditorContent, Workspace, navigation::Direction, routes::AppRoute};
use crate::{
    actions,
    domain::command::{Command, CommandState},
};

impl Workspace {
    /// What decides which commands run, for the palette and for registering
    /// actions. `dirty` comes from `is_dirty` or `is_dirty_now`.
    pub(super) fn command_state(&self, dirty: bool, cx: &App) -> CommandState {
        CommandState {
            editor_shown: self.current_route() == self.editor_route(),
            draft: self.editor_content == EditorContent::Draft,
            dirty,
            has_url: self.request_editor.read(cx).has_url(cx),
            sending: self.pending.is_some(),
            can_go_back: self.can_go(Direction::Back, cx),
            can_go_forward: self.can_go(Direction::Forward, cx),
            in_collection: self.current_collection().is_some(),
        }
    }

    /// The collection the main pane shows, or holds the request it shows.
    fn current_collection(&self) -> Option<Uuid> {
        match self.current_route() {
            AppRoute::Collection(id)
            | AppRoute::SavedRequest {
                collection_id: id, ..
            } => Some(id),
            AppRoute::Request | AppRoute::Settings | AppRoute::SharedEnvironments => None,
        }
    }

    /// Where commands run: the focused element, if it's in the workspace.
    pub(super) fn command_focus(&self, window: &Window, cx: &Context<Self>) -> FocusHandle {
        window
            .focused(cx)
            .filter(|_| self.focus_handle.contains_focused(window, cx))
            .unwrap_or_else(|| self.focus_handle.clone())
    }

    /// Runs `command` as if its shortcut was pressed where focus is. Call
    /// after the palette has closed and handed focus back.
    pub(super) fn run_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(action) = actions::action(command) {
            let focus = self.command_focus(window, cx);
            window.focus(&focus, cx);
            window.dispatch_action(action, cx);
            return;
        }
        match command {
            Command::NewCollection => self.create_collection(window, cx),
            Command::NewEnvironment => {
                if let Some(id) = self.current_collection() {
                    self.add_environment(id, window, cx);
                }
            }
            Command::NewSharedEnvironment => self.add_shared_environment(window, cx),
            _ => {}
        }
    }
}
