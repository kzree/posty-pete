use std::{collections::HashMap, sync::Arc};

use gpui::{
    AnyElement, AppContext, Context, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    list::ListItem,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    tab::TabBar,
    tooltip::Tooltip,
    v_flex,
};
use uuid::Uuid;

use domain::{
    auth::InheritNote,
    collection::Collection,
    defined_names::DefinedNames,
    environment::{self, Environment, InheritedVariable},
    request::{Auth, KeyValue},
};

use crate::ui::{
    auth_editor::{AuthEditor, AuthEditorEvent, AuthEditorOptions},
    auto_saved::{AutoSavedAuth, AutoSavedVariables, SAVE_DELAY},
    confirm_dialog::open_confirm_dialog,
    key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
    rename_dialog::open_rename_dialog,
};

#[derive(Clone, Debug)]
pub enum EnvironmentsEditorEvent {
    Add,
    Rename {
        id: Uuid,
        name: String,
    },
    Duplicate(Uuid),
    Delete(Uuid),
    /// `None` = no environment.
    Use(Option<Uuid>),
    VariablesChanged {
        id: Uuid,
        variables: Vec<KeyValue>,
    },
    AuthChanged {
        id: Uuid,
        auth: Auth,
    },
    /// `base: None` unlinks the environment.
    BaseChanged {
        id: Uuid,
        base: Option<Uuid>,
    },
    /// Open the bases' editor, with `Uuid` selected if given.
    ManageBases(Option<Uuid>),
    /// The environment shown changed; see [`EnvironmentsEditor::selected`].
    SelectionChanged,
}

/// What differs between the places environments are edited.
#[derive(Clone, Copy)]
pub struct EnvironmentsEditorOptions {
    /// Whether an environment can be made active: the active one is checked
    /// and the row menu has "Use environment".
    pub usable: bool,
    /// Shown with "Add environment" when there are none.
    pub empty_text: &'static str,
    /// Shown above the selected environment's variables.
    pub hint: &'static str,
    /// Shown above the selected environment's auth.
    pub auth_hint: &'static str,
    pub auth: AuthEditorOptions,
    /// Whether environments can be based on others; `None` hides it.
    pub bases: Option<BaseOptions>,
}

/// How environments based on others are labelled.
#[derive(Clone, Copy)]
pub struct BaseOptions {
    /// What the bases are, as in "Shared › Staging".
    pub group: &'static str,
    /// The menu item emitting [`EnvironmentsEditorEvent::ManageBases`].
    pub manage_label: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Variables,
    Authorization,
}

impl Pane {
    const ALL: [Pane; 2] = [Self::Variables, Self::Authorization];

    fn label(self) -> &'static str {
        match self {
            Self::Variables => "Variables",
            Self::Authorization => "Authorization",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|pane| *pane == self).unwrap_or(0)
    }
}

/// The environments an [`EnvironmentsEditor`] shows, shared with their owner
/// rather than copied.
#[derive(Clone)]
pub enum EnvironmentList {
    None,
    /// A collection's environments.
    Collection(Arc<Collection>),
    Shared(Vec<Arc<Environment>>),
}

impl EnvironmentList {
    fn iter(&self) -> impl Iterator<Item = &Environment> {
        let (owned, shared): (&[Environment], &[Arc<Environment>]) = match self {
            Self::None => (&[], &[]),
            Self::Collection(collection) => (&collection.environments, &[]),
            Self::Shared(environments) => (&[], environments),
        };
        owned.iter().chain(shared.iter().map(|env| &**env))
    }

    fn find(&self, id: Uuid) -> Option<&Environment> {
        self.iter().find(|env| env.id == id)
    }

    fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Whether both hold the same environments. Shared environments are
    /// compared by pointer: while one is held here, a change copies it.
    /// A collection's are compared by value unless it's the same snapshot.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Collection(a), Self::Collection(b)) => {
                Arc::ptr_eq(a, b) || a.environments == b.environments
            }
            (Self::Shared(a), Self::Shared(b)) => same_arcs(a, b),
            _ => self.is_empty() && other.is_empty(),
        }
    }
}

fn same_arcs(a: &[Arc<Environment>], b: &[Arc<Environment>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Arc::ptr_eq(a, b))
}

/// A list of environments with the selected one's variables or auth next to
/// it. Variables and auth auto-save; everything else is emitted for the
/// parent to apply.
pub struct EnvironmentsEditor {
    options: EnvironmentsEditorOptions,
    environments: EnvironmentList,
    active: Option<Uuid>,
    /// How many environments are based on each one, by id.
    linked: HashMap<Uuid, usize>,
    /// The environments others can be based on.
    bases: Vec<Arc<Environment>>,
    /// What each environment's auth can reference, by id.
    defined: HashMap<Uuid, DefinedNames>,
    selected: Option<Uuid>,
    pane: Pane,
    variables: AutoSavedVariables<Uuid>,
    auth: AutoSavedAuth<Uuid>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<EnvironmentsEditorEvent> for EnvironmentsEditor {}

impl EnvironmentsEditor {
    pub fn new(
        options: EnvironmentsEditorOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            KeyValueEditor::with_column_labels("variable", "Variable", "Value", window, cx)
        });
        let variables_sub = cx.subscribe(&editor, |this, _, event, cx| match event {
            KeyValueEditorEvent::Changed => {
                let save = this.save_after_delay(cx);
                this.variables.schedule(save);
                // The inherited list marks what the rows override.
                cx.notify();
            }
        });
        let auth = cx.new(|cx| AuthEditor::new(options.auth, window, cx));
        let auth_sub = cx.subscribe(&auth, |this, _, event, cx| match event {
            AuthEditorEvent::Changed => {
                let save = this.save_after_delay(cx);
                this.auth.schedule(save);
                // The base's auth reads as inherited or overridden.
                cx.notify();
            }
            AuthEditorEvent::OpenSource(_) => {}
        });
        Self {
            options,
            environments: EnvironmentList::None,
            active: None,
            linked: HashMap::new(),
            bases: Vec::new(),
            defined: HashMap::new(),
            selected: None,
            pane: Pane::Variables,
            variables: AutoSavedVariables::new(editor),
            auth: AutoSavedAuth::new(auth),
            _subscriptions: [variables_sub, auth_sub],
        }
    }

    /// Shows `environments`, keeping the selection if it still exists.
    /// Variables being edited are kept; a pending edit of an environment no
    /// longer shown is emitted. `bases` are what environments can be based on;
    /// `defined` is what each one's auth can reference, for the unresolved warning.
    /// Nothing is reloaded if none of it changed.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        environments: EnvironmentList,
        active: Option<Uuid>,
        linked: HashMap<Uuid, usize>,
        bases: Vec<Arc<Environment>>,
        defined: HashMap<Uuid, DefinedNames>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // No selection: shown for the first time or after `clear_selection`.
        let unchanged = self.selected.is_some()
            && self.environments.same(&environments)
            && self.active == active
            && same_arcs(&self.bases, &bases)
            && self.linked == linked
            && self.defined == defined;
        // Keeps the newest snapshot, so older ones can be freed.
        self.environments = environments;
        self.bases = bases;
        if unchanged {
            return;
        }
        let previous = self.selected;
        self.active = active;
        self.linked = linked;
        self.defined = defined;
        self.sync_selection(window, cx);
        self.emit_selected_if_changed(previous, cx);
        cx.notify();
    }

    /// Forgets the selection, so the next [`Self::show`] starts from the
    /// active or first environment. Take the pending edit first.
    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    pub fn select(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.selected;
        if previous != Some(id) {
            self.selected = Some(id);
            self.sync_selection(window, cx);
            self.emit_selected_if_changed(previous, cx);
            cx.notify();
        }
    }

    /// The environment shown.
    pub fn selected(&self) -> Option<Uuid> {
        self.selected
    }

    /// Shows the variables and focuses the first one.
    pub fn focus_variables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select_pane(Pane::Variables, cx);
        self.variables
            .editor()
            .update(cx, |editor, cx| editor.focus_first_key(window, cx));
    }

    pub fn show_auth(&mut self, cx: &mut Context<Self>) {
        self.select_pane(Pane::Authorization, cx);
    }

    /// Takes the variables edit waiting for its save delay, as the
    /// environment id and its variables.
    pub fn take_pending_variables(&mut self, cx: &gpui::App) -> Option<(Uuid, Vec<KeyValue>)> {
        self.variables.take_pending(cx)
    }

    /// Takes the auth edit waiting for its save delay, as the environment id
    /// and its auth.
    pub fn take_pending_auth(&mut self, cx: &gpui::App) -> Option<(Uuid, Auth)> {
        self.auth.take_pending(cx)
    }

    fn save_after_delay(&mut self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| this.emit_pending(cx)).ok();
        })
    }

    fn select_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        if self.pane != pane {
            self.emit_pending(cx);
            self.pane = pane;
            cx.notify();
        }
    }

    fn emit_selected_if_changed(&self, previous: Option<Uuid>, cx: &mut Context<Self>) {
        if self.selected != previous {
            cx.emit(EnvironmentsEditorEvent::SelectionChanged);
        }
    }

    fn emit_pending(&mut self, cx: &mut Context<Self>) {
        if let Some((id, variables)) = self.take_pending_variables(cx) {
            cx.emit(EnvironmentsEditorEvent::VariablesChanged { id, variables });
        }
        if let Some((id, auth)) = self.take_pending_auth(cx) {
            cx.emit(EnvironmentsEditorEvent::AuthChanged { id, auth });
        }
    }

    /// Keeps the selection on an existing environment and loads its
    /// variables and auth, emitting pending edits of the one shown before.
    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = environment_to_select(&self.environments, self.selected, self.active);
        self.selected = id;
        if id != self.variables.set() || id != self.auth.set() {
            self.emit_pending(cx);
        }
        let inherit_note = id
            .and_then(|id| self.environments.find(id))
            .and_then(|env| self.base_of(env))
            .and_then(|(_, base)| environment::base_auth_note(&Auth::Inherit, base))
            .map(InheritNote::plain);
        let environment = id.and_then(|id| self.environments.find(id));
        let empty = Vec::new();
        let stored = environment.map_or(&empty, |env| &env.variables);
        self.variables.load(id, stored, window, cx);
        let inherit = Auth::Inherit;
        let stored = environment.map_or(&inherit, |env| &env.auth);
        self.auth.load(id, stored, window, cx);
        let defined = id.and_then(|id| self.defined.get(&id).cloned());
        self.auth.editor().update(cx, |editor, cx| {
            editor.set_inherit_note(inherit_note, cx);
            editor.set_defined_names(defined, cx);
        });
    }

    /// The existing base of `environment`, when bases are shown.
    fn base_of(&self, environment: &Environment) -> Option<(BaseOptions, &Environment)> {
        let options = self.options.bases?;
        let base = environment::find(&self.bases, environment.base?)?;
        Some((options, base))
    }

    fn rename(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(environment) = self.environments.find(id) else {
            return;
        };
        let view = cx.weak_entity();
        open_rename_dialog(
            "Rename environment",
            environment.name.clone(),
            "Save",
            move |name, _, cx| {
                view.update(cx, |_, cx| {
                    cx.emit(EnvironmentsEditorEvent::Rename { id, name })
                })
                .ok();
            },
            window,
            cx,
        );
    }

    fn delete(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(environment) = self.environments.find(id) else {
            return;
        };
        let linked = self.linked.get(&id).copied().unwrap_or(0);
        let view = cx.weak_entity();
        open_confirm_dialog(
            "Delete environment?",
            delete_message(&environment.name, linked),
            "Delete",
            move |_, cx| {
                view.update(cx, |_, cx| cx.emit(EnvironmentsEditorEvent::Delete(id)))
                    .ok();
            },
            window,
            cx,
        );
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .max_w(px(480.))
            .items_start()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.options.empty_text),
            )
            .child(
                Button::new("environment-add")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add environment")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(EnvironmentsEditorEvent::Add))),
            )
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let rows: Vec<_> = self
            .environments
            .iter()
            .map(|environment| self.render_row(environment, view.clone(), cx))
            .collect();
        v_flex()
            .w(px(200.))
            .flex_none()
            .h_full()
            .gap_1()
            .child(
                Button::new("environment-new")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .label("New environment")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(EnvironmentsEditorEvent::Add))),
            )
            .child(
                v_flex()
                    .id("environment-list")
                    .role(gpui::Role::List)
                    .aria_label("Environments")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
    }

    fn render_row(
        &self,
        environment: &Environment,
        view: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = environment.id;
        let usable = self.options.usable;
        let active = usable && self.active == Some(id);
        let name = environment.name.clone();
        let base = self
            .base_of(environment)
            .map(|(options, base)| format!("{} › {}", options.group, base.name));
        let mut label = name.clone();
        if active {
            label.push_str(", active");
        }
        if let Some(base) = &base {
            label.push_str(&format!(", based on {base}"));
        }
        ListItem::new(id)
            .rounded(cx.theme().radius)
            .text_sm()
            .selected(self.selected == Some(id))
            .when(usable, |item| item.check_icon(IconName::Check))
            .confirmed(active)
            .accessibility_label(label)
            .child(row_label(name, base, cx))
            .on_click(cx.listener(move |this, _, window, cx| this.select(id, window, cx)))
            .context_menu(move |menu, _, _| {
                let menu = if usable {
                    use_items(&view, id, active, menu)
                } else {
                    menu
                };
                edit_items(&view, id, menu)
            })
            .into_any_element()
    }

    fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.and_then(|id| self.environments.find(id));
        let base_select = selected
            .zip(self.options.bases)
            .map(|(environment, options)| {
                self.render_base_select(environment, options, cx)
                    .into_any_element()
            });
        let tabs = TabBar::new("environment-pane")
            .segmented()
            .small()
            .selected_index(self.pane.index())
            .on_click(cx.listener(|this, ix: &usize, _, cx| this.select_pane(Pane::ALL[*ix], cx)))
            .children(Pane::ALL.map(Pane::label));
        let (hint, content) = match self.pane {
            Pane::Variables => (
                self.options.hint,
                self.render_variables(selected, cx).into_any_element(),
            ),
            Pane::Authorization => (
                self.options.auth_hint,
                self.render_auth(selected, cx).into_any_element(),
            ),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_2()
            .child(h_flex().child(tabs))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
            .children(base_select)
            .child(content)
    }

    fn render_variables(
        &self,
        selected: Option<&Environment>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let inherited = selected
            .and_then(|environment| self.base_of(environment))
            .map(|(options, base)| self.render_inherited(options, base, cx).into_any_element());
        v_flex()
            .id("environment-variables")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(self.variables.editor().clone())
            .children(inherited)
    }

    fn render_auth(
        &self,
        selected: Option<&Environment>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let own = self.auth.editor().read(cx).auth(cx);
        let overridden = selected
            .and_then(|environment| self.base_of(environment))
            .filter(|_| !own.is_inherit())
            .and_then(|(_, base)| environment::base_auth_note(&own, base));
        v_flex()
            .id("environment-auth")
            .flex_1()
            .min_h_0()
            .gap_3()
            .overflow_y_scrollbar()
            .children(overridden.map(|note| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(note)
            }))
            .child(self.auth.editor().clone())
    }

    fn render_base_select(
        &self,
        environment: &Environment,
        options: BaseOptions,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = environment.id;
        let base = self.base_of(environment).map(|(_, base)| base);
        let label = base.map_or("None".to_string(), |base| base.name.clone());
        let base = base.map(|base| base.id);
        let choices: Vec<_> = self
            .bases
            .iter()
            .map(|env| (env.id, env.name.clone()))
            .collect();
        let view = cx.weak_entity();
        h_flex().gap_2().text_sm().child("Based on").child(
            Button::new("environment-base")
                .outline()
                .small()
                .label(label.clone())
                .dropdown_caret(true)
                .max_w(px(240.))
                .accessibility_label(format!("Based on: {label}"))
                .dropdown_menu(move |menu: PopupMenu, _, _| {
                    base_items(&view, id, base, &choices, options, menu)
                }),
        )
    }

    fn render_inherited(
        &self,
        options: BaseOptions,
        base: &Environment,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let own = self.variables.editor().read(cx).values(cx);
        let rows = environment::inherited_variables(&base.variables, &own);
        let muted = cx.theme().muted_foreground;
        let body = if rows.is_empty() {
            div().child("No variables.").into_any_element()
        } else {
            v_flex()
                .gap_1()
                .children(rows.into_iter().enumerate().map(inherited_row))
                .into_any_element()
        };
        v_flex()
            .id("environment-inherited")
            .role(gpui::Role::Group)
            .aria_label(format!("From {} › {}", options.group, base.name))
            .pt_4()
            .gap_2()
            .text_sm()
            .text_color(muted)
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .child(format!("From {} › {}", options.group, base.name)),
            )
            .child(body)
    }
}

impl Render for EnvironmentsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.environments.is_empty() {
            return div()
                .size_full()
                .child(self.render_empty(cx))
                .into_any_element();
        }
        h_flex()
            .size_full()
            .items_start()
            .gap_4()
            .child(self.render_list(cx))
            .child(self.render_details(cx))
            .into_any_element()
    }
}

fn use_items(
    view: &WeakEntity<EnvironmentsEditor>,
    id: Uuid,
    active: bool,
    menu: PopupMenu,
) -> PopupMenu {
    let (label, use_id) = if active {
        ("Stop using environment", None)
    } else {
        ("Use environment", Some(id))
    };
    let view = view.clone();
    menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
        view.update(cx, |_, cx| cx.emit(EnvironmentsEditorEvent::Use(use_id)))
            .ok();
    }))
    .separator()
}

fn edit_items(view: &WeakEntity<EnvironmentsEditor>, id: Uuid, menu: PopupMenu) -> PopupMenu {
    let (rename_view, duplicate_view, delete_view) = (view.clone(), view.clone(), view.clone());
    menu.item(
        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
            rename_view
                .update(cx, |this, cx| this.rename(id, window, cx))
                .ok();
        }),
    )
    .item(PopupMenuItem::new("Duplicate").on_click(move |_, _, cx| {
        duplicate_view
            .update(cx, |_, cx| cx.emit(EnvironmentsEditorEvent::Duplicate(id)))
            .ok();
    }))
    .item(
        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
            delete_view
                .update(cx, |this, cx| this.delete(id, window, cx))
                .ok();
        }),
    )
}

/// The list row's name, with what it's based on, if anything, below it.
fn row_label(
    name: String,
    base: Option<String>,
    cx: &mut Context<EnvironmentsEditor>,
) -> impl IntoElement {
    let base = base.map(|base| {
        let tooltip = format!("Based on {base}");
        div()
            .id("environment-base-marker")
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .truncate()
            .child(base)
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
    });
    v_flex()
        .min_w_0()
        .child(div().truncate().child(name))
        .children(base)
}

fn base_items(
    view: &WeakEntity<EnvironmentsEditor>,
    id: Uuid,
    current: Option<Uuid>,
    choices: &[(Uuid, String)],
    options: BaseOptions,
    menu: PopupMenu,
) -> PopupMenu {
    let choose = |label: String, base: Option<Uuid>| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .checked(current == base)
            .on_click(move |_, _, cx| {
                view.update(cx, |_, cx| {
                    cx.emit(EnvironmentsEditorEvent::BaseChanged { id, base })
                })
                .ok();
            })
    };
    let menu = menu
        .max_w(px(360.))
        .scrollable(true)
        .item(choose("None".to_string(), None));
    let menu = choices.iter().fold(menu, |menu, (base, name)| {
        menu.item(choose(name.clone(), Some(*base)))
    });
    let view = view.clone();
    menu.separator().item(
        PopupMenuItem::new(options.manage_label).on_click(move |_, _, cx| {
            view.update(cx, |_, cx| {
                cx.emit(EnvironmentsEditorEvent::ManageBases(current))
            })
            .ok();
        }),
    )
}

fn inherited_row((ix, row): (usize, InheritedVariable)) -> impl IntoElement {
    let InheritedVariable {
        name,
        value,
        overridden,
    } = row;
    let label = if overridden {
        format!("{name}: {value}, overridden")
    } else {
        format!("{name}: {value}")
    };
    h_flex()
        .id(("environment-inherited-row", ix))
        .aria_label(label)
        .gap_4()
        .when(overridden, |row| {
            row.line_through().tooltip(|window, cx| {
                Tooltip::new("Overridden by this environment").build(window, cx)
            })
        })
        .child(div().flex_1().min_w_0().truncate().child(name))
        .child(div().flex_1().min_w_0().truncate().child(value))
}

/// The environment to show: `current` if it still exists, else the active
/// one, else the first.
fn environment_to_select(
    environments: &EnvironmentList,
    current: Option<Uuid>,
    active: Option<Uuid>,
) -> Option<Uuid> {
    let exists = |id: &Uuid| environments.find(*id).is_some();
    current
        .filter(exists)
        .or(active.filter(exists))
        .or_else(|| environments.iter().next().map(|env| env.id))
}

/// The delete confirmation's message. `linked` environments are based on it.
pub fn delete_message(name: &str, linked: usize) -> String {
    let linked = match linked {
        0 => String::new(),
        1 => " It's used by 1 environment, which keeps its own values.".to_string(),
        n => format!(" It's used by {n} environments, which keep their own values."),
    };
    format!("“{name}” and its variables will be deleted.{linked} This can't be undone.")
}

#[cfg(test)]
mod tests {
    use super::*;

    use domain::{collection, request::Request};

    fn environments() -> (EnvironmentList, Uuid, Uuid) {
        let environments = vec![
            Arc::new(Environment::new("Staging")),
            Arc::new(Environment::new("Production")),
        ];
        let (first, second) = (environments[0].id, environments[1].id);
        (EnvironmentList::Shared(environments), first, second)
    }

    fn variable(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    /// Stored collections, and what the editor shows: the same snapshot.
    fn collections() -> (Vec<Arc<Collection>>, EnvironmentList, Uuid, Uuid) {
        let mut collection = Collection::new("API");
        collection.environments = vec![Environment::new("Staging")];
        let environment_id = collection.environments[0].id;
        collection
            .items
            .push(collection::Item::Request(collection::SavedRequest::new(
                "List",
                Request::default(),
            )));
        let collection = Arc::new(collection);
        let shown = EnvironmentList::Collection(collection.clone());
        let collection_id = collection.id;
        (vec![collection], shown, collection_id, environment_id)
    }

    fn current(collections: &[Arc<Collection>]) -> EnvironmentList {
        EnvironmentList::Collection(collections[0].clone())
    }

    #[test]
    fn editing_an_unrelated_request_keeps_the_environments() {
        let (mut collections, shown, id, _) = collections();
        assert!(collection::modify(&mut collections, id, |collection| {
            collection.items.clear();
            true
        }));
        assert!(shown.same(&current(&collections)));
    }

    #[test]
    fn an_environment_change_is_a_change() {
        let (mut collections, shown, id, environment_id) = collections();
        assert!(collection::set_environment_variables(
            &mut collections,
            id,
            environment_id,
            vec![variable("host", "example.com")],
        ));
        assert!(!shown.same(&current(&collections)));
    }

    #[test]
    fn shared_environments_compare_by_pointer() {
        let (shown, first, _) = environments();
        let EnvironmentList::Shared(mut stored) = shown.clone() else {
            unreachable!()
        };
        assert!(shown.same(&EnvironmentList::Shared(stored.clone())));
        assert!(environment::set_variables(
            &mut stored,
            first,
            vec![variable("host", "example.com")],
        ));
        assert!(!shown.same(&EnvironmentList::Shared(stored.clone())));
        stored.pop();
        assert!(!shown.same(&EnvironmentList::Shared(stored)));
    }

    #[test]
    fn no_environments_are_the_same_wherever_they_come_from() {
        assert!(EnvironmentList::None.same(&EnvironmentList::Shared(Vec::new())));
        let (_, shown, ..) = collections();
        assert!(!EnvironmentList::None.same(&shown));
    }

    #[test]
    fn the_selection_survives_an_environment_change() {
        let (shown, first, second) = environments();
        let EnvironmentList::Shared(mut stored) = shown else {
            unreachable!()
        };
        assert!(environment::set_variables(
            &mut stored,
            second,
            vec![variable("host", "example.com")],
        ));
        let next = EnvironmentList::Shared(stored);
        assert_eq!(
            environment_to_select(&next, Some(second), Some(first)),
            Some(second)
        );
    }

    #[test]
    fn keeps_the_current_environment() {
        let (environments, first, second) = environments();
        assert_eq!(
            environment_to_select(&environments, Some(second), Some(first)),
            Some(second)
        );
    }

    #[test]
    fn falls_back_to_the_active_then_the_first_environment() {
        let (environments, first, second) = environments();
        let gone = Some(Uuid::new_v4());
        assert_eq!(
            environment_to_select(&environments, gone, Some(second)),
            Some(second)
        );
        assert_eq!(
            environment_to_select(&environments, gone, gone),
            Some(first)
        );
        assert_eq!(
            environment_to_select(&environments, None, None),
            Some(first)
        );
    }

    #[test]
    fn selects_nothing_without_environments() {
        assert_eq!(
            environment_to_select(&EnvironmentList::None, None, None),
            None
        );
    }

    #[test]
    fn delete_message_mentions_linked_environments() {
        assert_eq!(
            delete_message("Staging", 0),
            "“Staging” and its variables will be deleted. This can't be undone."
        );
        assert_eq!(
            delete_message("Staging", 1),
            "“Staging” and its variables will be deleted. It's used by 1 environment, \
             which keeps its own values. This can't be undone."
        );
        assert_eq!(
            delete_message("Staging", 3),
            "“Staging” and its variables will be deleted. It's used by 3 environments, \
             which keep their own values. This can't be undone."
        );
    }
}
