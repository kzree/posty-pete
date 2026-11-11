use std::time::Duration;

use gpui::{App, Entity, Task, Window};

use crate::{domain::request::KeyValue, ui::key_value_editor::KeyValueEditor};

/// How long after the last edit variables are saved.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// A variables editor that saves after a delay. Edits are emitted by the
/// view; this tracks what's pending and keeps reloads from clobbering typing.
/// `K` identifies the stored variables the editor holds.
pub struct AutoSavedVariables<K> {
    editor: Entity<KeyValueEditor>,
    /// The set the editor holds; `None` while it isn't shown.
    set: Option<K>,
    /// The stored variables the editor was loaded with or last emitted.
    synced: Vec<KeyValue>,
    /// Set from an edit until it's emitted.
    save: Option<Task<()>>,
}

impl<K: Copy + PartialEq> AutoSavedVariables<K> {
    pub fn new(editor: Entity<KeyValueEditor>) -> Self {
        Self {
            editor,
            set: None,
            synced: Vec::new(),
            save: None,
        }
    }

    pub fn editor(&self) -> &Entity<KeyValueEditor> {
        &self.editor
    }

    pub fn set(&self) -> Option<K> {
        self.set
    }

    /// Loads `stored` for `set`. While the same set is being edited, the
    /// editor wins; its next save catches up. Take the pending edit before
    /// switching sets, or it's lost.
    pub fn load(&mut self, set: Option<K>, stored: &[KeyValue], window: &mut Window, cx: &mut App) {
        let same_set = self.set == set;
        if !same_set {
            self.save = None;
        }
        if reload_variables(same_set, self.save.is_some(), stored, &self.synced) {
            self.editor
                .update(cx, |editor, cx| editor.set_values(stored, window, cx));
            self.synced = stored.to_vec();
        }
        self.set = set;
    }

    /// Replaces the timer of an earlier edit.
    pub fn schedule(&mut self, save: Task<()>) {
        self.save = Some(save);
    }

    /// Takes the edit waiting for its save delay. `None` if nothing changed.
    pub fn take_pending(&mut self, cx: &App) -> Option<(K, Vec<KeyValue>)> {
        drop(self.save.take()?);
        let set = self.set?;
        let variables = self.editor.read(cx).values(cx);
        if variables == self.synced {
            return None;
        }
        self.synced = variables.clone();
        Some((set, variables))
    }
}

/// Whether to load the stored variables into the editor. While the same set
/// is being edited, the editor wins.
fn reload_variables(
    same_set: bool,
    editing: bool,
    stored: &[KeyValue],
    synced: &[KeyValue],
) -> bool {
    !same_set || (!editing && stored != synced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn reloads_for_another_set() {
        let vars = [variable("host", "a")];
        assert!(reload_variables(false, false, &vars, &vars));
        assert!(reload_variables(false, true, &vars, &vars));
    }

    #[test]
    fn keeps_the_editor_when_the_stored_variables_are_what_it_saved() {
        let vars = [variable("host", "a")];
        assert!(!reload_variables(true, false, &vars, &vars));
        assert!(!reload_variables(true, true, &vars, &vars));
    }

    #[test]
    fn keeps_the_editor_while_editing_even_if_the_stored_variables_differ() {
        let stored = [variable("host", "a")];
        let synced = [variable("host", "b")];
        assert!(!reload_variables(true, true, &stored, &synced));
    }

    #[test]
    fn reloads_when_the_stored_variables_changed_elsewhere() {
        let stored = [variable("host", "a")];
        let synced = [variable("host", "b")];
        assert!(reload_variables(true, false, &stored, &synced));
    }
}
