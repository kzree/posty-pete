use std::{sync::Arc, time::Duration};

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    form::{field, v_form},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    tab::TabBar,
    v_flex,
};
use uuid::Uuid;

use crate::{
    domain::{collection::Collection, request::KeyValue},
    ui::key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
};

const VARIABLES_SAVE_DELAY: Duration = Duration::from_millis(500);

pub enum CollectionViewEvent {
    Rename { id: Uuid, name: String },
    VariablesChanged { id: Uuid, variables: Vec<KeyValue> },
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum CollectionTab {
    Settings,
    Variables,
}

impl CollectionTab {
    const ALL: [CollectionTab; 2] = [Self::Settings, Self::Variables];

    fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Variables => "Variables",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

pub struct CollectionView {
    /// The id last passed to `show`, kept when the collection is missing.
    id: Option<Uuid>,
    collection: Option<Arc<Collection>>,
    active_tab: CollectionTab,
    name: Entity<InputState>,
    variables: Entity<KeyValueEditor>,
    /// The stored variables the editor was loaded with or last emitted.
    synced_variables: Vec<KeyValue>,
    /// Set from a variables edit until it's emitted.
    variables_save: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CollectionViewEvent> for CollectionView {}

impl CollectionView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Collection name"));
        let name_sub = cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.save(window, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        });
        let variables = cx.new(|cx| {
            KeyValueEditor::with_column_labels("variable", "Variable", "Value", window, cx)
        });
        let variables_sub = cx.subscribe(&variables, |this, _, event, cx| match event {
            KeyValueEditorEvent::Changed => this.schedule_variables_save(cx),
        });
        Self {
            id: None,
            collection: None,
            active_tab: CollectionTab::Settings,
            name,
            variables,
            synced_variables: Vec::new(),
            variables_save: None,
            _subscriptions: vec![name_sub, variables_sub],
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        self.id
    }

    /// Shows `collection` (`None` if `id` doesn't exist). For the collection
    /// already shown, an unsaved edit of the name and variables being edited
    /// are kept. Switching collection emits a pending variables edit first.
    pub fn show(
        &mut self,
        id: Uuid,
        collection: Option<Arc<Collection>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown_name = self
            .collection
            .as_ref()
            .filter(|_| self.id == Some(id))
            .map(|c| c.name.clone());
        let new_name = collection.as_ref().map(|c| c.name.clone());

        let replace_input = match &shown_name {
            None => true,
            Some(shown) => new_name.as_ref() != Some(shown) && self.name_value(cx) == *shown,
        };
        let same_collection = self.id == Some(id);
        if !same_collection {
            self.active_tab = CollectionTab::Settings;
            self.emit_pending_variables(cx);
        }
        if replace_input {
            let value = new_name.unwrap_or_default();
            self.name
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let stored = collection
            .as_ref()
            .map_or(&[][..], |c| c.variables.as_slice());
        if reload_variables(
            same_collection,
            self.variables_save.is_some(),
            stored,
            &self.synced_variables,
        ) {
            self.variables
                .update(cx, |editor, cx| editor.set_values(stored, window, cx));
            self.synced_variables = stored.to_vec();
        }

        self.id = Some(id);
        self.collection = collection;
        cx.notify();
    }

    pub fn focus_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.active_tab = CollectionTab::Settings;
        self.name.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Takes the variables edit waiting for its save delay, with the id of
    /// the collection it belongs to. `None` if nothing changed.
    pub fn take_pending_variables(&mut self, cx: &App) -> Option<(Uuid, Vec<KeyValue>)> {
        drop(self.variables_save.take()?);
        let id = self.id?;
        let variables = self.variables.read(cx).values(cx);
        if variables == self.synced_variables {
            return None;
        }
        self.synced_variables = variables.clone();
        Some((id, variables))
    }

    fn schedule_variables_save(&mut self, cx: &mut Context<Self>) {
        self.variables_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VARIABLES_SAVE_DELAY).await;
            this.update(cx, |this, cx| this.emit_pending_variables(cx))
                .ok();
        }));
    }

    fn emit_pending_variables(&mut self, cx: &mut Context<Self>) {
        if let Some((id, variables)) = self.take_pending_variables(cx) {
            cx.emit(CollectionViewEvent::VariablesChanged { id, variables });
        }
    }

    fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Use variables as {{name}} in this collection's requests. Changes save automatically."),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.variables.clone()),
            )
    }

    fn name_value(&self, cx: &App) -> SharedString {
        self.name.read(cx).value()
    }

    /// The trimmed name to save, if it's non-empty and differs from the stored one.
    fn pending_name(&self, cx: &App) -> Option<String> {
        let collection = self.collection.as_ref()?;
        let value = self.name_value(cx);
        let name = value.trim();
        (!name.is_empty() && name != collection.name).then(|| name.to_string())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some(name)) = (self.id, self.pending_name(cx)) else {
            return;
        };
        if self.name_value(cx) != name {
            let trimmed = name.clone();
            self.name
                .update(cx, |input, cx| input.set_value(trimmed, window, cx));
        }
        cx.emit(CollectionViewEvent::Rename { id, name });
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_save = self.pending_name(cx).is_some();
        v_form().max_w(px(480.)).child(
            field().label("Name").child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.name).aria_label("Name")),
                    )
                    .child(
                        Button::new("collection-save")
                            .primary()
                            .label("Save")
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            ),
        )
    }

    fn render_not_found(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("collection-not-found")
            .role(Role::Group)
            .aria_label("Collection not found")
            .size_full()
            .items_start()
            .gap_2()
            .p_4()
            .child(
                Button::new("collection-back")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .label("Back")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CollectionViewEvent::Back))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Collection not found."),
            )
    }
}

impl Render for CollectionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(collection) = &self.collection else {
            return self.render_not_found(cx).into_any_element();
        };

        let tabs = TabBar::new("collection-tabs")
            .underline()
            .selected_index(self.active_tab.index())
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.active_tab = CollectionTab::ALL[*ix];
                cx.notify();
            }))
            .children(CollectionTab::ALL.map(CollectionTab::label));

        let content = div().flex_1().min_h_0().p_4();
        let content = match self.active_tab {
            CollectionTab::Settings => content.child(self.render_settings(cx)),
            CollectionTab::Variables => content.child(self.render_variables(cx)),
        };

        v_flex()
            .id("collection-view")
            .role(Role::Group)
            .aria_label("Collection")
            .size_full()
            .child(
                div()
                    .id("collection-heading")
                    .role(Role::Heading)
                    .aria_level(1)
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .text_lg()
                    .font_semibold()
                    .truncate()
                    .child(collection.name.clone()),
            )
            .child(div().px_4().child(tabs))
            .child(content)
            .into_any_element()
    }
}

/// Whether `show` should load the stored variables into the editor. While the
/// same collection is being edited, the editor wins; its next save catches up.
fn reload_variables(
    same_collection: bool,
    editing: bool,
    stored: &[KeyValue],
    synced: &[KeyValue],
) -> bool {
    !same_collection || (!editing && stored != synced)
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
    fn reloads_for_another_collection() {
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
