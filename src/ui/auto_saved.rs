use std::time::Duration;

use gpui::{App, Context, Entity, Task, Window};

use crate::{
    domain::request::{Auth, KeyValue},
    ui::{auth_editor::AuthEditor, key_value_editor::KeyValueEditor},
};

/// How long after the last edit changes are saved.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// An editor whose value [`AutoSaved`] can load and read back.
pub trait AutoSaveEditor: 'static {
    type Value: Clone + Default + PartialEq;

    fn value(&self, cx: &App) -> Self::Value;
    fn set_value(&mut self, value: &Self::Value, window: &mut Window, cx: &mut Context<Self>)
    where
        Self: Sized;
}

impl AutoSaveEditor for KeyValueEditor {
    type Value = Vec<KeyValue>;

    fn value(&self, cx: &App) -> Vec<KeyValue> {
        self.values(cx)
    }

    fn set_value(&mut self, value: &Vec<KeyValue>, window: &mut Window, cx: &mut Context<Self>) {
        self.set_values(value, window, cx)
    }
}

impl AutoSaveEditor for AuthEditor {
    type Value = Auth;

    fn value(&self, cx: &App) -> Auth {
        self.auth(cx)
    }

    fn set_value(&mut self, value: &Auth, window: &mut Window, cx: &mut Context<Self>) {
        self.set_auth(value, window, cx)
    }
}

pub type AutoSavedVariables<K> = AutoSaved<K, KeyValueEditor>;
pub type AutoSavedAuth<K> = AutoSaved<K, AuthEditor>;

/// An editor that saves after a delay. Edits are emitted by the view; this
/// tracks what's pending and keeps reloads from clobbering typing.
/// `K` identifies the stored value the editor holds.
pub struct AutoSaved<K, E: AutoSaveEditor> {
    editor: Entity<E>,
    /// The stored value the editor holds; `None` while it isn't shown.
    set: Option<K>,
    /// The stored value the editor was loaded with or last emitted.
    synced: E::Value,
    /// Set from an edit until it's emitted.
    save: Option<Task<()>>,
}

impl<K: Copy + PartialEq, E: AutoSaveEditor> AutoSaved<K, E> {
    pub fn new(editor: Entity<E>) -> Self {
        Self {
            editor,
            set: None,
            synced: E::Value::default(),
            save: None,
        }
    }

    pub fn editor(&self) -> &Entity<E> {
        &self.editor
    }

    pub fn set(&self) -> Option<K> {
        self.set
    }

    /// Loads `stored` for `set`. While the same set is being edited, the
    /// editor wins; its next save catches up. Take the pending edit before
    /// switching sets, or it's lost.
    pub fn load(&mut self, set: Option<K>, stored: &E::Value, window: &mut Window, cx: &mut App) {
        let same_set = self.set == set;
        if !same_set {
            self.save = None;
        }
        if reload(same_set, self.save.is_some(), stored, &self.synced) {
            self.editor
                .update(cx, |editor, cx| editor.set_value(stored, window, cx));
            self.synced = stored.clone();
        }
        self.set = set;
    }

    /// Replaces the timer of an earlier edit.
    pub fn schedule(&mut self, save: Task<()>) {
        self.save = Some(save);
    }

    /// Takes the edit waiting for its save delay. `None` if nothing changed.
    pub fn take_pending(&mut self, cx: &App) -> Option<(K, E::Value)> {
        drop(self.save.take()?);
        let set = self.set?;
        let value = self.editor.read(cx).value(cx);
        if value == self.synced {
            return None;
        }
        self.synced = value.clone();
        Some((set, value))
    }
}

/// Whether to load the stored value into the editor. While the same set is
/// being edited, the editor wins.
fn reload<T: PartialEq + ?Sized>(same_set: bool, editing: bool, stored: &T, synced: &T) -> bool {
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
        assert!(reload(false, false, &vars, &vars));
        assert!(reload(false, true, &vars, &vars));
    }

    #[test]
    fn keeps_the_editor_when_the_stored_value_is_what_it_saved() {
        let vars = [variable("host", "a")];
        assert!(!reload(true, false, &vars, &vars));
        assert!(!reload(true, true, &vars, &vars));
    }

    #[test]
    fn keeps_the_editor_while_editing_even_if_the_stored_value_differs() {
        let stored = [variable("host", "a")];
        let synced = [variable("host", "b")];
        assert!(!reload(true, true, &stored, &synced));
    }

    #[test]
    fn reloads_when_the_stored_value_changed_elsewhere() {
        let stored = [variable("host", "a")];
        let synced = [variable("host", "b")];
        assert!(reload(true, false, &stored, &synced));
    }

    #[test]
    fn reloads_auth_changed_elsewhere_only_when_not_editing() {
        let stored = Auth::Bearer { token: "a".into() };
        let synced = Auth::None;
        assert!(reload(true, false, &stored, &synced));
        assert!(!reload(true, true, &stored, &synced));
        assert!(!reload(true, false, &synced, &synced));
    }
}
