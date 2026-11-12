use std::{collections::HashMap, sync::Arc};

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Role, StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use gpui_component::{StyledExt, v_flex};
use uuid::Uuid;

use domain::{
    defined_names::DefinedNames,
    environment::Environment,
    request::{Auth, KeyValue},
};

use crate::ui::{
    auth_editor::AuthEditorOptions,
    environments_editor::{
        EnvironmentList, EnvironmentsEditor, EnvironmentsEditorEvent, EnvironmentsEditorOptions,
    },
};

const OPTIONS: EnvironmentsEditorOptions = EnvironmentsEditorOptions {
    usable: false,
    empty_text: "Shared environments hold values, such as a staging host, that environments in any collection can be based on.",
    hint: "Environments based on this one use these values unless they set their own. Changes save automatically.",
    auth_hint: "Environments based on this one, and drafts using it, use this auth unless they set their own. Changes save automatically.",
    auth: AuthEditorOptions {
        inherit: Some("Uses the collection's auth, if any."),
        none: "Requests set to Inherit send no auth with this environment.",
    },
    bases: None,
};

/// The `/environments` page: edits the shared environments.
pub struct SharedEnvironmentsView {
    environments: Entity<EnvironmentsEditor>,
    _subscription: Subscription,
}

impl EventEmitter<EnvironmentsEditorEvent> for SharedEnvironmentsView {}

impl SharedEnvironmentsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let environments = cx.new(|cx| EnvironmentsEditor::new(OPTIONS, window, cx));
        let subscription = cx.subscribe(&environments, |_, _, event, cx| cx.emit(event.clone()));
        Self {
            environments,
            _subscription: subscription,
        }
    }

    /// Shows `environments`; `linked` counts the environments based on each,
    /// and `defined` is what each one's auth can reference.
    pub fn show(
        &mut self,
        environments: Vec<Arc<Environment>>,
        linked: HashMap<Uuid, usize>,
        defined: HashMap<Uuid, DefinedNames>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let environments = EnvironmentList::Shared(environments);
        self.environments.update(cx, |editor, cx| {
            editor.show(environments, None, linked, Vec::new(), defined, window, cx)
        });
    }

    pub fn select(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.environments
            .update(cx, |editor, cx| editor.select(id, window, cx));
    }

    /// The environment shown.
    pub fn selected(&self, cx: &App) -> Option<Uuid> {
        self.environments.read(cx).selected()
    }

    /// Shows the auth of the environment shown.
    pub fn show_auth(&mut self, cx: &mut Context<Self>) {
        self.environments
            .update(cx, |editor, cx| editor.show_auth(cx));
    }

    /// Focuses the first variable of the environment shown.
    pub fn focus_variables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.environments
            .update(cx, |editor, cx| editor.focus_variables(window, cx));
    }

    /// Takes the variables edit waiting for its save delay.
    pub fn take_pending_variables(&mut self, cx: &mut App) -> Option<(Uuid, Vec<KeyValue>)> {
        self.environments
            .update(cx, |editor, cx| editor.take_pending_variables(cx))
    }

    /// Takes the auth edit waiting for its save delay.
    pub fn take_pending_auth(&mut self, cx: &mut App) -> Option<(Uuid, Auth)> {
        self.environments
            .update(cx, |editor, cx| editor.take_pending_auth(cx))
    }
}

impl Render for SharedEnvironmentsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("shared-environments")
            .role(Role::Group)
            .aria_label("Shared environments")
            .size_full()
            .child(
                div()
                    .id("shared-environments-heading")
                    .role(Role::Heading)
                    .aria_level(1)
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .text_lg()
                    .font_semibold()
                    .child("Shared environments"),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_4()
                    .pt_2()
                    .pb_4()
                    .child(self.environments.clone()),
            )
    }
}
