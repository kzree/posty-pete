mod auto_save;
mod environments;

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

use self::auto_save::AutoSavedVariables;
pub use self::auto_save::VariableSet;
use crate::{
    domain::{collection::Collection, request::KeyValue},
    ui::key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
};

const VARIABLES_SAVE_DELAY: Duration = Duration::from_millis(500);

pub enum CollectionViewEvent {
    Rename {
        id: Uuid,
        name: String,
    },
    VariablesChanged {
        set: VariableSet,
        variables: Vec<KeyValue>,
    },
    AddEnvironment {
        collection_id: Uuid,
    },
    RenameEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
        name: String,
    },
    DeleteEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
    },
    /// `environment_id: None` = no environment.
    UseEnvironment {
        collection_id: Uuid,
        environment_id: Option<Uuid>,
    },
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum CollectionTab {
    Settings,
    Variables,
    Environments,
}

impl CollectionTab {
    const ALL: [CollectionTab; 3] = [Self::Settings, Self::Variables, Self::Environments];

    fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Variables => "Variables",
            Self::Environments => "Environments",
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
    variables: AutoSavedVariables,
    /// The collection's active environment, as last passed to `show`.
    active_environment: Option<Uuid>,
    /// The environment shown in the Environments tab.
    selected_environment: Option<Uuid>,
    environment_variables: AutoSavedVariables,
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
        let variables = variables_editor(window, cx);
        let variables_sub = cx.subscribe(&variables, |this, _, event, cx| match event {
            KeyValueEditorEvent::Changed => {
                let save = this.save_after_delay(cx);
                this.variables.schedule(save);
            }
        });
        let environment_variables = variables_editor(window, cx);
        let environment_sub =
            cx.subscribe(&environment_variables, |this, _, event, cx| match event {
                KeyValueEditorEvent::Changed => {
                    let save = this.save_after_delay(cx);
                    this.environment_variables.schedule(save);
                }
            });
        Self {
            id: None,
            collection: None,
            active_tab: CollectionTab::Settings,
            name,
            variables: AutoSavedVariables::new(variables),
            active_environment: None,
            selected_environment: None,
            environment_variables: AutoSavedVariables::new(environment_variables),
            _subscriptions: vec![name_sub, variables_sub, environment_sub],
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        self.id
    }

    /// Shows `collection` (`None` if `id` doesn't exist) with
    /// `active_environment` marked. For the collection already shown, an
    /// unsaved edit of the name and variables being edited are kept.
    /// Switching collection emits pending variables edits first.
    pub fn show(
        &mut self,
        id: Uuid,
        collection: Option<Arc<Collection>>,
        active_environment: Option<Uuid>,
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
        if self.id != Some(id) {
            self.active_tab = CollectionTab::Settings;
            self.selected_environment = None;
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
        self.variables
            .load(Some(VariableSet::Collection(id)), stored, window, cx);

        self.id = Some(id);
        self.collection = collection;
        self.active_environment = active_environment;
        self.sync_environment_selection(window, cx);
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

    /// Takes the variables edits waiting for their save delay.
    pub fn take_pending_variables(&mut self, cx: &App) -> Vec<(VariableSet, Vec<KeyValue>)> {
        [&mut self.variables, &mut self.environment_variables]
            .into_iter()
            .filter_map(|variables| variables.take_pending(cx))
            .collect()
    }

    /// The timer of a variables edit. Saves every pending edit when it fires.
    fn save_after_delay(&mut self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VARIABLES_SAVE_DELAY).await;
            this.update(cx, |this, cx| this.emit_pending_variables(cx))
                .ok();
        })
    }

    fn emit_pending_variables(&mut self, cx: &mut Context<Self>) {
        for (set, variables) in self.take_pending_variables(cx) {
            cx.emit(CollectionViewEvent::VariablesChanged { set, variables });
        }
    }

    fn select_tab(&mut self, tab: CollectionTab, cx: &mut Context<Self>) {
        self.emit_pending_variables(cx);
        self.active_tab = tab;
        cx.notify();
    }

    fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_2()
            .child(
                v_flex()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Use variables as {{name}} in this collection's requests. Changes save automatically.")
                    .child("The active environment's values override these."),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.variables.editor().clone()),
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
            .on_click(
                cx.listener(|this, ix: &usize, _, cx| this.select_tab(CollectionTab::ALL[*ix], cx)),
            )
            .children(CollectionTab::ALL.map(CollectionTab::label));

        let content = div().flex_1().min_h_0().p_4();
        let content = match self.active_tab {
            CollectionTab::Settings => content.child(self.render_settings(cx)),
            CollectionTab::Variables => content.child(self.render_variables(cx)),
            CollectionTab::Environments => content.child(self.render_environments(cx)),
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

fn variables_editor(
    window: &mut Window,
    cx: &mut Context<CollectionView>,
) -> Entity<KeyValueEditor> {
    cx.new(|cx| KeyValueEditor::with_column_labels("variable", "Variable", "Value", window, cx))
}
