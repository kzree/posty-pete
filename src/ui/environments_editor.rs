use std::collections::HashMap;

use gpui::{
    AnyElement, AppContext, Context, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    list::ListItem,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    v_flex,
};
use uuid::Uuid;

use crate::{
    domain::{environment::Environment, request::KeyValue},
    ui::{
        auto_saved_variables::{AutoSavedVariables, SAVE_DELAY},
        confirm_dialog::open_confirm_dialog,
        key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
        rename_dialog::open_rename_dialog,
    },
};

#[derive(Clone, Debug)]
pub enum EnvironmentsEditorEvent {
    Add,
    Rename {
        id: Uuid,
        name: String,
    },
    Delete(Uuid),
    /// `None` = no environment.
    Use(Option<Uuid>),
    VariablesChanged {
        id: Uuid,
        variables: Vec<KeyValue>,
    },
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
}

/// A list of environments with the selected one's variables next to it.
/// Variables auto-save; everything else is emitted for the parent to apply.
pub struct EnvironmentsEditor {
    options: EnvironmentsEditorOptions,
    environments: Vec<Environment>,
    active: Option<Uuid>,
    /// How many environments are based on each one, by id.
    linked: HashMap<Uuid, usize>,
    selected: Option<Uuid>,
    variables: AutoSavedVariables<Uuid>,
    _subscription: Subscription,
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
        let subscription = cx.subscribe(&editor, |this, _, event, cx| match event {
            KeyValueEditorEvent::Changed => {
                let save = cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SAVE_DELAY).await;
                    this.update(cx, |this, cx| this.emit_pending(cx)).ok();
                });
                this.variables.schedule(save);
            }
        });
        Self {
            options,
            environments: Vec::new(),
            active: None,
            linked: HashMap::new(),
            selected: None,
            variables: AutoSavedVariables::new(editor),
            _subscription: subscription,
        }
    }

    /// Shows `environments`, keeping the selection if it still exists.
    /// Variables being edited are kept; a pending edit of an environment no
    /// longer shown is emitted.
    pub fn show(
        &mut self,
        environments: Vec<Environment>,
        active: Option<Uuid>,
        linked: HashMap<Uuid, usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.environments = environments;
        self.active = active;
        self.linked = linked;
        self.sync_selection(window, cx);
        cx.notify();
    }

    /// Forgets the selection, so the next [`Self::show`] starts from the
    /// active or first environment. Take the pending edit first.
    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    pub fn select(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            self.sync_selection(window, cx);
            cx.notify();
        }
    }

    /// Focuses the first variable of the environment shown.
    pub fn focus_variables(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.variables
            .editor()
            .update(cx, |editor, cx| editor.focus_first_key(window, cx));
    }

    /// Takes the variables edit waiting for its save delay, as the
    /// environment id and its variables.
    pub fn take_pending(&mut self, cx: &gpui::App) -> Option<(Uuid, Vec<KeyValue>)> {
        self.variables.take_pending(cx)
    }

    fn emit_pending(&mut self, cx: &mut Context<Self>) {
        if let Some((id, variables)) = self.take_pending(cx) {
            cx.emit(EnvironmentsEditorEvent::VariablesChanged { id, variables });
        }
    }

    /// Keeps the selection on an existing environment and loads its
    /// variables, emitting a pending edit of the one shown before.
    fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = environment_to_select(&self.environments, self.selected, self.active);
        self.selected = id;
        if id != self.variables.set() {
            self.emit_pending(cx);
        }
        let environment = id.and_then(|id| find(&self.environments, id));
        let stored = environment.map_or(&[][..], |env| env.variables.as_slice());
        self.variables.load(id, stored, window, cx);
    }

    fn rename(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(environment) = find(&self.environments, id) else {
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
        let Some(environment) = find(&self.environments, id) else {
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
        let label = if active {
            format!("{name}, active")
        } else {
            name.clone()
        };
        ListItem::new(id)
            .rounded(cx.theme().radius)
            .text_sm()
            .selected(self.selected == Some(id))
            .when(usable, |item| item.check_icon(IconName::Check))
            .confirmed(active)
            .accessibility_label(label)
            .child(div().truncate().child(name))
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

    fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.options.hint),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.variables.editor().clone()),
            )
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
            .child(self.render_variables(cx))
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
    let (rename_view, delete_view) = (view.clone(), view.clone());
    menu.item(
        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
            rename_view
                .update(cx, |this, cx| this.rename(id, window, cx))
                .ok();
        }),
    )
    .item(
        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
            delete_view
                .update(cx, |this, cx| this.delete(id, window, cx))
                .ok();
        }),
    )
}

fn find(environments: &[Environment], id: Uuid) -> Option<&Environment> {
    environments.iter().find(|env| env.id == id)
}

/// The environment to show: `current` if it still exists, else the active
/// one, else the first.
fn environment_to_select(
    environments: &[Environment],
    current: Option<Uuid>,
    active: Option<Uuid>,
) -> Option<Uuid> {
    let exists = |id: &Uuid| find(environments, *id).is_some();
    current
        .filter(exists)
        .or(active.filter(exists))
        .or_else(|| environments.first().map(|env| env.id))
}

/// The delete confirmation's message. `linked` environments are based on it.
fn delete_message(name: &str, linked: usize) -> String {
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

    fn environments() -> (Vec<Environment>, Uuid, Uuid) {
        let environments = vec![Environment::new("Staging"), Environment::new("Production")];
        let (first, second) = (environments[0].id, environments[1].id);
        (environments, first, second)
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
        assert_eq!(environment_to_select(&[], None, None), None);
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
