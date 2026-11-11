use gpui::{
    AnyElement, Context, IntoElement, ParentElement, Styled, WeakEntity, Window, div, prelude::*,
    px,
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

use super::{CollectionTab, CollectionView, CollectionViewEvent, VariableSet};
use crate::{
    domain::{collection::Collection, environment::Environment},
    ui::{confirm_dialog::open_confirm_dialog, rename_dialog::open_rename_dialog},
};

impl CollectionView {
    /// Shows the Environments tab, with `environment_id` selected if given.
    pub fn open_environments(
        &mut self,
        environment_id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab(CollectionTab::Environments, cx);
        if let Some(id) = environment_id {
            self.select_environment(id, window, cx);
        }
    }

    /// Focuses the first variable of the environment shown.
    pub fn focus_environment_variables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.environment_variables
            .editor()
            .update(cx, |editor, cx| editor.focus_first_key(window, cx));
    }

    fn select_environment(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_environment != Some(id) {
            self.selected_environment = Some(id);
            self.sync_environment_selection(window, cx);
            cx.notify();
        }
    }

    /// Keeps the selection on an existing environment and loads its
    /// variables, emitting a pending edit of the one shown before.
    pub(super) fn sync_environment_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collection = self.collection.clone();
        let environment = collection.as_deref().and_then(|collection| {
            let id = environment_to_select(
                collection,
                self.selected_environment,
                self.active_environment,
            )?;
            collection.environment(id).map(|env| (collection.id, env))
        });
        self.selected_environment = environment.map(|(_, env)| env.id);
        let set = environment.map(|(collection_id, env)| VariableSet::Environment {
            collection_id,
            environment_id: env.id,
        });
        if set != self.environment_variables.set() {
            self.emit_pending_variables(cx);
        }
        let stored = environment.map_or(&[][..], |(_, env)| env.variables.as_slice());
        self.environment_variables.load(set, stored, window, cx);
    }

    fn environment_name(&self, id: Uuid) -> Option<String> {
        let environment = self.collection.as_ref()?.environment(id)?;
        Some(environment.name.clone())
    }

    fn add_environment(&mut self, cx: &mut Context<Self>) {
        if let Some(collection_id) = self.id {
            cx.emit(CollectionViewEvent::AddEnvironment { collection_id });
        }
    }

    fn use_environment(&mut self, environment_id: Option<Uuid>, cx: &mut Context<Self>) {
        if let Some(collection_id) = self.id {
            cx.emit(CollectionViewEvent::UseEnvironment {
                collection_id,
                environment_id,
            });
        }
    }

    fn rename_environment(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(collection_id), Some(name)) = (self.id, self.environment_name(id)) else {
            return;
        };
        let view = cx.weak_entity();
        open_rename_dialog(
            "Rename environment",
            name,
            "Save",
            move |name, _, cx| {
                view.update(cx, |_, cx| {
                    cx.emit(CollectionViewEvent::RenameEnvironment {
                        collection_id,
                        environment_id: id,
                        name,
                    })
                })
                .ok();
            },
            window,
            cx,
        );
    }

    fn delete_environment(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(collection_id), Some(name)) = (self.id, self.environment_name(id)) else {
            return;
        };
        let view = cx.weak_entity();
        open_confirm_dialog(
            "Delete environment?",
            format!("“{name}” and its variables will be deleted. This can't be undone."),
            "Delete",
            move |_, cx| {
                view.update(cx, |_, cx| {
                    cx.emit(CollectionViewEvent::DeleteEnvironment {
                        collection_id,
                        environment_id: id,
                    })
                })
                .ok();
            },
            window,
            cx,
        );
    }

    pub(super) fn render_environments(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(collection) = self.collection.as_deref() else {
            return div().into_any_element();
        };
        if collection.environments.is_empty() {
            return self.render_no_environments(cx).into_any_element();
        }
        h_flex()
            .size_full()
            .items_start()
            .gap_4()
            .child(self.render_environment_list(collection, cx))
            .child(self.render_environment_variables(cx))
            .into_any_element()
    }

    fn render_no_environments(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .max_w(px(480.))
            .items_start()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Environments hold different values for the same variables, such as a host for staging and production."),
            )
            .child(
                Button::new("environment-add")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add environment")
                    .on_click(cx.listener(|this, _, _, cx| this.add_environment(cx))),
            )
    }

    fn render_environment_list(
        &self,
        collection: &Collection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.weak_entity();
        let rows: Vec<_> = collection
            .environments
            .iter()
            .map(|environment| self.render_environment_row(environment, view.clone(), cx))
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
                    .on_click(cx.listener(|this, _, _, cx| this.add_environment(cx))),
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

    fn render_environment_row(
        &self,
        environment: &Environment,
        view: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = environment.id;
        let active = self.active_environment == Some(id);
        let name = environment.name.clone();
        let label = if active {
            format!("{name}, active")
        } else {
            name.clone()
        };
        ListItem::new(id)
            .rounded(cx.theme().radius)
            .text_sm()
            .selected(self.selected_environment == Some(id))
            .check_icon(IconName::Check)
            .confirmed(active)
            .accessibility_label(label)
            .child(div().truncate().child(name))
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_environment(id, window, cx)),
            )
            .context_menu(move |menu, _, _| environment_menu(&view, id, active, menu))
            .into_any_element()
    }

    fn render_environment_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("While this environment is active, its values override the collection's variables. Changes save automatically."),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(self.environment_variables.editor().clone()),
            )
    }
}

fn environment_menu(
    view: &WeakEntity<CollectionView>,
    id: Uuid,
    active: bool,
    menu: PopupMenu,
) -> PopupMenu {
    let (use_label, use_id) = if active {
        ("Stop using environment", None)
    } else {
        ("Use environment", Some(id))
    };
    let (use_view, rename_view, delete_view) = (view.clone(), view.clone(), view.clone());
    menu.item(PopupMenuItem::new(use_label).on_click(move |_, _, cx| {
        use_view
            .update(cx, |this, cx| this.use_environment(use_id, cx))
            .ok();
    }))
    .separator()
    .item(
        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
            rename_view
                .update(cx, |this, cx| this.rename_environment(id, window, cx))
                .ok();
        }),
    )
    .item(
        PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
            delete_view
                .update(cx, |this, cx| this.delete_environment(id, window, cx))
                .ok();
        }),
    )
}

/// The environment to show: `current` if it still exists, else the active
/// one, else the first.
fn environment_to_select(
    collection: &Collection,
    current: Option<Uuid>,
    active: Option<Uuid>,
) -> Option<Uuid> {
    let exists = |id: &Uuid| collection.environment(*id).is_some();
    current
        .filter(exists)
        .or(active.filter(exists))
        .or_else(|| collection.environments.first().map(|env| env.id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::collection::add_environment;

    /// A collection with two environments.
    fn collection() -> (Collection, Uuid, Uuid) {
        let mut collections = [Arc::new(Collection::new("API"))];
        let id = collections[0].id;
        let first = add_environment(&mut collections, id).unwrap();
        let second = add_environment(&mut collections, id).unwrap();
        let [collection] = collections;
        (Arc::unwrap_or_clone(collection), first, second)
    }

    #[test]
    fn keeps_the_current_environment() {
        let (collection, first, second) = collection();
        assert_eq!(
            environment_to_select(&collection, Some(second), Some(first)),
            Some(second)
        );
    }

    #[test]
    fn falls_back_to_the_active_then_the_first_environment() {
        let (collection, first, second) = collection();
        let gone = Some(Uuid::new_v4());
        assert_eq!(
            environment_to_select(&collection, gone, Some(second)),
            Some(second)
        );
        assert_eq!(environment_to_select(&collection, gone, gone), Some(first));
        assert_eq!(environment_to_select(&collection, None, None), Some(first));
    }

    #[test]
    fn selects_nothing_without_environments() {
        let collection = Collection::new("API");
        assert_eq!(environment_to_select(&collection, None, None), None);
    }
}
