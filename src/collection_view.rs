use std::{collections::HashMap, sync::Arc};

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
    domain::{
        collection::Collection,
        environment::Environment,
        request::{Auth, KeyValue},
    },
    ui::{
        auth_editor::{AuthEditor, AuthEditorEvent, AuthEditorOptions},
        auto_saved::{AutoSavedAuth, AutoSavedVariables, SAVE_DELAY},
        environments_editor::{
            BaseOptions, EnvironmentsEditor, EnvironmentsEditorEvent, EnvironmentsEditorOptions,
        },
        key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
    },
};

const ENVIRONMENTS_OPTIONS: EnvironmentsEditorOptions = EnvironmentsEditorOptions {
    usable: true,
    empty_text: "Environments hold different values for the same variables, such as a host for staging and production.",
    hint: "While this environment is active, its values override the collection's variables. Changes save automatically.",
    auth_hint: "While this environment is active, requests set to Inherit use its auth. Changes save automatically.",
    auth: AuthEditorOptions {
        inherit: Some("Uses the collection's auth, if any."),
        none: "Requests set to Inherit send no auth while this environment is active.",
    },
    bases: Some(BaseOptions {
        group: "Shared",
        manage_label: "Manage shared environments…",
    }),
};

const AUTH_OPTIONS: AuthEditorOptions = AuthEditorOptions {
    inherit: None,
    none: "This collection doesn't set authorization.",
};

/// The collection or environment whose variables or auth an editor edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTarget {
    Collection(Uuid),
    Environment {
        collection_id: Uuid,
        environment_id: Uuid,
    },
}

pub enum CollectionViewEvent {
    Rename {
        id: Uuid,
        name: String,
    },
    VariablesChanged {
        set: EditTarget,
        variables: Vec<KeyValue>,
    },
    AuthChanged {
        target: EditTarget,
        auth: Auth,
    },
    AddEnvironment {
        collection_id: Uuid,
    },
    RenameEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
        name: String,
    },
    DuplicateEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
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
    /// `base: None` unlinks the environment.
    SetEnvironmentBase {
        collection_id: Uuid,
        environment_id: Uuid,
        base: Option<Uuid>,
    },
    /// Open the shared environments page, with `Uuid` selected if given.
    ManageSharedEnvironments(Option<Uuid>),
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum CollectionTab {
    Settings,
    Variables,
    Authorization,
    Environments,
}

impl CollectionTab {
    const ALL: [CollectionTab; 4] = [
        Self::Settings,
        Self::Variables,
        Self::Authorization,
        Self::Environments,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Variables => "Variables",
            Self::Authorization => "Authorization",
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
    variables: AutoSavedVariables<EditTarget>,
    /// Keyed by the collection id.
    auth: AutoSavedAuth<Uuid>,
    environments: Entity<EnvironmentsEditor>,
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
            KeyValueEditorEvent::Changed => {
                let save = this.save_after_delay(cx);
                this.variables.schedule(save);
            }
        });
        let auth = cx.new(|cx| AuthEditor::new(AUTH_OPTIONS, window, cx));
        let auth_sub = cx.subscribe(&auth, |this, _, event, cx| match event {
            AuthEditorEvent::Changed => {
                let save = this.save_after_delay(cx);
                this.auth.schedule(save);
            }
            AuthEditorEvent::OpenSource(_) => {}
        });
        let environments = cx.new(|cx| EnvironmentsEditor::new(ENVIRONMENTS_OPTIONS, window, cx));
        let environments_sub = cx.subscribe(&environments, |this, _, event, cx| {
            this.on_environments_event(event, cx)
        });
        Self {
            id: None,
            collection: None,
            active_tab: CollectionTab::Settings,
            name,
            variables: AutoSavedVariables::new(variables),
            auth: AutoSavedAuth::new(auth),
            environments,
            _subscriptions: vec![name_sub, variables_sub, auth_sub, environments_sub],
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        self.id
    }

    /// Shows `collection` (`None` if `id` doesn't exist) with
    /// `active_environment` marked; its environments can be based on `shared`.
    /// For the collection already shown, an unsaved edit of the name and
    /// variables and auth being edited are kept.
    /// Switching collection emits pending edits first.
    pub fn show(
        &mut self,
        id: Uuid,
        collection: Option<Arc<Collection>>,
        active_environment: Option<Uuid>,
        shared: Vec<Arc<Environment>>,
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
            self.emit_pending(cx);
            self.environments
                .update(cx, |environments, _| environments.clear_selection());
        }
        if replace_input {
            let value = new_name.unwrap_or_default();
            self.name
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let no_variables = Vec::new();
        let stored = collection.as_ref().map_or(&no_variables, |c| &c.variables);
        self.variables
            .load(Some(EditTarget::Collection(id)), stored, window, cx);
        let no_auth = Auth::None;
        let stored_auth = collection.as_ref().map_or(&no_auth, |c| &c.auth);
        self.auth.load(Some(id), stored_auth, window, cx);

        let environments = collection
            .as_ref()
            .map_or_else(Vec::new, |c| c.environments.clone());
        self.environments.update(cx, |editor, cx| {
            editor.show(
                environments,
                active_environment,
                HashMap::new(),
                shared,
                window,
                cx,
            )
        });

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

    /// Shows the Environments tab, with `environment_id` selected if given.
    pub fn open_environments(
        &mut self,
        environment_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab(CollectionTab::Environments, cx);
        if let Some(id) = environment_id {
            self.environments
                .update(cx, |environments, cx| environments.select(id, window, cx));
        }
    }

    /// Shows the Authorization tab.
    pub fn open_auth(&mut self, cx: &mut Context<Self>) {
        self.select_tab(CollectionTab::Authorization, cx);
    }

    /// Shows the auth of the environment shown.
    pub fn show_environment_auth(&mut self, cx: &mut Context<Self>) {
        self.environments
            .update(cx, |environments, cx| environments.show_auth(cx));
    }

    /// Focuses the first variable of the environment shown.
    pub fn focus_environment_variables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.environments.update(cx, |environments, cx| {
            environments.focus_variables(window, cx)
        });
    }

    /// Takes the variables edits waiting for their save delay.
    pub fn take_pending_variables(&mut self, cx: &mut App) -> Vec<(EditTarget, Vec<KeyValue>)> {
        let mut pending: Vec<_> = self.variables.take_pending(cx).into_iter().collect();
        let environment = self.environments.update(cx, |environments, cx| {
            environments.take_pending_variables(cx)
        });
        if let (Some(collection_id), Some((environment_id, variables))) = (self.id, environment) {
            let set = EditTarget::Environment {
                collection_id,
                environment_id,
            };
            pending.push((set, variables));
        }
        pending
    }

    /// Takes the collection and environment auth edits waiting for their
    /// save delay.
    pub fn take_pending_auth(&mut self, cx: &mut App) -> Vec<(EditTarget, Auth)> {
        let mut pending: Vec<_> = self
            .auth
            .take_pending(cx)
            .map(|(id, auth)| (EditTarget::Collection(id), auth))
            .into_iter()
            .collect();
        let environment = self
            .environments
            .update(cx, |environments, cx| environments.take_pending_auth(cx));
        if let (Some(collection_id), Some((environment_id, auth))) = (self.id, environment) {
            let target = EditTarget::Environment {
                collection_id,
                environment_id,
            };
            pending.push((target, auth));
        }
        pending
    }

    fn on_environments_event(&mut self, event: &EnvironmentsEditorEvent, cx: &mut Context<Self>) {
        let Some(collection_id) = self.id else {
            return;
        };
        let event = match event.clone() {
            EnvironmentsEditorEvent::Add => CollectionViewEvent::AddEnvironment { collection_id },
            EnvironmentsEditorEvent::Rename { id, name } => {
                CollectionViewEvent::RenameEnvironment {
                    collection_id,
                    environment_id: id,
                    name,
                }
            }
            EnvironmentsEditorEvent::Duplicate(id) => CollectionViewEvent::DuplicateEnvironment {
                collection_id,
                environment_id: id,
            },
            EnvironmentsEditorEvent::Delete(id) => CollectionViewEvent::DeleteEnvironment {
                collection_id,
                environment_id: id,
            },
            EnvironmentsEditorEvent::Use(environment_id) => CollectionViewEvent::UseEnvironment {
                collection_id,
                environment_id,
            },
            EnvironmentsEditorEvent::VariablesChanged { id, variables } => {
                CollectionViewEvent::VariablesChanged {
                    set: EditTarget::Environment {
                        collection_id,
                        environment_id: id,
                    },
                    variables,
                }
            }
            EnvironmentsEditorEvent::AuthChanged { id, auth } => CollectionViewEvent::AuthChanged {
                target: EditTarget::Environment {
                    collection_id,
                    environment_id: id,
                },
                auth,
            },
            EnvironmentsEditorEvent::BaseChanged { id, base } => {
                CollectionViewEvent::SetEnvironmentBase {
                    collection_id,
                    environment_id: id,
                    base,
                }
            }
            EnvironmentsEditorEvent::ManageBases(id) => {
                CollectionViewEvent::ManageSharedEnvironments(id)
            }
            EnvironmentsEditorEvent::SelectionChanged => return,
        };
        cx.emit(event);
    }

    /// The timer of a collection variables or auth edit.
    fn save_after_delay(&mut self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| this.emit_pending(cx)).ok();
        })
    }

    fn emit_pending(&mut self, cx: &mut Context<Self>) {
        for (set, variables) in self.take_pending_variables(cx) {
            cx.emit(CollectionViewEvent::VariablesChanged { set, variables });
        }
        for (target, auth) in self.take_pending_auth(cx) {
            cx.emit(CollectionViewEvent::AuthChanged { target, auth });
        }
    }

    fn select_tab(&mut self, tab: CollectionTab, cx: &mut Context<Self>) {
        self.emit_pending(cx);
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

    fn render_auth(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_3()
            .child(
                v_flex()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Requests set to Inherit use this unless their environment sets its own.",
                    )
                    .child("Changes save automatically."),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.auth.editor().clone()),
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
            CollectionTab::Authorization => content.child(self.render_auth(cx)),
            CollectionTab::Environments => content.child(self.environments.clone()),
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
