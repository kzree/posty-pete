use std::{borrow::Cow, collections::HashMap, sync::Arc};

use gpui::{
    AnyElement, AppContext, Context, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
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
    environment::{self, Environment},
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

/// Accessible names stop after this many characters of a name or value.
const LABEL_MAX_CHARS: usize = 100;
/// Shown text stops after this many characters; it's truncated to the
/// width well before that.
const DISPLAY_MAX_CHARS: usize = 500;

/// A base's variables ready to show, built once per base snapshot.
struct InheritedRows {
    base: Arc<Environment>,
    title: SharedString,
    rows: Vec<InheritedRow>,
}

#[derive(Clone, Debug, PartialEq)]
struct InheritedRow {
    name: SharedString,
    shown_name: SharedString,
    value: SharedString,
    label: SharedString,
    overridden_label: SharedString,
}

impl InheritedRows {
    fn new(base: Arc<Environment>, options: BaseOptions) -> Self {
        let title = format!("From {} › {}", options.group, base.name).into();
        let rows = environment::inherited_variables(&base.variables)
            .into_iter()
            .map(|row| {
                let label = format!(
                    "{}: {}",
                    capped(row.name, LABEL_MAX_CHARS),
                    capped(row.value, LABEL_MAX_CHARS)
                );
                let name = SharedString::from(row.name.to_string());
                let shown_name = match capped(row.name, DISPLAY_MAX_CHARS) {
                    Cow::Borrowed(_) => name.clone(),
                    Cow::Owned(shown) => shown.into(),
                };
                InheritedRow {
                    name,
                    shown_name,
                    value: capped(row.value, DISPLAY_MAX_CHARS).into_owned().into(),
                    overridden_label: format!("{label}, overridden").into(),
                    label: label.into(),
                }
            })
            .collect();
        Self { base, title, rows }
    }
}

/// `text` up to `max_chars` characters, with an ellipsis if it's longer.
fn capped(text: &str, max_chars: usize) -> Cow<'_, str> {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => Cow::Owned(format!("{}…", &text[..end])),
        None => Cow::Borrowed(text),
    }
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
    /// The selected environment's base variables, while it has a base.
    inherited: Option<InheritedRows>,
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
            inherited: None,
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
        self.base_snapshot(environment)
            .map(|(options, base)| (options, &**base))
    }

    fn base_snapshot(&self, environment: &Environment) -> Option<(BaseOptions, &Arc<Environment>)> {
        let options = self.options.bases?;
        let id = environment.base?;
        let base = self.bases.iter().find(|base| base.id == id)?;
        Some((options, base))
    }

    /// Rebuilds the inherited rows only when the selected environment's base
    /// snapshot changed.
    fn refresh_inherited(&mut self) {
        let base = self
            .selected
            .and_then(|id| self.environments.find(id))
            .and_then(|environment| self.base_snapshot(environment));
        self.inherited = match base {
            None => None,
            Some((_, base))
                if self
                    .inherited
                    .as_ref()
                    .is_some_and(|inherited| Arc::ptr_eq(&inherited.base, base)) =>
            {
                return;
            }
            Some((options, base)) => Some(InheritedRows::new(base.clone(), options)),
        };
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
            .and(self.inherited.as_ref())
            .map(|inherited| self.render_inherited(inherited, cx).into_any_element());
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

    /// Whether each inherited row is overridden by an enabled own variable.
    fn overridden_rows(&self, inherited: &InheritedRows, cx: &gpui::App) -> Vec<bool> {
        let own: Vec<_> = self.variables.editor().read(cx).keys(cx).collect();
        let overridden =
            environment::overridden_names(own.iter().map(|(enabled, key)| (*enabled, &**key)));
        inherited
            .rows
            .iter()
            .map(|row| overridden.contains(&*row.name))
            .collect()
    }

    fn render_inherited(
        &self,
        inherited: &InheritedRows,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rows: Vec<_> = inherited
            .rows
            .iter()
            .zip(self.overridden_rows(inherited, cx))
            .enumerate()
            .map(|(ix, (row, overridden))| inherited_row(ix, row, overridden))
            .collect();
        let muted = cx.theme().muted_foreground;
        let body = if rows.is_empty() {
            div().child("No variables.").into_any_element()
        } else {
            v_flex().gap_1().children(rows).into_any_element()
        };
        v_flex()
            .id("environment-inherited")
            .role(gpui::Role::Group)
            .aria_label(inherited.title.clone())
            .pt_4()
            .gap_2()
            .text_sm()
            .text_color(muted)
            .child(div().text_xs().truncate().child(inherited.title.clone()))
            .child(body)
    }
}

impl Render for EnvironmentsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_inherited();
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

fn inherited_row(ix: usize, row: &InheritedRow, overridden: bool) -> impl IntoElement {
    let label = if overridden {
        &row.overridden_label
    } else {
        &row.label
    };
    h_flex()
        .id(("environment-inherited-row", ix))
        .aria_label(label.clone())
        .gap_4()
        .when(overridden, |row| {
            row.line_through().tooltip(|window, cx| {
                Tooltip::new("Overridden by this environment").build(window, cx)
            })
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(row.shown_name.clone()),
        )
        .child(div().flex_1().min_w_0().truncate().child(row.value.clone()))
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

    const BASES: BaseOptions = BaseOptions {
        group: "Shared",
        manage_label: "Manage shared environments…",
    };

    #[test]
    fn inherited_rows_are_named_once_and_labelled() {
        let mut base = Environment::new("Staging");
        let mut disabled = variable("token", "t");
        disabled.enabled = false;
        base.variables = vec![
            variable(" host ", "staging"),
            disabled,
            variable("host", "duplicate"),
        ];
        let inherited = InheritedRows::new(Arc::new(base), BASES);
        assert_eq!(inherited.title, "From Shared › Staging");
        assert_eq!(
            inherited.rows,
            [InheritedRow {
                name: "host".into(),
                shown_name: "host".into(),
                value: "staging".into(),
                label: "host: staging".into(),
                overridden_label: "host: staging, overridden".into(),
            }]
        );
    }

    #[test]
    fn long_inherited_values_are_capped() {
        let mut base = Environment::new("Staging");
        let long = "é".repeat(DISPLAY_MAX_CHARS * 2);
        base.variables = vec![variable("token", &long)];
        let inherited = InheritedRows::new(Arc::new(base), BASES);
        let row = &inherited.rows[0];
        let label_value = format!("{}…", "é".repeat(LABEL_MAX_CHARS));
        assert_eq!(row.label, format!("token: {label_value}"));
        assert_eq!(
            row.overridden_label,
            format!("token: {label_value}, overridden")
        );
        assert_eq!(row.value.chars().count(), DISPLAY_MAX_CHARS + 1);
        assert!(row.value.ends_with('…'));

        let mut base = Environment::new("Staging");
        base.variables = vec![variable(&long, "v")];
        let inherited = InheritedRows::new(Arc::new(base), BASES);
        let row = &inherited.rows[0];
        assert_eq!(row.name, long);
        assert_eq!(row.shown_name.chars().count(), DISPLAY_MAX_CHARS + 1);
    }

    #[test]
    fn capped_keeps_short_text() {
        assert!(matches!(capped("host", 4), Cow::Borrowed("host")));
        assert_eq!(capped("hosts", 4), "host…");
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

    mod views {
        use std::{cell::Cell, rc::Rc};

        use gpui::{Entity, TestAppContext, VisualTestContext};
        use gpui_component::input::InputState;

        use super::*;
        use crate::{
            test_support::{open_view, type_into},
            ui::auth_editor::AuthEditorOptions,
        };

        const OPTIONS: EnvironmentsEditorOptions = EnvironmentsEditorOptions {
            usable: true,
            empty_text: "No environments.",
            hint: "",
            auth_hint: "",
            auth: AuthEditorOptions {
                inherit: Some("Inherits."),
                none: "No auth.",
            },
            bases: Some(BASES),
        };

        struct Harness<'a> {
            editor: Entity<EnvironmentsEditor>,
            cx: &'a mut VisualTestContext,
            notified: Rc<Cell<usize>>,
        }

        impl<'a> Harness<'a> {
            fn open(cx: &'a mut TestAppContext) -> Self {
                let (editor, cx) = open_view(cx, |window, cx| {
                    EnvironmentsEditor::new(OPTIONS, window, cx)
                });
                let notified = Rc::new(Cell::new(0));
                let counter = notified.clone();
                cx.update(|_, cx| {
                    cx.observe(&editor, move |_, _| counter.set(counter.get() + 1))
                        .detach()
                });
                Self {
                    editor,
                    cx,
                    notified,
                }
            }

            fn show(&mut self, environments: EnvironmentList, bases: Vec<Arc<Environment>>) {
                self.editor.update_in(self.cx, |editor, window, cx| {
                    editor.show(
                        environments,
                        None,
                        HashMap::new(),
                        bases,
                        HashMap::new(),
                        window,
                        cx,
                    )
                });
            }

            fn select(&mut self, id: Uuid) {
                self.editor
                    .update_in(self.cx, |editor, window, cx| editor.select(id, window, cx));
            }

            fn selected(&mut self) -> Option<Uuid> {
                self.editor
                    .read_with(self.cx, |editor, _| editor.selected())
            }

            /// Takes the count of notifications since the last call.
            fn notifications(&mut self) -> usize {
                self.notified.replace(0)
            }

            fn variables(&mut self) -> Entity<KeyValueEditor> {
                self.editor
                    .read_with(self.cx, |editor, _| editor.variables.editor().clone())
            }

            fn variable_inputs(&mut self, ix: usize) -> (Entity<InputState>, Entity<InputState>) {
                let variables = self.variables();
                variables.read_with(self.cx, |variables, _| variables.row_inputs(ix))
            }

            fn shown_variables(&mut self) -> Vec<KeyValue> {
                let variables = self.variables();
                variables.read_with(self.cx, |variables, cx| variables.values(cx))
            }

            fn type_into(&mut self, input: &Entity<InputState>, text: &str) {
                type_into(input, text, self.cx);
            }

            /// The cached inherited rows' storage and base, as of the last render.
            fn inherited(&mut self) -> Option<(*const InheritedRow, Arc<Environment>)> {
                self.editor.read_with(self.cx, |editor, _| {
                    let inherited = editor.inherited.as_ref()?;
                    Some((inherited.rows.as_ptr(), inherited.base.clone()))
                })
            }

            fn inherited_names(&mut self) -> Vec<String> {
                self.editor.read_with(self.cx, |editor, _| {
                    editor.inherited.as_ref().map_or(Vec::new(), |inherited| {
                        inherited
                            .rows
                            .iter()
                            .map(|row| row.name.to_string())
                            .collect()
                    })
                })
            }

            fn overridden(&mut self) -> Vec<bool> {
                self.editor.read_with(self.cx, |editor, cx| {
                    let inherited = editor.inherited.as_ref().expect("inherited rows");
                    editor.overridden_rows(inherited, cx)
                })
            }
        }

        fn two_environments() -> (Vec<Arc<Collection>>, Uuid, Uuid, Uuid) {
            let (mut collections, _, collection_id, staging) = collections();
            let production = Environment::new("Production");
            let production_id = production.id;
            assert!(collection::modify(&mut collections, collection_id, |c| {
                c.environments.push(production);
                true
            }));
            (collections, collection_id, staging, production_id)
        }

        #[gpui::test]
        fn an_unrelated_change_keeps_the_selection_and_the_edit(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (mut collections, id, staging, production) = two_environments();
            h.show(current(&collections), Vec::new());
            assert_eq!(h.selected(), Some(staging));
            h.select(production);
            let (key, _) = h.variable_inputs(0);
            h.type_into(&key, "host");
            h.notifications();

            assert!(collection::modify(&mut collections, id, |c| {
                c.items.clear();
                true
            }));
            h.show(current(&collections), Vec::new());
            assert_eq!(h.notifications(), 0, "the editor reloaded");
            assert_eq!(h.selected(), Some(production));
            assert_eq!(key.read_with(h.cx, |key, _| key.value()), "host");
            let pending = h
                .editor
                .update(h.cx, |editor, cx| editor.take_pending_variables(cx));
            assert_eq!(pending, Some((production, vec![variable("host", "")])));
        }

        #[gpui::test]
        fn a_changed_environment_reloads_keeping_the_selection(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (mut collections, id, _, production) = two_environments();
            h.show(current(&collections), Vec::new());
            h.select(production);
            h.notifications();

            let variables = vec![variable("host", "example.com")];
            assert!(collection::set_environment_variables(
                &mut collections,
                id,
                production,
                variables.clone(),
            ));
            h.show(current(&collections), Vec::new());
            assert!(h.notifications() > 0);
            assert_eq!(h.selected(), Some(production));
            assert_eq!(h.shown_variables(), variables);
        }

        #[gpui::test]
        fn an_edited_shared_environment_reloads(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (shown, _, second) = environments();
            let EnvironmentList::Shared(mut stored) = shown.clone() else {
                unreachable!()
            };
            h.show(shown, Vec::new());
            h.select(second);
            h.notifications();

            h.show(EnvironmentList::Shared(stored.clone()), Vec::new());
            assert_eq!(h.notifications(), 0);

            let variables = vec![variable("host", "example.com")];
            assert!(environment::set_variables(
                &mut stored,
                second,
                variables.clone()
            ));
            h.show(EnvironmentList::Shared(stored), Vec::new());
            assert!(h.notifications() > 0);
            assert_eq!(h.selected(), Some(second));
            assert_eq!(h.shown_variables(), variables);
        }

        fn based_environment() -> (EnvironmentList, Vec<Arc<Environment>>) {
            let mut base = Environment::new("Shared staging");
            base.variables = vec![variable("host", "staging"), variable("token", "t")];
            let mut environment = Environment::new("Staging");
            environment.base = Some(base.id);
            (
                EnvironmentList::Shared(vec![Arc::new(environment)]),
                vec![Arc::new(base)],
            )
        }

        #[gpui::test]
        fn typing_an_own_value_keeps_the_inherited_rows(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (list, bases) = based_environment();
            h.show(list, bases.clone());
            let (rows, base) = h.inherited().expect("inherited rows");
            assert!(Arc::ptr_eq(&base, &bases[0]));
            assert_eq!(h.inherited_names(), ["host", "token"]);

            let (key, value) = h.variable_inputs(0);
            h.type_into(&key, "host");
            h.type_into(&value, "localhost");
            assert!(h.notifications() > 0);
            assert_eq!(h.inherited().map(|(rows, _)| rows), Some(rows));
        }

        #[gpui::test]
        fn own_keys_matching_a_base_name_override_it(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (list, bases) = based_environment();
            h.show(list, bases);
            assert_eq!(h.overridden(), [false, false]);

            let (key, _) = h.variable_inputs(0);
            h.type_into(&key, "token");
            assert_eq!(h.overridden(), [false, true]);

            let variables = h.variables();
            variables.update(h.cx, |variables, cx| variables.toggle_row(0, false, cx));
            assert_eq!(h.overridden(), [false, false]);
            variables.update(h.cx, |variables, cx| variables.toggle_row(0, true, cx));
            assert_eq!(h.overridden(), [false, true]);

            let (key, _) = h.variable_inputs(1);
            h.type_into(&key, "host");
            assert_eq!(h.overridden(), [true, true]);
        }

        #[gpui::test]
        fn a_new_base_snapshot_rebuilds_the_rows(cx: &mut TestAppContext) {
            let mut h = Harness::open(cx);
            let (list, mut bases) = based_environment();
            h.show(list.clone(), bases.clone());
            let (rows, _) = h.inherited().expect("inherited rows");

            let base_id = bases[0].id;
            assert!(environment::set_variables(
                &mut bases,
                base_id,
                vec![variable("region", "eu")],
            ));
            h.show(list, bases.clone());
            let (new_rows, base) = h.inherited().expect("inherited rows");
            assert!(Arc::ptr_eq(&base, &bases[0]));
            assert_ne!(new_rows, rows);
            assert_eq!(h.inherited_names(), ["region"]);
        }
    }
}
